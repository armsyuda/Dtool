use dtool_core::workspace::Workspace;
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn live() -> (Server, u64) {
    let mut workspace = Workspace::default();
    let doc = workspace.new_document("c11-style-gaps");
    workspace.activate(doc).unwrap();
    let mut server = Server::new(Config::default(), workspace);
    let added = run(&mut server, "text.art.add", json!({ "pos": [20.0, 20.0], "text": "Hello styled world" }));
    let story = added["story"].as_u64().expect("story");
    (server, story)
}

#[test]
fn detach_set_no_style_delete_unused_via_mcp() {
    let (mut server, story) = live();
    run(&mut server, "text.style.define", json!({ "id": "Emph", "kind": "character", "props": { "italic": true } }));
    run(&mut server, "text.style.define", json!({ "id": "Spare", "kind": "character", "props": { "tracking": 20.0 } }));
    run(&mut server, "text.style.apply_named", json!({ "story": story, "id": "Emph", "kind": "character", "range": { "start": 0, "end": 5 } }));
    run(&mut server, "text.style.detach", json!({ "id": "Emph", "kind": "character" }));
    run(&mut server, "edit.undo", json!({}));
    run(&mut server, "text.style.set_no_style", json!({ "story": story, "kind": "character", "range": { "start": 0, "end": 5 } }));
    run(&mut server, "edit.undo", json!({}));
    let removed = run(&mut server, "text.style.delete_unused", json!({}));
    assert!(removed["removed"].as_array().unwrap().iter().any(|item| item["name"] == json!("Spare")), "{removed}");
}
