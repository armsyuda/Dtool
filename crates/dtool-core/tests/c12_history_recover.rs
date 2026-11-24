use dtool_core::autosave::{recovered_history, Autosave};
use dtool_core::document::{Document, Units};
use dtool_core::history_store::{sidecar_path, HistoryStore};
use dtool_core::prefs::{Prefs, CMD_PERFORMANCE_SET, CMD_SET};
use dtool_core::Session;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn session() -> Session { Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1)) }

fn create(session: &mut Session, name: &str) {
    session.run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
}

fn names(session: &Session) -> Vec<String> {
    session.document.objects.iter().map(|object| object.name.clone()).collect()
}

fn scratch(tag: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("c12_fix_{tag}_{}_{stamp}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn c12_sidecar_recover_restores_reachable_undo_and_redo_steps() {
    let mut source = session();
    create(&mut source, "first");
    create(&mut source, "second");
    create(&mut source, "third");
    source.run_named("edit.undo", json!({})).unwrap();
    let dir = scratch("sidecar");
    let document_path = dir.join("art.dtool");
    dtool_core::workspace::Workspace::write_document(&source.document, &document_path).unwrap();
    let document_bytes = std::fs::read(&document_path).unwrap();
    let saved = HistoryStore::from_undo(&source.undo);
    let sidecar = saved.save_sidecar(&document_path).unwrap();
    assert_eq!(sidecar, sidecar_path(&document_path));
    assert_ne!(sidecar, document_path);
    assert_eq!(std::fs::read(&document_path).unwrap(), document_bytes);

    let recovered = HistoryStore::recover_sidecar(&document_path).unwrap().unwrap();
    assert_eq!(recovered.list(), saved.list());
    assert_eq!(recovered.budget().reachable_depth, 2);
    assert!(recovered.can_undo());
    assert!(recovered.can_redo());

    let mut reopened = Session::new(dtool_core::commands::document_files::read_document(&document_path).unwrap());
    assert_eq!(names(&reopened), vec!["first", "second"]);
    assert_eq!(recovered.restore_into(&mut reopened.undo), 2);
    reopened.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(names(&reopened), vec!["first", "second", "third"]);
    reopened.run_named("edit.undo", json!({})).unwrap();
    reopened.run_named("edit.undo", json!({})).unwrap();
    reopened.run_named("edit.undo", json!({})).unwrap();
    assert!(names(&reopened).is_empty());
    assert!(!reopened.undo.can_undo());
    assert_eq!(std::fs::read(&document_path).unwrap(), document_bytes);
}

#[test]
fn c12_autosave_history_recovers_without_touching_original_path() {
    let mut source = session();
    create(&mut source, "kept-a");
    create(&mut source, "kept-b");
    let dir = scratch("autosave");
    let original = dir.join("original.dtool");
    std::fs::write(&original, b"ORIGINAL-BYTES").unwrap();
    let mut autosave = Autosave::with_dir(dir.join("recovery"));
    autosave.set(dtool_core::autosave::AutosaveSetArgs { enabled: Some(true), interval_s: Some(1) });
    let body = serde_json::to_string(&source.document).unwrap();
    let recovery = autosave.tick(1, Instant::now() + Duration::from_secs(5), "original", Some(&original), || body).unwrap().unwrap();
    let history = HistoryStore::from_undo(&source.undo);
    let sidecar = autosave.write_history(&history, &recovery, Some(&original)).unwrap();
    assert_ne!(sidecar, original);
    assert!(autosave.write_history(&history, &original, Some(&original)).is_err());
    assert_eq!(std::fs::read(&original).unwrap(), b"ORIGINAL-BYTES");

    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(recovery.with_extension("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["original_path"].as_str().map(PathBuf::from), Some(original.clone()));

    let restored = recovered_history(&recovery).unwrap().unwrap();
    assert_eq!(restored.budget().reachable_depth, 2);
    let mut recovered = Session::new(dtool_core::commands::document_files::read_document(&recovery).unwrap());
    restored.restore_into(&mut recovered.undo);
    recovered.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(names(&recovered), vec!["kept-a"]);
    assert_eq!(std::fs::read(&original).unwrap(), b"ORIGINAL-BYTES");
    let meta_after: serde_json::Value = serde_json::from_slice(&std::fs::read(recovery.with_extension("meta.json")).unwrap()).unwrap();
    assert_eq!(meta_after, meta);
}

#[test]
fn c12_corrupt_tail_keeps_only_verified_steps() {
    let mut source = session();
    create(&mut source, "one");
    create(&mut source, "two");
    let saved = HistoryStore::from_undo(&source.undo);
    let mut bytes = saved.encode().unwrap();
    bytes.truncate(bytes.len() - 2);
    let recovered = HistoryStore::decode(&bytes).unwrap();
    assert_eq!(recovered.list().len(), 1);
    assert_eq!(recovered.budget().reachable_depth, 1);
    let mut wrong = saved.encode().unwrap();
    let at = wrong.windows(b"history.object.create".len()).position(|w| w == b"history.object.create").unwrap();
    wrong[at] = b'H';
    let rejected = HistoryStore::decode(&wrong).unwrap();
    assert!(rejected.list().is_empty());
    assert!(!rejected.can_undo());
}

#[test]
fn c12_recent_count_caps_recent_list() {
    let dir = scratch("recent");
    let mut prefs = Prefs::default();
    prefs.run(CMD_SET, &json!({ "recent_count": 2 }), &dir.join("prefs.json")).unwrap();
    assert_eq!(dtool_core::commands::recent::cap(), 2);
    for name in ["a", "b", "c"] {
        dtool_core::commands::recent::push(&dir.join(format!("{name}.dtool")));
    }
    assert_eq!(dtool_core::commands::recent::list(), vec![dir.join("c.dtool"), dir.join("b.dtool")]);
    prefs.run(CMD_PERFORMANCE_SET, &json!({ "recent_count": 0 }), &dir.join("prefs.json")).unwrap();
    assert_eq!(dtool_core::commands::recent::cap(), 1);
    assert_eq!(dtool_core::commands::recent::list(), vec![dir.join("c.dtool")]);
    assert_eq!(dtool_core::commands::recent::set_cap(3), 3);
    for name in ["d", "e", "f", "g"] {
        dtool_core::commands::recent::push(&dir.join(format!("{name}.dtool")));
    }
    assert_eq!(dtool_core::commands::recent::list().len(), 3);
}

#[test]
fn c12_autosave_poll_writes_history_only_when_save_with_document() {
    for enabled in [false, true] {
        let mut live = session();
        live.has_document = true;
        live.history.save_with_document = enabled;
        let dir = scratch("poll");
        live.autosave = Autosave::with_dir(dir.join("recovery"));
        live.autosave.set(dtool_core::autosave::AutosaveSetArgs { enabled: Some(true), interval_s: Some(1) });
        create(&mut live, "polled");
        std::thread::sleep(Duration::from_millis(1100));
        let original = dir.join("doc.dtool");
        std::fs::write(&original, b"KEEP").unwrap();
        let target = dtool_core::command::autosave_commands::poll(&mut live, "doc", Some(&original)).unwrap().unwrap();
        let restored = recovered_history(&target).unwrap();
        assert_eq!(restored.is_some(), enabled);
        if let Some(restored) = restored {
            assert_eq!(restored.budget().reachable_depth, 1);
        }
        assert_eq!(std::fs::read(&original).unwrap(), b"KEEP");
    }
}
