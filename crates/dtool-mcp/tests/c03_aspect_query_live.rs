use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn object_ids(server: &Server, scope: u64) -> Vec<u64> {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.objects.iter().map(|o| o.id.0).collect()
}

fn size(server: &Server, scope: u64, id: u64) -> [f32; 2] {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.get_object(dtool_core::object::ObjectId(id)).unwrap().size
}

#[test]
fn c03_transform_query_reports_last_aspect_lock_on_live_document() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a").0;
    let b = workspace.new_document("b").0;
    let mut s = Server::new(Config::default(), workspace);
    run(&mut s, a, "object.create", json!({ "name": "one", "pos": [0.0, 0.0], "size": [20.0, 10.0] }));
    run(&mut s, a, "object.create", json!({ "name": "two", "pos": [50.0, 0.0], "size": [30.0, 10.0] }));
    let ids = object_ids(&s, a);
    run(&mut s, a, "selection.set", json!({ "ids": ids }));
    let q = run(&mut s, a, "transform.query", json!({}));
    assert_eq!(q["mixed"]["w"], true, "{q}");
    assert_eq!(q["size"][0].as_f64().unwrap() as f32, 20.0, "first object's size, not an average: {q}");
    assert_eq!(q["aspect_locked"], false);

    run(&mut s, a, "selection.set", json!({ "ids": [ids[0]] }));
    run(&mut s, a, "transform.set", json!({ "w": 40.0, "aspect_locked": true }));
    assert_eq!(size(&s, a, ids[0]), [40.0, 20.0]);
    assert_eq!(run(&mut s, a, "transform.query", json!({}))["aspect_locked"], true);

    run(&mut s, a, "transform.set", json!({ "x": 5.0 }));
    assert_eq!(run(&mut s, a, "transform.query", json!({}))["aspect_locked"], true, "x-only set keeps the remembered lock");
    assert_eq!(run(&mut s, b, "transform.query", json!({}))["aspect_locked"], false, "lock is per document session");

    run(&mut s, a, "edit.undo", json!({}));
    assert_eq!(run(&mut s, a, "transform.query", json!({}))["aspect_locked"], true, "lock is not undo state");
    run(&mut s, a, "transform.set", json!({ "w": 10.0, "aspect_locked": false }));
    assert_eq!(run(&mut s, a, "transform.query", json!({}))["aspect_locked"], false);
}