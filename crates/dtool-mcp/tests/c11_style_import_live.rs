#[path = "../../dtool-core/tests/support/test_home.rs"]
mod test_home;

use dtool_core::text_styles::StyleKind;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn call(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    let mut reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params.clone() } }));
    if reply["error"]["code"] == "NEEDS_CONFIRMATION" {
        let id = reply["error"]["confirm_id"].as_u64().unwrap();
        dtool_mcp::permission::confirm_store().approve(id).unwrap();
        let status = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "mcp.confirm.status", "params": { "confirm_id": id } }));
        let mut params = params;
        params["confirm_token"] = status["result"]["confirm_token"].clone();
        reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 3, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }));
    }
    reply
}

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    let reply = call(server, scope, name, params);
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn style(server: &Server, scope: u64, kind: StyleKind, name: &str) -> Option<(Option<String>, Value)> {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let store = &ws.get(ContextId(scope)).unwrap().session.document.text_styles;
    store.get(kind, name).map(|s| (s.based_on.clone(), Value::Object(s.props.clone())))
}

fn first_run_size(server: &Server, scope: u64, story: u64) -> Option<f64> {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let doc = &ws.get(ContextId(scope)).unwrap().session.document;
    doc.stories.iter().find(|s| s.id.0 == story).and_then(|s| s.ranges.first()).and_then(|r| r["size_pt"].as_f64())
}

fn art(server: &mut Server, scope: u64, text: &str) -> u64 {
    run(server, scope, "text.art.add", json!({ "pos": [0.0, 0.0], "text": text }));
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let doc = &ws.get(ContextId(scope)).unwrap().session.document;
    doc.objects.last().unwrap().story_id().unwrap().0
}

#[test]
fn c11_style_import_renames_or_replaces_with_one_undo_and_keeps_define_from_selection() {
    let home = test_home::enter("c11_style_import_live");
    let mut workspace = Workspace::default();
    let src = workspace.new_document("src").0;
    let dst = workspace.new_document("dst").0;
    let mut config = Config::default();
    config.allow_dirs = vec![home.root.clone()];
    let mut s = Server::new(config, workspace);

    run(&mut s, src, "text.style.define", json!({ "id": "Head", "props": { "size_pt": 30.0 } }));
    run(&mut s, src, "text.style.define", json!({ "id": "Lead", "based_on": "Head", "props": { "bold": true } }));
    run(&mut s, src, "text.style.define", json!({ "id": "Body", "kind": "paragraph", "props": { "space_after": 6.0 } }));
    let file = home.root.join("styles_src.dtool");
    run(&mut s, src, "doc.save_as", json!({ "path": file }));

    let story = art(&mut s, dst, "Hello world");
    run(&mut s, dst, "text.style.define", json!({ "id": "Head", "props": { "size_pt": 12.0 } }));
    run(&mut s, dst, "text.style.apply_named", json!({ "story": story, "id": "Head", "range": { "start": 0, "end": 5 } }));
    assert_eq!(first_run_size(&s, dst, story), Some(12.0));
    run(&mut s, dst, "text.style.define_from_selection", json!({ "story": story, "id": "Picked", "range": { "start": 0, "end": 1 } }));
    assert!(style(&s, dst, StyleKind::Character, "Picked").is_some(), "create from selection still works");

    let reply = run(&mut s, dst, "text.style.import", json!({ "path": file }));
    let imported = reply["imported"].as_array().unwrap();
    assert_eq!(imported.len(), 3, "{reply}");
    assert!(imported.iter().any(|r| r["name"] == "Head" && r["as"] == "Head 2" && r["action"] == "renamed"), "{reply}");
    assert_eq!(style(&s, dst, StyleKind::Character, "Head").unwrap().1["size_pt"], 12.0, "rename keeps the existing style");
    assert_eq!(style(&s, dst, StyleKind::Character, "Head 2").unwrap().1["size_pt"], 30.0);
    assert_eq!(style(&s, dst, StyleKind::Character, "Lead").unwrap().0.as_deref(), Some("Head 2"), "based_on follows the rename");
    assert!(style(&s, dst, StyleKind::Paragraph, "Body").is_some());
    assert_eq!(first_run_size(&s, dst, story), Some(12.0));

    run(&mut s, dst, "edit.undo", json!({}));
    assert!(style(&s, dst, StyleKind::Character, "Head 2").is_none(), "import is one undo step");
    assert!(style(&s, dst, StyleKind::Character, "Lead").is_none());
    assert!(style(&s, dst, StyleKind::Paragraph, "Body").is_none());

    let reply = run(&mut s, dst, "text.style.import", json!({ "path": file, "names": ["Lead"], "conflict": "replace" }));
    let imported = reply["imported"].as_array().unwrap();
    assert_eq!(imported.len(), 2, "Lead brings its parent Head: {reply}");
    assert!(imported.iter().any(|r| r["name"] == "Head" && r["action"] == "replaced"), "{reply}");
    assert_eq!(style(&s, dst, StyleKind::Character, "Head").unwrap().1["size_pt"], 30.0);
    assert_eq!(first_run_size(&s, dst, story), Some(30.0), "story using Head relinks on replace");
    run(&mut s, dst, "edit.undo", json!({}));
    assert_eq!(first_run_size(&s, dst, story), Some(12.0));
    assert_eq!(style(&s, dst, StyleKind::Character, "Head").unwrap().1["size_pt"], 12.0);

    let bad = call(&mut s, dst, "text.style.import", json!({ "path": file, "conflict": "merge" }));
    assert!(bad.get("error").is_some(), "{bad}");
    let missing = call(&mut s, dst, "text.style.import", json!({ "path": file, "names": ["Nope"] }));
    assert!(missing.get("error").is_some(), "{missing}");
    let none = call(&mut s, dst, "text.style.import", json!({ "path": home.root.join("absent.dtool") }));
    assert!(none.get("error").is_some(), "{none}");
}