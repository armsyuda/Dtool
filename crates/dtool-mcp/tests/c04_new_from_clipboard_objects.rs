use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn call(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    let reply = call(server, scope, name, params);
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

#[test]
fn c04_new_document_from_copied_objects_over_mcp() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a").0;
    let mut s = Server::new(Config::default(), workspace);
    run(&mut s, a, "object.create", json!({ "name": "one", "pos": [100.0, 50.0], "size": [20.0, 10.0] }));
    run(&mut s, a, "object.create", json!({ "name": "two", "pos": [130.0, 70.0], "size": [30.0, 40.0] }));
    let ids: Vec<u64> = {
        let ws = s.workspace();
        let ws = ws.lock().unwrap();
        ws.get(ContextId(a)).unwrap().session.document.objects.iter().map(|o| o.id.0).collect()
    };
    run(&mut s, a, "selection.set", json!({ "ids": ids }));
    run(&mut s, a, "edit.copy", json!({}));
    let source_before = {
        let ws = s.workspace();
        let ws = ws.lock().unwrap();
        serde_json::to_value(&ws.get(ContextId(a)).unwrap().session.document.objects).unwrap()
    };

    let reply = run(&mut s, a, "doc.new_from_clipboard", json!({}));
    let new_id = reply["context"].as_u64().expect("context id");
    assert_ne!(new_id, a);
    let ws = s.workspace();
    let ws = ws.lock().unwrap();
    let ctx = ws.get(ContextId(new_id)).unwrap();
    let doc = &ctx.session.document;
    assert_eq!(doc.pages[0].size, [60.0, 60.0], "page = copied bounds");
    let mut placed: Vec<([f32; 2], [f32; 2], String)> = doc.objects.iter().map(|o| (o.pos, o.size, o.name.clone())).collect();
    placed.sort_by(|x, y| x.2.cmp(&y.2));
    assert_eq!(placed, vec![([0.0, 0.0], [20.0, 10.0], "one".to_string()), ([30.0, 20.0], [30.0, 40.0], "two".to_string())]);
    assert_eq!(ctx.session.selection.ids.len(), 2);
    assert!(!ctx.session.undo.can_undo(), "new document starts with empty history");
    assert_eq!(serde_json::to_value(&ws.get(ContextId(a)).unwrap().session.document.objects).unwrap(), source_before);
    assert_eq!(ws.active(), Some(ContextId(new_id)));
}

#[test]
fn c04_new_document_from_empty_clipboard_is_invalid_and_opens_no_tab() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a").0;
    let count = workspace.contexts().len();
    let mut s = Server::new(Config::default(), workspace);
    let reply = call(&mut s, a, "doc.new_from_clipboard", json!({}));
    assert_eq!(reply["error"]["code"], "INVALID_PARAMS", "{reply}");
    assert_eq!(s.workspace().lock().unwrap().contexts().len(), count);
}