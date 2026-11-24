use dtool_core::workspace::Workspace;
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn create(scope: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": "object.create", "scope": scope, "params": { "pos": [0.0, 0.0], "size": [4.0, 4.0] } } })
}

fn setup() -> (Server, u64, u64) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    workspace.activate(b).unwrap();
    (Server::new(Config::default(), workspace), a.0, b.0)
}

fn count(server: &Server, id: u64) -> usize {
    server.workspace().lock().unwrap().get(dtool_core::workspace::ContextId(id)).map(|c| c.session.document.objects.len()).unwrap_or(usize::MAX)
}

#[test]
fn scope_targets_named_context() {
    let (mut server, a, b) = setup();
    for scope in [json!(a), json!(a.to_string()), json!({ "context": a })] {
        let before = (count(&server, a), count(&server, b));
        let reply = server.handle_request(create(scope));
        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!(reply["context_id"], json!(a));
        assert_eq!((count(&server, a), count(&server, b)), (before.0 + 1, before.1));
    }
}

#[test]
fn closed_scope_is_context_not_found() {
    let (mut server, a, b) = setup();
    server.workspace().lock().unwrap().close_others(dtool_core::workspace::ContextId(b));
    let before = count(&server, b);
    let reply = server.handle_request(create(json!(a)));
    assert_eq!(reply["error"]["code"], "CONTEXT_NOT_FOUND", "{reply}");
    assert_eq!(count(&server, b), before);
}

#[test]
fn bad_scope_type_is_invalid_params() {
    let (mut server, _, _) = setup();
    for scope in [json!(true), json!("nope"), json!([1]), json!({ "context": "x" })] {
        let reply = server.handle_request(create(scope));
        assert_eq!(reply["error"]["code"], "INVALID_PARAMS", "{reply}");
    }
}

#[test]
fn missing_scope_uses_active() {
    let (mut server, a, b) = setup();
    for scope in [Value::Null, json!("active")] {
        let before = (count(&server, a), count(&server, b));
        let reply = server.handle_request(create(scope));
        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!((count(&server, a), count(&server, b)), (before.0, before.1 + 1));
    }
    let undo = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.undo", "params": { "scope": b } }));
    assert_eq!(undo["context_id"], json!(b), "{undo}");
}
