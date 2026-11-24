use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use serde_json::json;

fn run(ws: &mut Workspace, name: &str, params: serde_json::Value) -> serde_json::Value {
    ws.run(Scope::active(), Command { name: name.into(), params }).1.unwrap()
}

#[test]
fn k03_mutating_replies_carry_doc_version_changes_and_context() {
    let mut ws = Workspace::default();
    let id = ws.new_document("k03");
    let created = run(&mut ws, "object.create", json!({"pos": [0.0, 0.0], "size": [4.0, 4.0]}));
    let generation = ws.get(id).unwrap().session.generation;
    assert_eq!(created["doc_version"], json!(generation));
    assert_eq!(created["context_id"], json!(id.0));
    assert!(created["changes"].as_array().is_some_and(|changes| !changes.is_empty()));
    let undone = run(&mut ws, "edit.undo", json!({}));
    assert_eq!(undone["doc_version"], json!(ws.get(id).unwrap().session.generation), "{undone}");
    assert!(undone["changes"].is_array(), "{undone}");
}

#[test]
fn k03_read_only_queries_stay_unchanged() {
    let mut ws = Workspace::default();
    ws.new_document("k03q");
    let listed = run(&mut ws, "commands.list", json!({}));
    assert!(listed.get("doc_version").is_none(), "{listed}");
}
