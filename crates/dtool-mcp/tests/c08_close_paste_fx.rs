use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn ok(reply: &Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply["result"].clone()
}

fn effects(server: &Server, scope: u64, index: usize) -> Value {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    serde_json::to_value(&ws.get(ContextId(scope)).unwrap().session.document.objects[index].effects).unwrap()
}

fn version(server: &Server, scope: u64) -> u64 {
    server.workspace().lock().unwrap().get(ContextId(scope)).unwrap().session.document.version
}

fn last_id(server: &Server, scope: u64) -> u64 {
    server.workspace().lock().unwrap().get(ContextId(scope)).unwrap().session.document.objects.last().unwrap().id.0
}

#[test]
fn mcp_copy_then_paste_fx_applies_effects_on_live_document_with_one_undo() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("paste_fx").0;
    let mut s = Server::new(Config::default(), workspace);
    ok(&run(&mut s, a, "object.create", json!({ "pos": [0.0, 0.0], "size": [10.0, 10.0] })));
    let source = last_id(&s, a);
    ok(&run(&mut s, a, "effect.add", json!({ "id": source, "kind": "OuterShadow" })));
    ok(&run(&mut s, a, "object.create", json!({ "pos": [20.0, 0.0], "size": [10.0, 10.0] })));
    let target = last_id(&s, a);
    assert_eq!(effects(&s, a, 1), json!([]));

    ok(&run(&mut s, a, "selection.set", json!({ "ids": [source] })));
    ok(&run(&mut s, a, "edit.copy", json!({})));
    ok(&run(&mut s, a, "selection.set", json!({ "ids": [target] })));
    let v = version(&s, a);
    let reply = ok(&run(&mut s, a, "edit.paste_fx", json!({})));
    assert_eq!(reply["changes"][0]["name"], json!("edit.paste_fx"), "{reply}");
    assert_eq!(effects(&s, a, 1), effects(&s, a, 0));
    assert_ne!(effects(&s, a, 1), json!([]));
    assert_eq!(version(&s, a), v + 1);

    let again = ok(&run(&mut s, a, "edit.paste_fx", json!({})));
    assert_eq!(again["changes"], json!([]), "{again}");
    assert_eq!(version(&s, a), v + 1);

    ok(&run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(effects(&s, a, 1), json!([]));
    assert_ne!(effects(&s, a, 0), json!([]));
    ok(&run(&mut s, a, "edit.redo", json!({})));
    assert_eq!(effects(&s, a, 1), effects(&s, a, 0));
}

#[test]
fn mcp_paste_fx_without_effects_or_selection_is_rejected_without_change() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("paste_fx_bad").0;
    let mut s = Server::new(Config::default(), workspace);
    ok(&run(&mut s, a, "object.create", json!({ "pos": [0.0, 0.0], "size": [10.0, 10.0] })));
    let id = last_id(&s, a);
    let v = version(&s, a);
    assert!(run(&mut s, a, "edit.paste_fx", json!({})).get("error").is_some());
    ok(&run(&mut s, a, "selection.set", json!({ "ids": [id] })));
    assert!(run(&mut s, a, "edit.paste_fx", json!({})).get("error").is_some());
    assert_eq!(version(&s, a), v);
    assert_eq!(effects(&s, a, 0), json!([]));
}