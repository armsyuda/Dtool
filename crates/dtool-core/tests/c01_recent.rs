use dtool_core::workspace::{Scope, SharedServices, Workspace};
use dtool_protocol::Command;
use serde_json::json;
use std::sync::Arc;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("dtool_c01_recent_{tag}_{}_{}", std::process::id(), nanos));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn workspace() -> Workspace {
    Workspace::new(Arc::new(SharedServices::default()))
}

fn write_doc(path: &std::path::Path) {
    std::fs::write(path, serde_json::to_string(&dtool_core::document::Document::default()).unwrap()).unwrap();
}

fn run(workspace: &mut Workspace, name: &str, params: serde_json::Value) -> dtool_protocol::CommandResult {
    workspace.run(Scope::active(), Command::with_params(name, params)).1
}

fn listed(reply: &serde_json::Value, key: &str) -> Vec<String> {
    reply[key].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
}

#[test]
fn recent_list_is_the_workspace_list() {
    let dir = TempDir::new("list");
    let a = dir.0.join("a.dtool");
    write_doc(&a);
    let mut ws = workspace();
    ws.open_path(&a).unwrap();
    let reply = run(&mut ws, "doc.recent.list", json!({})).unwrap();
    assert_eq!(listed(&reply, "recent"), vec![a.display().to_string()]);
    assert!(listed(&reply, "missing").is_empty());
}

#[test]
fn open_recent_missing_returns_not_found_and_keeps_list() {
    let dir = TempDir::new("missing");
    let a = dir.0.join("a.dtool");
    write_doc(&a);
    let mut ws = workspace();
    let id = ws.open_path(&a).unwrap();
    ws.close_with(id, dtool_core::workspace::CloseAnswer::No).unwrap();
    std::fs::remove_file(&a).unwrap();
    let error = run(&mut ws, "doc.open_recent", json!({ "path": a })).unwrap_err();
    assert_eq!(error.code, "NOT_FOUND");
    assert!(ws.contexts().is_empty());
    let reply = run(&mut ws, "doc.recent.list", json!({})).unwrap();
    assert_eq!(listed(&reply, "recent"), vec![a.display().to_string()]);
    assert_eq!(listed(&reply, "missing"), vec![a.display().to_string()]);
}

#[test]
fn remove_recent_drops_entry_and_clear_empties() {
    let dir = TempDir::new("remove");
    let a = dir.0.join("a.dtool");
    let b = dir.0.join("b.dtool");
    write_doc(&a);
    write_doc(&b);
    let mut ws = workspace();
    ws.open_path(&a).unwrap();
    ws.open_path(&b).unwrap();
    let reply = run(&mut ws, "doc.recent.remove", json!({ "path": a })).unwrap();
    assert_eq!(reply["removed"], json!(true));
    assert_eq!(listed(&reply, "recent"), vec![b.display().to_string()]);
    let again = run(&mut ws, "doc.recent.remove", json!({ "path": a })).unwrap();
    assert_eq!(again["removed"], json!(false));
    assert_eq!(run(&mut ws, "doc.recent.remove", json!({})).unwrap_err().code, "INVALID_PARAMS");
    let cleared = run(&mut ws, "doc.recent.clear", json!({})).unwrap();
    assert!(listed(&cleared, "recent").is_empty());
    assert_eq!(ws.contexts().len(), 2);
}

#[test]
fn open_recent_opens_a_tab_and_keeps_current_document() {
    let dir = TempDir::new("tab");
    let a = dir.0.join("a.dtool");
    write_doc(&a);
    let mut ws = workspace();
    let first = ws.open_path(&a).unwrap();
    ws.close_with(first, dtool_core::workspace::CloseAnswer::No).unwrap();
    let current = ws.new_document("Current");
    run(&mut ws, "shape.rect", json!({ "x": 0.0, "y": 0.0, "w": 10.0, "h": 10.0 })).ok();
    let before = serde_json::to_string(ws.get(current).unwrap().session.document().unwrap()).unwrap();
    let reply = run(&mut ws, "doc.open_recent", json!({ "path": a })).unwrap();
    let opened = dtool_core::workspace::ContextId(reply["context"].as_u64().unwrap() as _);
    assert_ne!(opened, current);
    assert_eq!(ws.contexts().len(), 2);
    let after = serde_json::to_string(ws.get(current).unwrap().session.document().unwrap()).unwrap();
    assert_eq!(before, after);
}

#[test]
fn recent_commands_work_without_a_document() {
    let dir = TempDir::new("nodoc");
    let a = dir.0.join("a.dtool");
    let mut ws = workspace();
    ws.push_recent(&a);
    assert!(ws.contexts().is_empty());
    let reply = run(&mut ws, "doc.recent.list", json!({})).unwrap();
    assert_eq!(listed(&reply, "missing"), vec![a.display().to_string()]);
}

#[test]
fn session_scope_open_recent_never_replaces_current_document() {
    let dir = TempDir::new("session");
    let a = dir.0.join("a.dtool");
    write_doc(&a);
    let mut ws = workspace();
    let id = ws.new_document("Current");
    let session = &mut ws.get_mut(id).unwrap().session;
    let version = session.generation;
    let before = serde_json::to_string(session.document().unwrap()).unwrap();
    assert!(session.run(&Command::with_params("doc.open_recent", json!({ "path": a }))).is_err());
    assert_eq!(session.generation, version);
    assert_eq!(serde_json::to_string(session.document().unwrap()).unwrap(), before);
}