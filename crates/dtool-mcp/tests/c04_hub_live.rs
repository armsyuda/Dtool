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

fn setup() -> (Server, u64, u64) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a").0;
    let b = workspace.new_document("b").0;
    workspace.activate(ContextId(b)).unwrap();
    (Server::new(Config::default(), workspace), a, b)
}

#[test]
fn mcp_power_duplicate_repeats_rotate_and_move_on_the_scoped_document_only() {
    let (mut s, a, b) = setup();
    ok(run(&mut s, a, "object.create", json!({ "name": "box", "pos": [0.0, 0.0], "size": [10.0, 10.0] })));
    let source = doc(&s, a)["objects"][0]["id"].as_u64().unwrap();
    ok(run(&mut s, a, "selection.set", json!({ "ids": [source] })));
    ok(run(&mut s, a, "edit.duplicate", json!({})));
    let b_before = doc(&s, b);
    ok(run(&mut s, a, "transform.selection_box", json!({ "origin": [5.0, 5.0], "translate": [20.0, 0.0], "rotation_deg": 15.0 })));
    ok(run(&mut s, a, "edit.power_duplicate", json!({})));
    let objects = doc(&s, a)["objects"].as_array().unwrap().clone();
    assert_eq!(objects.len(), 3);
    let last = objects.last().unwrap();
    assert_eq!(last["rotation_deg"].as_f64().unwrap() as f32, 30.0);
    assert!((last["pos"][0].as_f64().unwrap() - 40.0).abs() < 1e-3, "{last}");
    assert_eq!(doc(&s, b), b_before);
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(doc(&s, a)["objects"].as_array().unwrap().len(), 2);
}

#[test]
fn mcp_text_to_path_makes_real_outlines_and_undo_restores_text() {
    let (mut s, a, _) = setup();
    ok(run(&mut s, a, "text.art.add", json!({ "text": "Hi", "pos": [10.0, 20.0], "size_pt": 40.0 })));
    let id = doc(&s, a)["objects"][0]["id"].as_u64().unwrap();
    let before = doc(&s, a);
    ok(run(&mut s, a, "text.to_path", json!({ "id": id })));
    let converted = doc(&s, a)["objects"].as_array().unwrap().iter().find(|o| o["id"].as_u64() == Some(id)).unwrap().clone();
    assert!(converted["kind"].get("Curve").is_some(), "{}", converted["kind"]);
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(doc(&s, a)["objects"], before["objects"]);
    assert_eq!(doc(&s, a)["stories"], before["stories"]);
}

#[test]
fn mcp_paste_without_format_makes_art_text_with_one_undo() {
    let (mut s, a, _) = setup();
    s.workspace().lock().unwrap().get_mut(ContextId(a)).unwrap().session.clipboard.set_text("plain".into());
    ok(run(&mut s, a, "edit.paste_without_format", json!({})));
    let objects = doc(&s, a)["objects"].as_array().unwrap().clone();
    assert_eq!(objects.len(), 1);
    let object = objects[0].clone();
    assert!(object["kind"].get("ArtText").is_some(), "{}", object["kind"]);
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert!(doc(&s, a)["objects"].as_array().unwrap().is_empty());
}