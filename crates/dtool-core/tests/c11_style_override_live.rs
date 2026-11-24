use dtool_core::commands::text_styles_cmd;
use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    if matches!(name, "text.style.detach" | "text.style.set_no_style" | "text.style.delete_unused") {
        return text_styles_cmd::run(session, &Command::with_params(name, params)).expect("handled").expect("ok");
    }
    session.run(&Command::with_params(name, params)).expect(name)
}

fn live(text: &str) -> (Session, u64) {
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let added = run(&mut session, "text.frame.add", json!({ "rect": [40, 40, 400, 240], "text": text }));
    let story = added["story"].as_u64().or_else(|| session.document.stories.first().map(|story| story.id.0)).expect("story");
    (session, story)
}

fn run_at(session: &Session, story: u64, at: usize) -> Value {
    let found = session.document.stories.iter().find(|item| item.id.0 == story).expect("story");
    found
        .ranges
        .iter()
        .find(|run| run["start"].as_u64().unwrap() as usize <= at && (run["end"].as_u64().unwrap() as usize) > at)
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn text_style_patch_records_overrides_for_clear() {
    let (mut session, story) = live("Hello styled world");
    run(&mut session, "text.style.define", json!({ "id": "Emph", "kind": "character", "props": { "italic": true, "tracking": 40.0 } }));
    run(&mut session, "text.style.apply_named", json!({ "story": story, "id": "Emph", "kind": "character", "range": { "start": 0, "end": 5 }, "keep_overrides": false }));
    run(&mut session, "text.style", json!({ "story": story, "range": { "start": 0, "end": 5 }, "patch": { "tracking": 80.0 } }));
    let patched = run_at(&session, story, 0);
    assert_eq!(patched["tracking"], json!(80.0), "{patched}");
    assert_eq!(patched["style_overrides"]["tracking"], json!(80.0), "{patched}");
    run(&mut session, "text.style.clear_override", json!({ "story": story, "kind": "character", "range": { "start": 0, "end": 5 } }));
    let cleared = run(&mut session, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_eq!(cleared["tracking"], json!(40.0), "{cleared}");
    run(&mut session, "edit.undo", json!({}));
    let after = run(&mut session, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_eq!(after["tracking"], json!(80.0), "{after}");
}
