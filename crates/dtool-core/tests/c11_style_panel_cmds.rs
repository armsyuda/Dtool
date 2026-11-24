use dtool_core::commands::text_styles_cmd;
use dtool_core::document::{Document, Units};
use dtool_core::text_styles::StyleKind;
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
fn detach_named_keeps_look_and_undoes() {
    let (mut session, story) = live("Hello styled world");
    run(&mut session, "text.style.define", json!({ "id": "Emph", "kind": "character", "props": { "italic": true, "tracking": 40.0 } }));
    run(&mut session, "text.style.apply_named", json!({ "story": story, "id": "Emph", "kind": "character", "range": { "start": 0, "end": 5 }, "keep_overrides": false }));
    assert_eq!(run_at(&session, story, 0)["char_style"], json!("Emph"));
    run(&mut session, "text.style.detach", json!({ "id": "Emph", "kind": "character" }));
    let detached = run_at(&session, story, 0);
    assert_ne!(detached.get("char_style"), Some(&json!("Emph")), "{detached}");
    assert_eq!(detached["italic"], json!(true), "{detached}");
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(run_at(&session, story, 0)["char_style"], json!("Emph"));
}

#[test]
fn set_no_style_on_the_range_and_undo() {
    let (mut session, story) = live("Hello styled world");
    run(&mut session, "text.style.define", json!({ "id": "Emph", "kind": "character", "props": { "italic": true } }));
    run(&mut session, "text.style.apply_named", json!({ "story": story, "id": "Emph", "kind": "character", "range": { "start": 0, "end": 5 }, "keep_overrides": false }));
    run(&mut session, "text.style.set_no_style", json!({ "story": story, "kind": "character", "range": { "start": 0, "end": 5 } }));
    let plain = run_at(&session, story, 0);
    assert_ne!(plain.get("char_style"), Some(&json!("Emph")), "{plain}");
    assert_eq!(plain["italic"], json!(true), "{plain}");
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(run_at(&session, story, 0)["char_style"], json!("Emph"));
}

#[test]
fn delete_unused_leaves_applied_styles_and_undoes() {
    let (mut session, story) = live("Hello styled world");
    run(&mut session, "text.style.define", json!({ "id": "Emph", "kind": "character", "props": { "italic": true } }));
    run(&mut session, "text.style.define", json!({ "id": "Spare", "kind": "character", "props": { "tracking": 20.0 } }));
    run(&mut session, "text.style.apply_named", json!({ "story": story, "id": "Emph", "kind": "character", "range": { "start": 0, "end": 5 }, "keep_overrides": false }));
    let removed = run(&mut session, "text.style.delete_unused", json!({ "kind": "character" }));
    let names: Vec<_> = removed["removed"].as_array().unwrap().iter().map(|item| item["name"].as_str().unwrap().to_string()).collect();
    assert!(names.contains(&"Spare".to_string()), "{removed}");
    assert!(!names.contains(&"Emph".to_string()), "{removed}");
    let listed = run(&mut session, "text.styles.list", json!({ "kind": "character" }));
    assert!(listed["styles"].as_array().unwrap().iter().any(|style| style["name"] == json!("Emph")), "{listed}");
    assert!(listed["styles"].as_array().unwrap().iter().all(|style| style["name"] != json!("Spare")), "{listed}");
    run(&mut session, "edit.undo", json!({}));
    let restored = run(&mut session, "text.styles.list", json!({ "kind": "character" }));
    assert!(restored["styles"].as_array().unwrap().iter().any(|style| style["name"] == json!("Spare")), "{restored}");
}

#[test]
fn define_and_list_persist_next_style() {
    let (mut session, _) = live("Hello");
    run(&mut session, "text.style.define", json!({ "id": "Body", "kind": "paragraph", "props": { "space_after": 8.0 }, "next_style": "Body" }));
    let listed = run(&mut session, "text.styles.list", json!({ "kind": "paragraph" }));
    let body = listed["styles"].as_array().unwrap().iter().find(|style| style["name"] == json!("Body")).expect("body");
    assert_eq!(body["next_style"], json!("Body"), "{body}");
    let stored = session.document.text_styles.get(StyleKind::Paragraph, "Body").unwrap();
    assert_eq!(stored.next_style.as_deref(), Some("Body"));
}
