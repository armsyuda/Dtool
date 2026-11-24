use dtool_core::workspace::Workspace;
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn run_err(server: &mut Server, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }));
    reply.get("error").cloned().unwrap_or(reply)
}

fn undo(server: &mut Server) {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.undo", "params": {} }));
    assert!(reply.get("error").is_none(), "{reply}");
}

fn live() -> (Server, u64) {
    let mut workspace = Workspace::default();
    let doc = workspace.new_document("c11-styles");
    workspace.activate(doc).unwrap();
    let mut server = Server::new(Config::default(), workspace);
    let added = run(&mut server, "text.art.add", json!({ "pos": [20.0, 20.0], "text": "Hello styled world" }));
    let story = added["story"].as_u64().expect("story");
    (server, story)
}

#[test]
fn based_on_inheritance_and_cycle_rejection() {
    let (mut server, _) = live();
    run(&mut server, "text.style.define", json!({ "id": "Body", "kind": "paragraph", "props": { "space_after": 6.0, "align": "Left" } }));
    run(&mut server, "text.style.define", json!({ "id": "Quote", "kind": "paragraph", "based_on": "Body", "props": { "indent_left": 12.0 } }));
    run(&mut server, "text.style.define", json!({ "id": "Pull", "kind": "paragraph", "based_on": "Quote", "props": { "align": "Center" } }));
    let listed = run(&mut server, "text.styles.list", json!({ "kind": "paragraph" }));
    let pull = listed["styles"].as_array().unwrap().iter().find(|style| style["name"] == json!("Pull")).expect("pull");
    assert_eq!(pull["based_on"], json!("Quote"));
    let cycle = run_err(&mut server, "text.style.define", json!({ "id": "Body", "kind": "paragraph", "based_on": "Pull", "props": {} }));
    let code = cycle.get("code").or_else(|| cycle.get("data").and_then(|data| data.get("code"))).cloned();
    let message = cycle.to_string();
    assert!(message.contains("STYLE_CYCLE") || code == Some(json!("STYLE_CYCLE")), "{cycle}");
}

#[test]
fn apply_modes_and_clear_undo_on_the_live_document() {
    let (mut server, story) = live();
    run(&mut server, "text.style.define", json!({ "id": "Emph", "kind": "character", "props": { "italic": true, "tracking": 40.0 } }));
    run(&mut server, "text.style.apply_named", json!({ "story": story, "id": "Emph", "kind": "character", "range": { "start": 0, "end": 5 }, "keep_overrides": false }));
    let styled = run(&mut server, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_eq!(styled["italic"], json!(true), "{styled}");
    run(&mut server, "text.style", json!({ "story": story, "range": { "start": 0, "end": 5 }, "patch": { "tracking": 80.0 } }));
    run(&mut server, "text.style.clear_override", json!({ "story": story, "kind": "character", "range": { "start": 0, "end": 5 } }));
    let cleared = run(&mut server, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_eq!(cleared["tracking"], json!(40.0), "{cleared}");
    undo(&mut server);
    let after_clear_undo = run(&mut server, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_eq!(after_clear_undo["tracking"], json!(80.0), "{after_clear_undo}");
    undo(&mut server);
    undo(&mut server);
    let plain = run(&mut server, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_ne!(plain["italic"], json!(true), "{plain}");
}

#[test]
fn create_from_selection_stores_the_live_range() {
    let (mut server, story) = live();
    run(&mut server, "text.style", json!({ "story": story, "range": { "start": 0, "end": 5 }, "patch": { "size_pt": 22.0, "weight": 700 } }));
    run(&mut server, "text.style.define_from_selection", json!({ "story": story, "id": "Picked", "kind": "character", "range": { "start": 0, "end": 5 } }));
    let listed = run(&mut server, "text.styles.list", json!({ "kind": "character" }));
    let picked = listed["styles"].as_array().unwrap().iter().find(|style| style["name"] == json!("Picked")).expect("picked");
    assert_eq!(picked["props"]["size_pt"], json!(22.0), "{picked}");
    undo(&mut server);
    let after = run(&mut server, "text.styles.list", json!({ "kind": "character" }));
    assert!(after["styles"].as_array().unwrap().iter().all(|style| style["name"] != json!("Picked")), "{after}");
}

#[test]
fn panel_actions_call_the_same_commands() {
    let (mut server, story) = live();
    run(&mut server, "text.style.define", json!({ "id": "Emph", "kind": "character", "props": { "italic": true } }));
    run(&mut server, "text.style.apply_named", json!({ "story": story, "id": "Emph", "kind": "character", "range": { "start": 0, "end": 5 }, "keep_overrides": false }));
    run(&mut server, "text.style.clear_override", json!({ "story": story, "kind": "character", "range": { "start": 0, "end": 5 } }));
    run(&mut server, "text.style.remove", json!({ "id": "Emph", "kind": "character" }));
    let listed = run(&mut server, "text.styles.list", json!({}));
    assert!(listed["styles"].as_array().unwrap().iter().all(|style| style["name"] != json!("Emph")), "{listed}");
    undo(&mut server);
    let restored = run(&mut server, "text.styles.list", json!({}));
    assert!(restored["styles"].as_array().unwrap().iter().any(|style| style["name"] == json!("Emph")), "{restored}");
}
