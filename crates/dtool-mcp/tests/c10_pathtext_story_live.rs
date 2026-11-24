#[path = "../../dtool-core/tests/support/test_home.rs"]
mod test_home;

use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn call(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    let mut reply = call(server, scope, name, params.clone());
    if reply["error"]["code"] == "NEEDS_CONFIRMATION" {
        let id = reply["error"]["confirm_id"].as_u64().unwrap();
        dtool_mcp::permission::confirm_store().approve(id).unwrap();
        let status = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "mcp.confirm.status", "params": { "confirm_id": id } }));
        let mut params = params;
        params["confirm_token"] = status["result"]["confirm_token"].clone();
        reply = call(server, scope, name, params);
    }
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn with_doc<T>(server: &Server, scope: u64, f: impl FnOnce(&dtool_core::Session) -> T) -> T {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    f(&ws.get(ContextId(scope)).unwrap().session)
}

fn story_text(server: &Server, scope: u64, story: u64) -> String {
    with_doc(server, scope, |s| s.document.stories.iter().find(|st| st.id.0 == story).map(|st| st.text.clone()).unwrap_or_default())
}

fn path_text(server: &mut Server, scope: u64) -> (u64, u64) {
    run(server, scope, "object.create", json!({ "name": "curve", "pos": [10.0, 10.0], "size": [200.0, 80.0] }));
    let path = with_doc(server, scope, |s| s.document.objects.last().unwrap().id.0);
    run(server, scope, "text.path_text.add", json!({ "path": path }));
    let story = with_doc(server, scope, |s| match &s.document.get_object(ObjectId(path)).unwrap().kind {
        ObjectKind::PathText(data) => data.story.0,
        other => panic!("not path text: {other:?}"),
    });
    (path, story)
}

#[test]
fn c10_path_text_types_composed_korean_formats_undoes_and_round_trips() {
    let home = test_home::enter("c10_pathtext_story_live");
    let mut workspace = Workspace::default();
    let a = workspace.new_document("path").0;
    let mut config = Config::default();
    config.allow_dirs = vec![home.root.clone()];
    let mut s = Server::new(config, workspace);
    let (path, story) = path_text(&mut s, a);

    run(&mut s, a, "text.select", json!({ "story": story, "id": path, "range": { "start": 0, "end": 0 } }));
    let version = with_doc(&s, a, |x| x.document.version);
    run(&mut s, a, "text.edit.compose", json!({ "text": "ㅎ" }));
    run(&mut s, a, "text.edit.compose", json!({ "text": "하" }));
    assert_eq!(story_text(&s, a, story), "", "preedit is not document text");
    assert_eq!(with_doc(&s, a, |x| x.document.version), version, "preedit does not bump the document");
    run(&mut s, a, "text.edit.compose_commit", json!({ "text": "한" }));
    run(&mut s, a, "text.edit.compose", json!({ "text": "그" }));
    run(&mut s, a, "text.edit.compose_commit", json!({ "text": "글" }));
    run(&mut s, a, "text.edit.type", json!({ "text": " 길" }));
    assert_eq!(story_text(&s, a, story), "한글 길");
    run(&mut s, a, "text.edit.backspace", json!({}));
    run(&mut s, a, "text.edit.caret", json!({ "to": 0 }));
    run(&mut s, a, "text.edit.delete_forward", json!({}));
    assert_eq!(story_text(&s, a, story), "글 ");
    run(&mut s, a, "text.edit.end", json!({}));

    run(&mut s, a, "text.select", json!({ "story": story, "id": path, "range": { "start": 0, "end": 1 } }));
    run(&mut s, a, "text.style", json!({ "story": story, "range": { "start": 0, "end": 1 }, "patch": { "size_pt": 30.0, "bold": true } }));
    let q = run(&mut s, a, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 1 } }));
    assert_eq!(q["style"]["size_pt"].as_f64().or(q["size_pt"].as_f64()), Some(30.0), "{q}");

    run(&mut s, a, "edit.undo", json!({}));
    let q = run(&mut s, a, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 1 } }));
    assert_ne!(q["style"]["size_pt"].as_f64().or(q["size_pt"].as_f64()), Some(30.0), "format undone: {q}");
    run(&mut s, a, "edit.undo", json!({}));
    assert_eq!(story_text(&s, a, story), "한글 ", "caret move closed the typing step; delete_forward is its own step");
    run(&mut s, a, "edit.undo", json!({}));
    assert_eq!(story_text(&s, a, story), "", "compose commits + typing + backspace undo as one step");
    for _ in 0..3 {
        run(&mut s, a, "edit.redo", json!({}));
    }
    assert_eq!(story_text(&s, a, story), "글 ");
    let q = run(&mut s, a, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 1 } }));
    assert_eq!(q["style"]["size_pt"].as_f64().or(q["size_pt"].as_f64()), Some(30.0), "format redone: {q}");

    let file = home.root.join("path_text.dtool");
    run(&mut s, a, "doc.save_as", json!({ "path": file }));
    let opened = run(&mut s, a, "doc.open", json!({ "path": file }));
    let b = opened["context"].as_u64().or(opened["scope"].as_u64()).or(opened["id"].as_u64()).unwrap_or_else(|| panic!("{opened}"));
    let (kind_ok, text, size) = with_doc(&s, b, |x| {
        let object = x.document.get_object(ObjectId(path)).unwrap();
        let story = object.story_id().unwrap();
        let st = x.document.stories.iter().find(|st| st.id == story).unwrap();
        (matches!(object.kind, ObjectKind::PathText(_)), st.text.clone(), st.ranges.clone())
    });
    assert!(kind_ok);
    assert_eq!(text, "글 ");
    assert!(serde_json::to_string(&size).unwrap().contains("30"), "char format saved: {size:?}");

    run(&mut s, b, "text.select", json!({ "story": story, "id": path, "range": { "start": 2, "end": 2 } }));
    run(&mut s, b, "text.edit.compose", json!({ "text": "ㄱ" }));
    run(&mut s, b, "text.edit.end", json!({}));
    assert_eq!(story_text(&s, b, story), "글 ㄱ", "edit end commits preedit on reopened path text");
}