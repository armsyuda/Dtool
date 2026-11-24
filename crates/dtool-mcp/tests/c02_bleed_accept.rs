use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn ok(reply: &Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply.clone()
}

fn state(server: &Server, scope: u64) -> (bool, u64, bool) {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let session = &ws.get(ContextId(scope)).unwrap().session;
    (session.view.canvas_flags.show_bleed, session.document.version, session.undo.can_undo())
}

#[test]
fn mcp_show_bleed_is_per_document_view_state_on_live_documents() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("bleed_a").0;
    let b = workspace.new_document("bleed_b").0;
    let mut s = Server::new(Config::default(), workspace);
    let (bleed_a, version_a, undo_a) = state(&s, a);
    let (bleed_b, version_b, undo_b) = state(&s, b);
    assert!(!bleed_a && !bleed_b);

    ok(&run(&mut s, a, "view.flags.set", json!({ "show_bleed": true })));
    assert_eq!(state(&s, a), (true, version_a, undo_a));
    assert_eq!(state(&s, b), (false, version_b, undo_b));
    let q = ok(&run(&mut s, a, "view.flags.query", json!({})));
    assert_eq!(q["result"]["flags"]["show_bleed"], json!(true), "{q}");
    let q = ok(&run(&mut s, b, "view.flags.query", json!({})));
    assert_eq!(q["result"]["flags"]["show_bleed"], json!(false), "{q}");

    ok(&run(&mut s, a, "view.flags.set", json!({ "clip_to_canvas": true })));
    assert!(state(&s, a).0);
    ok(&run(&mut s, a, "view.flags.set", json!({ "show_bleed": false })));
    assert_eq!(state(&s, a), (false, version_a, undo_a));
    assert!(run(&mut s, a, "view.flags.set", json!({ "show_bleed": "yes" })).get("error").is_some());
    assert!(!state(&s, a).0);
}