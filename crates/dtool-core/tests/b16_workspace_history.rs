use dtool_core::history_store::HistoryStore;
use dtool_core::workspace::{CloseAnswer, ContextId, Scope, SharedServices, Workspace};
use dtool_protocol::Command;
use serde_json::{Value, json};
use std::sync::Arc;

fn run(workspace: &mut Workspace, scope: Scope, name: &str, params: Value) -> Value {
    workspace.run(scope, Command::with_params(name, params)).1.unwrap()
}

fn create(workspace: &mut Workspace, id: ContextId, name: &str) {
    run(workspace, Scope::of(id), "object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]}));
}

fn labels(workspace: &Workspace, id: ContextId) -> Vec<String> {
    HistoryStore::from_undo(&workspace.get(id).unwrap().session.undo).list().iter().map(|entry| entry.label_key.clone()).collect()
}

#[test]
fn two_open_document_contexts_do_not_share_history_cursor() {
    let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
    let first = workspace.new_document("first");
    let second = workspace.new_document("second");
    create(&mut workspace, first, "one");
    create(&mut workspace, first, "two");
    create(&mut workspace, second, "other");
    run(&mut workspace, Scope::of(first), "edit.undo", json!({}));
    assert_eq!(workspace.get(first).unwrap().session.undo.applied_steps(), 1);
    assert_eq!(workspace.get(second).unwrap().session.undo.applied_steps(), 1);
    assert!(workspace.get(first).unwrap().session.undo.can_redo());
    assert!(!workspace.get(second).unwrap().session.undo.can_redo());
    assert_eq!(workspace.get(second).unwrap().session.document.objects[0].name, "other");
    run(&mut workspace, Scope::of(first), "edit.undo", json!({}));
    assert_eq!(workspace.get(first).unwrap().session.undo.applied_steps(), 0);
    assert_eq!(workspace.get(second).unwrap().session.undo.applied_steps(), 1);
}

#[test]
fn switching_active_context_lists_only_that_context_history() {
    let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
    let first = workspace.new_document("first");
    let second = workspace.new_document("second");
    create(&mut workspace, first, "one");
    create(&mut workspace, second, "two");
    create(&mut workspace, second, "three");
    workspace.activate(first).unwrap();
    assert_eq!(labels(&workspace, workspace.active().unwrap()), vec!["history.object.create"]);
    workspace.activate(second).unwrap();
    assert_eq!(labels(&workspace, workspace.active().unwrap()), vec!["history.object.create", "history.object.create"]);
    run(&mut workspace, Scope::active(), "edit.undo", json!({}));
    assert_eq!(workspace.get(first).unwrap().session.document.objects.len(), 1);
    assert_eq!(workspace.get(second).unwrap().session.document.objects.len(), 1);
}

#[test]
fn close_and_reopen_without_save_with_doc_recovers_no_steps() {
    let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
    let id = workspace.new_document("roundtrip");
    create(&mut workspace, id, "persisted");
    assert!(!HistoryStore::from_undo(&workspace.get(id).unwrap().session.undo).save_history_with_document());
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("b16_workspace_history_{}_{}.json", std::process::id(), nonce));
    std::fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        workspace.save_as(id, &path).unwrap();
        assert!(workspace.close_with(id, CloseAnswer::No).unwrap());
        assert!(workspace.get(id).is_none());
        let reopened = workspace.open_path(&path).unwrap();
        assert_ne!(reopened, id);
        let session = &workspace.get(reopened).unwrap().session;
        assert_eq!(session.document.objects[0].name, "persisted");
        assert_eq!(session.undo.applied_steps(), 0);
        assert!(!session.undo.can_undo());
        assert!(!session.undo.can_redo());
        assert!(HistoryStore::from_undo(&session.undo).list().is_empty());
        assert!(workspace.close_with(reopened, CloseAnswer::No).unwrap());
    }));
    std::fs::remove_file(&path).unwrap();
    if let Err(error) = result { std::panic::resume_unwind(error); }
}
