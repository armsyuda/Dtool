use dtool_core::document::{Document, Units};
use dtool_core::history_store::HistoryStore;
use dtool_core::Session;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn session() -> Session { Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1)) }

fn create(session: &mut Session, name: &str) {
    session.run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
}

fn objects(session: &Session) -> Value { serde_json::to_value(&session.document.objects).unwrap() }

struct OwnedHistoryFile(PathBuf);
impl OwnedHistoryFile {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!("b16_history_{}_{}_{}.bin", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed));
        let path = std::env::temp_dir().join(name);
        std::fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
        Self(path)
    }
}
impl Drop for OwnedHistoryFile {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("b16_history_"));
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn chronological_real_edits_undo_and_redo_restore_document_states() {
    let mut session = session();
    create(&mut session, "first");
    let id = session.document.objects[0].id.0;
    let first = objects(&session);
    session.run_named("object.update", json!({"id": id, "pos": [20.0, 30.0]})).unwrap();
    let second = objects(&session);
    create(&mut session, "second");
    let third = objects(&session);
    let history = HistoryStore::from_undo(&session.undo);
    assert_eq!(history.list().iter().map(|entry| entry.index).collect::<Vec<_>>(), vec![0, 1, 2]);
    assert_eq!(history.list().iter().map(|entry| entry.label_key.as_str()).collect::<Vec<_>>(), vec!["history.object.create", "history.object.update", "history.object.create"]);
    drop(history);
    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(objects(&session), second);
    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(objects(&session), first);
    session.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(objects(&session), second);
    session.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(objects(&session), third);
}

#[test]
fn real_history_jump_scrub_changes_document_and_new_edit_discards_redo() {
    let mut session = session();
    create(&mut session, "first");
    let first = objects(&session);
    create(&mut session, "second");
    session.run_named("history.jump", json!({"index": 0})).unwrap();
    assert_eq!(objects(&session), first);
    assert!(session.undo.can_redo());
    create(&mut session, "replacement");
    assert!(!session.undo.can_redo());
    assert_eq!(session.document.objects.len(), 2);
    assert_eq!(session.document.objects[1].name, "replacement");
}

#[test]
fn real_snapshot_restore_restores_document_not_only_cursor_metadata() {
    let mut session = session();
    create(&mut session, "snapshot-source");
    let expected = objects(&session);
    let snapshot = session.run_named("history.snapshot.create", json!({"name": "acceptance"})).unwrap();
    let id = snapshot.get("id").or_else(|| snapshot.get("payload").and_then(|value| value.get("id"))).and_then(Value::as_u64).unwrap();
    create(&mut session, "later-edit");
    session.run_named("history.snapshot.restore", json!({"id": id})).unwrap();
    assert_eq!(objects(&session), expected);
}

#[test]
fn save_with_doc_is_false_by_default_and_explicitly_configurable() {
    let session = session();
    let mut history = HistoryStore::from_undo(&session.undo);
    assert!(!history.save_history_with_document());
    history.set_save_history_with_document(true);
    assert!(history.save_history_with_document());
    history.set_save_history_with_document(false);
    assert!(!history.save_history_with_document());
}

#[test]
fn owner_configured_max_steps_caps_actual_reachable_undo_not_only_estimate() {
    let mut session = session();
    session.run_named("history.budget.set", json!({"max_steps": 2})).unwrap();
    for name in ["first", "second", "third"] { create(&mut session, name); }
    let mut history = HistoryStore::from_undo(&session.undo);
    history.set_max_steps(2);
    let budget = history.budget();
    assert_eq!(budget.max_steps, 2);
    assert_eq!(budget.reachable_depth as usize, session.undo.applied_steps());
    assert!(budget.reachable_depth <= 2);
    drop(history);
    let mut reachable = 0;
    while session.undo.can_undo() {
        session.run_named("edit.undo", json!({})).unwrap();
        reachable += 1;
    }
    assert_eq!(reachable, budget.reachable_depth);
}

#[test]
fn memory_estimate_tracks_real_payload_growth_instead_of_constant_per_row() {
    let mut small = session();
    create(&mut small, "x");
    let small_budget = HistoryStore::from_undo(&small.undo).budget();
    let mut large = session();
    let name = "x".repeat(16384);
    create(&mut large, &name);
    let large_budget = HistoryStore::from_undo(&large.undo).budget();
    assert_eq!(small_budget.reachable_depth, large_budget.reachable_depth);
    assert!(large_budget.memory_estimate_bytes > small_budget.memory_estimate_bytes);
    assert!(large_budget.memory_estimate_bytes >= name.len() as u64);
}

#[test]
fn real_history_file_round_trips_chronological_entries() {
    let mut session = session();
    create(&mut session, "first");
    create(&mut session, "second");
    let history = HistoryStore::from_undo(&session.undo);
    let file = OwnedHistoryFile::new();
    history.save_to_path(&file.0).unwrap();
    let recovered = HistoryStore::recover(&file.0).unwrap();
    assert_eq!(recovered.list(), history.list());
    if recovered.budget().reachable_depth > 0 { assert!(recovered.can_undo()); }
}

#[test]
fn interrupted_recovery_never_advertises_unverified_reachable_steps() {
    let mut session = session();
    create(&mut session, "first");
    create(&mut session, "second");
    let history = HistoryStore::from_undo(&session.undo);
    let file = OwnedHistoryFile::new();
    history.save_to_path(&file.0).unwrap();
    let mut bytes = std::fs::read(&file.0).unwrap();
    bytes.truncate(bytes.len() - 3);
    std::fs::write(&file.0, bytes).unwrap();
    let attempt = std::panic::catch_unwind(|| HistoryStore::recover(&file.0));
    assert!(attempt.is_ok());
    if let Ok(recovered) = attempt.unwrap() {
        assert!(recovered.list().len() < history.list().len());
        assert!(recovered.budget().reachable_depth as usize <= recovered.list().len());
        if !recovered.can_undo() { assert_eq!(recovered.budget().reachable_depth, 0); }
    }
}

#[test]
fn snapshot_restore_does_not_lower_next_object_id() {
    let mut session = session();
    create(&mut session, "snapshot-source");
    let expected = objects(&session);
    let snapshot = session.run_named("history.snapshot.create", json!({"name": "acceptance"})).unwrap();
    let id = snapshot.get("id").or_else(|| snapshot.get("payload").and_then(|value| value.get("id"))).and_then(Value::as_u64).unwrap();
    create(&mut session, "later-edit");
    let high = session.document.next_object_id;
    assert!(high > 1);
    session.run_named("history.snapshot.restore", json!({"id": id})).unwrap();
    assert_eq!(objects(&session), expected);
    assert!(session.document.next_object_id >= high);
}

#[test]
fn jump_then_recover_applies_objects_and_stories() {
    let mut session = session();
    create(&mut session, "first");
    session.document.stories.push(dtool_core::document::Story {
        id: dtool_core::document::StoryId(1),
        text: "earlier".to_string(),
        ranges: vec![json!({"start": 0, "end": 7})],
        ..dtool_core::document::Story::default()
    });
    session.document.next_story_id = session.document.next_story_id.max(2);
    let earlier_objects = objects(&session);
    let earlier_stories = serde_json::to_value(&session.document.stories).unwrap();
    let snapshot = session.run_named("history.snapshot.create", json!({"name": "earlier"})).unwrap();
    let snapshot_id = snapshot.get("id").and_then(Value::as_u64).unwrap();
    let earlier_index = snapshot.get("history_index").and_then(Value::as_u64).unwrap();
    create(&mut session, "second");
    session.document.stories.push(dtool_core::document::Story {
        id: dtool_core::document::StoryId(2),
        text: "later".to_string(),
        ranges: vec![json!({"start": 0, "end": 5})],
        ..dtool_core::document::Story::default()
    });
    session.document.next_story_id = session.document.next_story_id.max(3);
    let later_objects = objects(&session);
    let later_stories = serde_json::to_value(&session.document.stories).unwrap();
    let later_high = session.document.next_object_id;
    let later_version = session.document.version;
    let later_generation = session.generation;
    assert_ne!(later_objects, earlier_objects);
    assert_ne!(later_stories, earlier_stories);
    session.run_named("history.jump", json!({"index": earlier_index})).unwrap();
    assert_eq!(objects(&session), earlier_objects);
    assert_eq!(serde_json::to_value(&session.document.stories).unwrap(), earlier_stories);
    assert!(session.document.next_object_id >= later_high);
    assert!(session.undo.can_redo());
    session.run_named("history.recover", json!({})).unwrap();
    assert_eq!(objects(&session), later_objects);
    assert_eq!(serde_json::to_value(&session.document.stories).unwrap(), later_stories);
    assert!(session.document.next_object_id >= later_high);
    assert!(session.document.version >= later_version);
    assert!(session.generation >= later_generation);
    assert!(session.undo.can_undo());
    assert!(!session.undo.can_redo() || session.undo.applied_steps() == session.undo.total_steps());
    let _ = snapshot_id;
}

