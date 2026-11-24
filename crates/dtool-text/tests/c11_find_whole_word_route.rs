use dtool_core::workspace::{ContextId, Scope, SharedServices, Workspace};
use dtool_core::{Command, Story, StoryId};
use dtool_core::serde_json::{json, Value};
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

#[test]
fn c11_whole_word_commands_on_live_document() {
    let (mut ws, id) = workspace("cat catalog cat cats");
    let found = run(&mut ws, id, "text.find", json!({ "pattern": "cat", "whole_word": true, "from": { "story": 3, "at": 3 } })).unwrap();
    assert_eq!(found["match"], json!({ "story": 3, "start": 12, "end": 15 }));
    let substring = run(&mut ws, id, "text.find", json!({ "pattern": "cat", "whole_word": false, "from": { "story": 3, "at": 3 } })).unwrap();
    assert_eq!(substring["match"], json!({ "story": 3, "start": 4, "end": 7 }));
    let reply = run(&mut ws, id, "text.replace_all", json!({ "pattern": "cat", "replacement": "dog", "whole_word": true })).unwrap();
    assert_eq!(reply["replaced"], json!(2));
    assert_eq!(text(&ws, id), "dog catalog dog cats");
    run(&mut ws, id, "edit.undo", json!({})).unwrap();
    assert_eq!(text(&ws, id), "cat catalog cat cats");
    assert_eq!(run(&mut ws, id, "text.find", json!({ "pattern": "cat", "whole_word": "yes" })).unwrap_err().code, "INVALID_PARAMS");
}
