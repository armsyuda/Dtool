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

fn story(server: &Server, scope: u64, id: u64) -> dtool_core::Story {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.stories.iter().find(|s| s.id.0 == id).unwrap().clone()
}

fn run_at(story: &dtool_core::Story, at: usize) -> Value {
    story.ranges.iter().find(|r| r["start"].as_u64().unwrap() as usize <= at && (r["end"].as_u64().unwrap() as usize) > at).cloned().unwrap_or(Value::Null)
}

fn art(server: &mut Server, scope: u64, text: &str) -> u64 {
    run(server, scope, "text.art.add", json!({ "pos": [0.0, 0.0], "text": text }));
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.objects.last().unwrap().story_id().unwrap().0
}

fn found(reply: &Value) -> (u64, u64, u64) {
    let m = &reply["match"];
    (m["story"].as_u64().unwrap_or(0), m["start"].as_u64().unwrap_or(99), m["end"].as_u64().unwrap_or(99))
}

#[test]
fn c11_find_and_replace_by_format_on_live_document() {
    let mut workspace = Workspace::default();
    let d = workspace.new_document("fmt").0;
    let mut s = Server::new(Config::default(), workspace);
    let a = art(&mut s, d, "Red apple red Apple");
    run(&mut s, d, "text.style", json!({ "story": a, "range": { "start": 0, "end": 3 }, "patch": { "size_pt": 20.0, "fill": "#ff0000", "family": "Arial" } }));
    run(&mut s, d, "text.style.define", json!({ "id": "Hi", "props": { "size_pt": 30.0, "weight": 700 } }));
    run(&mut s, d, "text.style.apply_named", json!({ "story": a, "id": "Hi", "range": { "start": 14, "end": 19 } }));

    let r = run(&mut s, d, "text.find", json!({ "pattern": "apple", "case_sensitive": false, "format": { "char_style": "Hi" } }));
    assert_eq!(found(&r), (a, 14, 19), "char style filter: {r}");
    let r = run(&mut s, d, "text.find", json!({ "pattern": "red", "case_sensitive": false, "whole_word": true, "format": { "size_pt": 20 } }));
    assert_eq!(found(&r), (a, 0, 3), "size + whole word: {r}");
    let r = run(&mut s, d, "text.find", json!({ "pattern": "r.d", "regex": true, "case_sensitive": false, "format": { "family": "arial" }, "from": { "story": a, "at": 1 } }));
    assert_eq!(found(&r), (a, 0, 3), "regex + family wraps to the only formatted hit: {r}");
    let r = run(&mut s, d, "text.find", json!({ "format": { "color": "#ff0000" } }));
    assert_eq!(found(&r), (a, 0, 3), "format-only colour span: {r}");
    let r = run(&mut s, d, "text.find", json!({ "pattern": "", "format": { "weight": 700 } }));
    assert_eq!(found(&r), (a, 14, 19), "format-only weight span: {r}");
    let r = run(&mut s, d, "text.find", json!({ "pattern": "apple", "format": { "size_pt": 99 } }));
    assert!(r["match"].is_null(), "no format hit: {r}");

    let b = art(&mut s, d, "on\nno");
    run(&mut s, d, "text.style.define", json!({ "id": "Body", "kind": "paragraph", "props": { "space_after": 6.0 } }));
    run(&mut s, d, "text.style.apply_named", json!({ "story": b, "id": "Body", "kind": "paragraph", "range": { "start": 3, "end": 4 } }));
    let r = run(&mut s, d, "text.find", json!({ "pattern": "n", "scope": "story", "story": b }));
    assert_eq!(found(&r), (b, 1, 2));
    let r = run(&mut s, d, "text.find", json!({ "pattern": "n", "scope": "story", "story": b, "format": { "para_style": "Body" } }));
    assert_eq!(found(&r), (b, 3, 4), "para style filter: {r}");

    let r = run(&mut s, d, "text.replace_all", json!({ "pattern": "red", "case_sensitive": false, "format": { "size_pt": 20 }, "replacement": "green", "replace_format": { "char_style": "Hi" } }));
    assert_eq!(r["replaced"], 1, "{r}");
    let st = story(&s, d, a);
    assert_eq!(st.text, "green apple red Apple");
    let run0 = run_at(&st, 0);
    assert_eq!(run0["char_style"], "Hi", "{run0}");
    assert_eq!(run0["size_pt"].as_f64(), Some(30.0), "{run0}");
    assert_eq!(run_at(&st, 12)["char_style"], Value::Null, "unformatted 'red' untouched");
    run(&mut s, d, "edit.undo", json!({}));
    let st = story(&s, d, a);
    assert_eq!(st.text, "Red apple red Apple", "replace with format is one undo step");
    assert_eq!(run_at(&st, 0)["size_pt"].as_f64(), Some(20.0));

    let r = run(&mut s, d, "text.replace_all", json!({ "format": { "char_style": "Hi" }, "replace_format": { "italic": true } }));
    assert_eq!(r["replaced"], 1, "{r}");
    let st = story(&s, d, a);
    assert_eq!(st.text, "Red apple red Apple", "format-only replace keeps text");
    assert_eq!(run_at(&st, 15)["italic"], true);
    assert_ne!(run_at(&st, 10)["italic"], true);

    let r = run(&mut s, d, "text.replace", json!({ "pattern": "apple", "format": { "italic": false }, "target": { "story": a, "start": 4, "end": 9 }, "replacement": "pear", "replace_format": { "color": "#00ff00" } }));
    assert_eq!(found(&r), (a, 4, 8), "{r}");
    assert_eq!(story(&s, d, a).text, "Red pear red Apple");

    assert!(call(&mut s, d, "text.find", json!({ "pattern": "" })).get("error").is_some(), "empty pattern without format is invalid");
    assert!(call(&mut s, d, "text.replace_all", json!({ "pattern": "Red" })).get("error").is_some(), "replace needs replacement or replace_format");
    assert!(call(&mut s, d, "text.replace_all", json!({ "pattern": "Red", "replace_format": { "para_style": "Body" } })).get("error").is_some(), "para keys are find-only");
}