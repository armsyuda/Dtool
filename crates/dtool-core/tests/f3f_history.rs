use dtool_core::document::{Document, Units};
use dtool_core::history_store::HistoryStore;
use dtool_core::Session;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn session() -> Session { Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1)) }

fn create(session: &mut Session, name: &str) {
    session.run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
}

fn names(session: &Session) -> Vec<String> { session.document.objects.iter().map(|o| o.name.clone()).collect() }

struct TempFile(PathBuf);
impl TempFile {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("f3f_history_{}_{}.bin", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        Self(path)
    }
}
impl Drop for TempFile {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

#[test]
fn jump_back_then_forward_replays_redo() {
    let mut s = session();
    for n in ["a", "b", "c"] { create(&mut s, n); }
    s.run_named("history.jump", json!({"index": 0})).unwrap();
    assert_eq!(names(&s), vec!["a"]);
    let out = s.run_named("history.jump", json!({"index": 2})).unwrap();
    assert_eq!(names(&s), vec!["a", "b", "c"]);
    assert_eq!(out["cursor"], json!(2));
    assert!(s.run_named("history.jump", json!({"index": 3})).is_err());
    assert!(s.run_named("history.jump", json!({"index": 0, "x": 1})).is_err());
}

#[test]
fn list_reports_entries_and_cursor() {
    let mut s = session();
    let empty = s.run_named("history.list", json!({})).unwrap();
    assert_eq!(empty["cursor"], Value::Null);
    create(&mut s, "a");
    create(&mut s, "b");
    s.run_named("edit.undo", json!({})).unwrap();
    let out = s.run_named("history.list", json!({})).unwrap();
    assert_eq!(out["entries"].as_array().unwrap().len(), 2);
    assert_eq!(out["cursor"], json!(0));
    assert_eq!(out["can_redo"], json!(true));
}

#[test]
fn snapshot_restore_is_one_undo_step_and_list_shows_it() {
    let mut s = session();
    create(&mut s, "a");
    let snap = s.run_named("history.snapshot.create", json!({"name": "keep"})).unwrap();
    let id = snap["id"].as_u64().unwrap();
    create(&mut s, "b");
    let version = s.document.version;
    s.run_named("history.snapshot.restore", json!({"id": id})).unwrap();
    assert_eq!(names(&s), vec!["a"]);
    assert!(s.document.version > version);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(names(&s), vec!["a", "b"]);
    s.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(names(&s), vec!["a"]);
    let list = s.run_named("history.snapshot.list", json!({})).unwrap();
    assert_eq!(list["snapshots"][0]["name"], "keep");
    assert!(s.run_named("history.snapshot.restore", json!({"id": 99})).is_err());
}

#[test]
fn snapshot_default_name_and_long_name_rejected() {
    let mut s = session();
    let snap = s.run_named("history.snapshot.create", json!({})).unwrap();
    assert_eq!(snap["name"], "Snapshot 1");
    assert!(s.run_named("history.snapshot.create", json!({"name": "x".repeat(300)})).is_err());
}

#[test]
fn budget_set_validates_and_trims_undo() {
    let mut s = session();
    for n in ["a", "b", "c", "d"] { create(&mut s, n); }
    assert!(s.run_named("history.budget.set", json!({"max_steps": 0})).is_err());
    assert!(s.run_named("history.budget.set", json!({"max_steps": 1_000_000})).is_err());
    assert!(s.run_named("history.budget.set", json!({"max_steps": 2, "extra": true})).is_err());
    assert_eq!(s.undo.applied_steps(), 4);
    let out = s.run_named("history.budget.set", json!({"max_steps": 2})).unwrap();
    assert_eq!(out["max_steps"], json!(2));
    assert_eq!(out["reachable_depth"], json!(2));
    assert_eq!(s.undo.applied_steps(), 2);
    let q = s.run_named("history.budget.query", json!({})).unwrap();
    assert!(q["memory_estimate_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn history_undo_redo_route_to_edit_and_noop_when_empty() {
    let mut s = session();
    let out = s.run_named("history.undo", json!({}));
    assert!(out.is_err() || out.unwrap()["changes"] == json!([]));
    create(&mut s, "a");
    s.run_named("history.undo", json!({})).unwrap();
    assert!(names(&s).is_empty());
    s.run_named("history.redo", json!({})).unwrap();
    assert_eq!(names(&s), vec!["a"]);
}

#[test]
fn save_with_doc_flag_round_trips() {
    let mut s = session();
    assert_eq!(s.run_named("history.save_with_doc.query", json!({})).unwrap()["enabled"], json!(false));
    s.run_named("history.save_with_doc.set", json!({"enabled": true})).unwrap();
    assert_eq!(s.run_named("history.save_with_doc.query", json!({})).unwrap()["enabled"], json!(true));
    assert!(s.run_named("history.save_with_doc.set", json!({})).is_err());
}

#[test]
fn recover_rejects_hostile_lengths_without_huge_alloc() {
    let file = TempFile::new();
    let mut bytes = b"DTOLHIST".to_vec();
    bytes.push(1);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(&file.0, &bytes).unwrap();
    let recovered = HistoryStore::recover(&file.0).unwrap();
    assert!(recovered.list().is_empty());
    assert_eq!(recovered.budget().reachable_depth, 0);
    assert!(!recovered.can_undo());
    std::fs::write(&file.0, b"NOTHIST").unwrap();
    assert!(HistoryStore::recover(&file.0).is_err());
}

#[test]
fn catalog_lists_history_commands() {
    let s = session();
    for n in ["history.list", "history.jump", "history.snapshot.create", "history.snapshot.restore", "history.snapshot.list", "history.budget.query", "history.budget.set", "history.save_with_doc.set", "history.save_with_doc.query", "history.labels.query", "history.undo", "history.redo"] {
        assert!(s.catalog.contains(n), "{n}");
    }
}
