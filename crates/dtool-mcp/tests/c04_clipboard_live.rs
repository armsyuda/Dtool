use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn ok(reply: Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply
}

fn doc(server: &Server, scope: u64) -> Value {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let mut value = serde_json::to_value(&ws.get(ContextId(scope)).unwrap().session.document).unwrap();
    value.as_object_mut().unwrap().remove("version");
    value
}

fn names(server: &Server, scope: u64) -> Vec<String> {
    doc(server, scope)["objects"].as_array().unwrap().iter().map(|o| o["name"].as_str().unwrap_or("").to_string()).collect()
}

#[test]
fn mcp_copy_in_a_paste_in_b_edits_and_undoes_only_b() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a").0;
    let b = workspace.new_document("b").0;
    workspace.activate(ContextId(b)).unwrap();
    let mut s = Server::new(Config::default(), workspace);
    ok(run(&mut s, a, "object.create", json!({ "name": "one", "pos": [0.0, 0.0], "size": [10.0, 10.0] })));
    ok(run(&mut s, a, "object.create", json!({ "name": "two", "pos": [20.0, 0.0], "size": [10.0, 10.0] })));
    let ids: Vec<u64> = doc(&s, a)["objects"].as_array().unwrap().iter().map(|o| o["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    ok(run(&mut s, a, "selection.set", json!({ "ids": ids })));
    ok(run(&mut s, a, "edit.copy", json!({})));
    let a_before = doc(&s, a);
    let b_before = doc(&s, b);
    ok(run(&mut s, b, "edit.paste", json!({})));
    let mut pasted = names(&s, b);
    pasted.sort();
    assert_eq!(pasted, vec!["one".to_string(), "two".to_string()]);
    assert_eq!(doc(&s, a), a_before);
    let b_after = doc(&s, b);
    ok(run(&mut s, b, "edit.undo", json!({})));
    assert_eq!(doc(&s, b)["objects"], b_before["objects"]);
    assert_eq!(doc(&s, a), a_before);
    ok(run(&mut s, b, "edit.redo", json!({})));
    assert_eq!(doc(&s, b)["objects"], b_after["objects"]);
    assert_eq!(doc(&s, a), a_before);
}

#[test]
fn mcp_cut_two_objects_is_one_undo_with_redo() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a").0;
    let mut s = Server::new(Config::default(), workspace);
    ok(run(&mut s, a, "object.create", json!({ "name": "one", "pos": [0.0, 0.0], "size": [10.0, 10.0] })));
    ok(run(&mut s, a, "object.create", json!({ "name": "two", "pos": [20.0, 0.0], "size": [10.0, 10.0] })));
    let ids: Vec<u64> = doc(&s, a)["objects"].as_array().unwrap().iter().map(|o| o["id"].as_u64().unwrap()).collect();
    ok(run(&mut s, a, "selection.set", json!({ "ids": ids })));
    let before = doc(&s, a)["objects"].clone();
    ok(run(&mut s, a, "edit.cut", json!({})));
    assert!(doc(&s, a)["objects"].as_array().unwrap().is_empty());
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(doc(&s, a)["objects"], before);
    ok(run(&mut s, a, "edit.redo", json!({})));
    assert!(doc(&s, a)["objects"].as_array().unwrap().is_empty());
}