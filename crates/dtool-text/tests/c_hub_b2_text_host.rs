use dtool_core::workspace::{ContextId, Scope, SharedServices, Workspace};
use dtool_core::{Command, Story, StoryId};
use dtool_core::serde_json::{self, json, Value};
use std::sync::Arc;

fn workspace(text: &str) -> (Workspace, ContextId) {
    assert!(dtool_text::text_host::install());
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let id = ws.new_document("Find");
    let session = &mut ws.get_mut(id).unwrap().session;
    let mut story = Story { id: StoryId(3), text: text.into(), ..Default::default() };
    story.ranges.push(Default::default());
    story.ranges[0]["start"] = 0u64.into();
    story.ranges[0]["end"] = (text.chars().count() as u64).into();
    story.ranges[0]["size_pt"] = 12.0f64.into();
    session.document.stories.push(story);
    (ws, id)
}

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> dtool_core::CommandResult {
    ws.run(Scope::of(id), Command::with_params(name, params)).1
}

fn text(ws: &Workspace, id: ContextId) -> String {
    ws.get(id).unwrap().session.document.stories[0].text.clone()
}

fn generation(ws: &Workspace, id: ContextId) -> u64 {
    ws.get(id).unwrap().session.generation
}

fn can_undo(ws: &Workspace, id: ContextId) -> bool {
    ws.get(id).unwrap().session.undo.can_undo()
}

#[test]
fn find_is_a_query_on_the_live_document() {
    let (mut ws, id) = workspace("cat and dog\nthe Cat sat");
    let before = generation(&ws, id);
    let reply = run(&mut ws, id, "text.find", json!({ "pattern": "cat", "case_sensitive": false, "from": { "story": 3, "at": 3 } })).unwrap();
    assert_eq!(reply["match"], json!({ "story": 3, "start": 16, "end": 19 }));
    let none = run(&mut ws, id, "text.find", json!({ "pattern": "zebra" })).unwrap();
    assert_eq!(none["match"], Value::Null);
    assert_eq!(generation(&ws, id), before);
    assert!(!can_undo(&ws, id));
    assert_eq!(run(&mut ws, id, "text.find", json!({ "pattern": "(", "regex": true })).unwrap_err().code, "INVALID_PATTERN");
    assert_eq!(run(&mut ws, id, "text.find", json!({})).unwrap_err().code, "INVALID_PARAMS");
}

#[test]
fn replace_one_is_one_undo_and_rejects_stale_target() {
    let (mut ws, id) = workspace("cat and cat");
    let reply = run(&mut ws, id, "text.replace", json!({ "pattern": "cat", "replacement": "dog", "target": { "story": 3, "start": 8, "end": 11 } })).unwrap();
    assert_eq!(reply["match"]["start"], json!(8));
    assert_eq!(text(&ws, id), "cat and dog");
    assert!(reply["doc_version"].as_u64().unwrap() > 0);
    let stale = run(&mut ws, id, "text.replace", json!({ "pattern": "cat", "replacement": "dog", "target": { "story": 3, "start": 8, "end": 11 } })).unwrap_err();
    assert_eq!(stale.code, "STALE");
    assert_eq!(text(&ws, id), "cat and dog");
    run(&mut ws, id, "edit.undo", json!({})).unwrap();
    assert_eq!(text(&ws, id), "cat and cat");
    assert!(!can_undo(&ws, id));
}

#[test]
fn replace_all_is_one_undo_and_bad_pattern_changes_nothing() {
    let (mut ws, id) = workspace("a1 b22 c333");
    let before = generation(&ws, id);
    assert_eq!(run(&mut ws, id, "text.replace_all", json!({ "pattern": "[", "regex": true, "replacement": "#" })).unwrap_err().code, "INVALID_PATTERN");
    assert_eq!(generation(&ws, id), before);
    let reply = run(&mut ws, id, "text.replace_all", json!({ "pattern": "\\d+", "regex": true, "replacement": "#" })).unwrap();
    assert_eq!(reply["replaced"], json!(3));
    assert_eq!(text(&ws, id), "a# b# c#");
    run(&mut ws, id, "edit.undo", json!({})).unwrap();
    assert_eq!(text(&ws, id), "a1 b22 c333");
    assert!(!can_undo(&ws, id));
}

#[test]
fn font_list_and_preview_leave_the_document_alone() {
    let (mut ws, id) = workspace("Hello");
    let list = run(&mut ws, id, "font.list", json!({})).unwrap();
    let families = list["families"].as_array().expect("families").clone();
    let before = generation(&ws, id);
    let story_before = serde_json::to_string(&ws.get(id).unwrap().session.document.stories[0]).unwrap();
    assert_eq!(run(&mut ws, id, "text.font.preview", json!({ "story": 3, "range": [0, 5], "family": "No Such Family 123" })).unwrap_err().code, "NOT_FOUND");
    if let Some(family) = families.first().and_then(Value::as_str) {
        let reply = run(&mut ws, id, "text.font.preview", json!({ "story": 3, "range": { "start": 0, "end": 5 }, "family": family })).unwrap();
        assert_eq!(reply["preview"]["family"], json!(family));
        assert_eq!(dtool_text::font_preview::font_preview().unwrap().story, 3);
    }
    let cancel = run(&mut ws, id, "text.font.preview.cancel", json!({})).unwrap();
    assert_eq!(cancel["preview"], Value::Null);
    assert!(dtool_text::font_preview::font_preview().is_none());
    assert_eq!(generation(&ws, id), before);
    assert!(!can_undo(&ws, id));
    assert_eq!(serde_json::to_string(&ws.get(id).unwrap().session.document.stories[0]).unwrap(), story_before);
}

#[test]
fn catalog_rows_are_implemented_and_preflight_follows_handler() {
    let (ws, id) = workspace("x");
    let catalog = &ws.get(id).unwrap().session.catalog;
    for (name, mutating) in [("font.list", false), ("text.font.preview", false), ("text.font.preview.cancel", false), ("text.find", false), ("text.replace", true), ("text.replace_all", true)] {
        let spec = catalog.resolve(name).expect(name);
        assert!(spec.implemented, "{name}");
        assert_eq!(spec.mutating, mutating, "{name}");
    }
    let session = &ws.get(id).unwrap().session;
    assert!(session.can_execute(&Command::with_params("text.find", json!({ "pattern": "x" }))));
    assert!(!session.can_execute(&Command::with_params("text.find", json!({}))));
    assert!(!session.can_execute(&Command::with_params("text.replace", json!({ "pattern": "x", "replacement": "y" }))));
}