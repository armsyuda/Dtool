//! edit.batch carries effect.add + effect.update (effects dialog OK) as one undo group on the live document.
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

fn effects(server: &Server, scope: u64, id: u64) -> Vec<Value> {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let object = ws.get(ContextId(scope)).unwrap().session.document.objects.iter().find(|o| o.id.0 == id).unwrap().clone();
    object.effects.iter().map(|e| serde_json::to_value(e).unwrap()).collect()
}

fn last_id(server: &Server, scope: u64) -> u64 {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.objects.last().unwrap().id.0
}

#[test]
fn c08_batch_effect_add_then_update_is_one_undo_step() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a").0;
    let mut s = Server::new(Config::default(), workspace);
    ok(run(&mut s, a, "object.create", json!({ "name": "box", "pos": [0.0, 0.0], "size": [40.0, 40.0] })));
    let id = last_id(&s, a);
    let batch = json!({ "commands": [
        { "name": "effect.add", "params": { "id": id, "kind": "OuterShadow" } },
        { "name": "effect.update", "params": { "id": id, "index": 0, "opacity": 0.5 } },
        { "name": "effect.add", "params": { "id": id, "kind": "OuterGlow" } }
    ] });
    let reply = ok(run(&mut s, a, "edit.batch", batch));
    assert!(reply.to_string().contains("effect.update"), "{reply}");
    let after = effects(&s, a, id);
    assert_eq!(after.len(), 2, "{after:?}");
    assert_eq!(after[0]["opacity"], json!(0.5), "{after:?}");

    ok(run(&mut s, a, "edit.undo", json!({})));
    assert!(effects(&s, a, id).is_empty(), "one undo removes the whole dialog OK");
    ok(run(&mut s, a, "edit.redo", json!({})));
    assert_eq!(effects(&s, a, id), after);
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert!(effects(&s, a, id).is_empty());

    // A bad index in a later step rolls the whole batch back: nothing applied, redo stack kept.
    let bad = json!({ "commands": [
        { "name": "effect.add", "params": { "id": id, "kind": "OuterShadow" } },
        { "name": "effect.update", "params": { "id": id, "index": 5, "opacity": 0.5 } }
    ] });
    let reply = run(&mut s, a, "edit.batch", bad);
    assert!(reply.get("error").is_some(), "{reply}");
    assert!(effects(&s, a, id).is_empty());
    ok(run(&mut s, a, "edit.redo", json!({})));
    assert_eq!(effects(&s, a, id), after, "redo stack untouched by the failed batch");
}