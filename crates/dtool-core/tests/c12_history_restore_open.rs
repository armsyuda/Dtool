use dtool_core::autosave::Autosave;
use dtool_core::document::{Document, Units};
use dtool_core::history_store::{sidecar_path, HistoryStore};
use dtool_core::workspace::Workspace;
use dtool_core::{Command, Scope, Session};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn session() -> Session { Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1)) }

fn create(session: &mut Session, name: &str) {
    session.run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
}

fn scratch(tag: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("c12b_{tag}_{}_{stamp}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn saved_document(tag: &str, with_history: bool) -> (PathBuf, Vec<u8>) {
    let mut source = session();
    create(&mut source, "first");
    create(&mut source, "second");
    let path = scratch(tag).join("doc.dtool");
    Workspace::write_document(&source.document, &path).unwrap();
    if with_history {
        HistoryStore::from_undo(&source.undo).save_sidecar(&path).unwrap();
    }
    let bytes = std::fs::read(&path).unwrap();
    (path, bytes)
}

fn names(workspace: &Workspace) -> Vec<String> {
    workspace.session().unwrap().document.objects.iter().map(|object| object.name.clone()).collect()
}

#[test]
fn c12b_doc_open_restores_history_when_sidecar_present() {
    let (path, bytes) = saved_document("open", true);
    let mut workspace = Workspace::default();
    let (id, reply) = workspace.run(Scope::active(), Command::with_params("doc.open", json!({ "path": path })));
    let reply = reply.unwrap();
    assert_eq!(reply["history"]["restored"], 2, "{reply}");
    let id = id.unwrap();
    assert!(workspace.get(id).unwrap().session.history.save_with_document);
    assert_eq!(names(&workspace), vec!["first", "second"]);
    workspace.run(Scope::of(id), Command::new("edit.undo")).1.unwrap();
    assert_eq!(names(&workspace), vec!["first"]);
    workspace.run(Scope::of(id), Command::new("edit.undo")).1.unwrap();
    assert!(names(&workspace).is_empty());
    assert!(!workspace.get(id).unwrap().session.undo.can_undo());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn c12b_doc_open_without_or_with_bad_sidecar_still_opens() {
    let (path, _) = saved_document("plain", false);
    let mut workspace = Workspace::default();
    let (id, reply) = workspace.run(Scope::active(), Command::with_params("doc.open", json!({ "path": path })));
    assert_eq!(reply.unwrap()["history"]["restored"], 0);
    assert!(!workspace.get(id.unwrap()).unwrap().session.undo.can_undo());

    let (bad, _) = saved_document("bad", false);
    std::fs::write(sidecar_path(&bad), b"NOT A HISTORY FILE").unwrap();
    let (id, reply) = workspace.run(Scope::active(), Command::with_params("doc.open", json!({ "path": bad })));
    let reply = reply.unwrap();
    assert_eq!(reply["history"]["restored"], 0);
    assert!(reply["history"]["error"].is_string(), "{reply}");
    assert!(!workspace.get(id.unwrap()).unwrap().session.undo.can_undo());
}

#[test]
fn c12b_workspace_open_recent_opens_a_tab_and_restores_history() {
    let (path, _) = saved_document("recent", true);
    let mut workspace = Workspace::default();
    let current = workspace.new_document("Current");
    workspace.run(Scope::of(current), Command::with_params("object.create", json!({"name": "unrelated", "pos": [0.0, 0.0], "size": [10.0, 10.0]}))).1.unwrap();
    let (id, reply) = workspace.run(Scope::active(), Command::with_params("doc.open_recent", json!({ "path": path })));
    reply.unwrap();
    let id = id.expect("open_recent opens a context");
    assert_ne!(id, current);
    assert_eq!(workspace.active(), Some(id));
    assert_eq!(workspace.get(current).unwrap().session.document.objects.len(), 1);
    assert_eq!(workspace.get(current).unwrap().session.document.objects[0].name, "unrelated");
    let opened = &workspace.get(id).unwrap().session;
    assert_eq!(opened.undo.applied_steps(), 2);
    assert!(opened.history.save_with_document);
    workspace.run(Scope::of(id), Command::new("edit.undo")).1.unwrap();
    assert_eq!(workspace.get(id).unwrap().session.document.objects.len(), 1);
    let (again, reply) = workspace.run(Scope::active(), Command::with_params("doc.open_recent", json!({ "path": path })));
    reply.unwrap();
    assert_eq!(again, Some(id), "open_recent focuses the existing tab");
    assert_eq!(workspace.contexts().len(), 2);
}

#[test]
fn c12b_doc_recover_restores_autosaved_history_without_touching_original() {
    let profile = scratch("profile");
    unsafe { std::env::set_var("APPDATA", &profile) };
    let original = profile.join("original.dtool");
    std::fs::write(&original, b"ORIGINAL").unwrap();
    let mut source = session();
    create(&mut source, "kept-a");
    create(&mut source, "kept-b");
    let mut autosave = Autosave::new();
    assert!(autosave.dir.starts_with(&profile));
    autosave.set(dtool_core::autosave::AutosaveSetArgs { enabled: Some(true), interval_s: Some(1) });
    let body = serde_json::to_string(&source.document).unwrap();
    let recovery = autosave.tick(1, Instant::now() + Duration::from_secs(5), "original", Some(&original), || body).unwrap().unwrap();
    autosave.write_history(&HistoryStore::from_undo(&source.undo), &recovery, Some(&original)).unwrap();

    let mut workspace = Workspace::default();
    let (id, reply) = workspace.run(Scope::active(), Command::with_params("doc.recover", json!({ "path": recovery })));
    let reply = reply.unwrap();
    assert_eq!(reply["history"]["restored"], 2, "{reply}");
    assert_eq!(reply["path"], serde_json::Value::Null);
    let id = id.unwrap();
    workspace.run(Scope::of(id), Command::new("edit.undo")).1.unwrap();
    assert_eq!(names(&workspace), vec!["kept-a"]);
    assert_eq!(std::fs::read(&original).unwrap(), b"ORIGINAL");
}
