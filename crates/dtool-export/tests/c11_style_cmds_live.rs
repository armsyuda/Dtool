use dtool_core::commands::text_styles_cmd;

use dtool_core::document::{Document, Units};
use dtool_core::{Command, ObjectId, Session, StoryId};
use dtool_text::frame::layout_doc_flow;
use dtool_text::FontLibrary;
use serde_json::{json, Value};

const TEXT: &str = "first paragraph line\nsecond paragraph line";

fn live() -> (Session, StoryId) {
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [40, 40, 500, 400], "text": TEXT}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    session.run(&Command::with_params("text.style", json!({"story": story.0, "range": {"start": 0, "end": TEXT.chars().count()}, "patch": {"size_pt": 12.0}}))).unwrap();
    (session, story)
}

fn style(session: &mut Session, name: &str, params: Value) -> Value {
    text_styles_cmd::run(session, &Command::with_params(name, params)).expect("routed").expect("ok")
}

fn baselines(session: &Session, story: StoryId) -> Vec<f32> {
    let lib = FontLibrary::system();
    let flow = layout_doc_flow(&lib, &session.document, story).expect("flow");
    flow.layout.per_frame.iter().flatten().map(|line| line.baseline).collect()
}

fn run_at(session: &Session, story: StoryId, pos: usize, key: &str) -> Value {
    let s = session.document.stories.iter().find(|s| s.id == story).unwrap();
    s.ranges.iter().find(|r| r["start"].as_u64().unwrap() as usize <= pos && r["end"].as_u64().unwrap() as usize > pos).and_then(|r| r.get(key).cloned()).unwrap_or(Value::Null)
}

#[test]
fn live_define_and_apply_named_reflows_the_frame_and_undo_restores() {
    let (mut session, story) = live();
    let plain = baselines(&session, story);
    style(&mut session, "text.style.define", json!({"id": "Big", "props": {"size_pt": 36.0}}));
    assert_eq!(session.document.text_styles.names(dtool_core::text_styles::StyleKind::Character), vec!["Big".to_string()]);
    let version = session.document.version;
    style(&mut session, "text.style.apply_named", json!({"story": story.0, "id": "Big", "range": {"start": 0, "end": 5}}));
    assert!(session.document.version > version);
    assert_eq!(run_at(&session, story, 0, "char_style"), json!("Big"));
    assert_eq!(run_at(&session, story, 0, "size_pt"), json!(36.0));
    assert_eq!(run_at(&session, story, 6, "size_pt"), json!(12.0));
    let big = baselines(&session, story);
    assert!(big[0] > plain[0] + 10.0, "{plain:?} {big:?}");
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(baselines(&session, story), plain);
    assert_eq!(run_at(&session, story, 0, "char_style"), Value::Null);
}

#[test]
fn live_override_then_clear_override_returns_to_the_style() {
    let (mut session, story) = live();
    style(&mut session, "text.style.define", json!({"id": "Big", "props": {"size_pt": 36.0}}));
    style(&mut session, "text.style.apply_named", json!({"story": story.0, "id": "Big", "range": {"start": 0, "end": 5}}));
    let styled = baselines(&session, story);
    let mut runs = session.document.stories.iter().find(|s| s.id == story).unwrap().ranges.clone();
    for run in runs.iter_mut().filter(|r| r["char_style"] == json!("Big")) {
        run["size_pt"] = json!(20.0);
        run["style_overrides"] = json!({"size_pt": 20.0});
    }
    session.document.stories.iter_mut().find(|s| s.id == story).unwrap().ranges = runs;
    assert_ne!(baselines(&session, story), styled);
    style(&mut session, "text.style.clear_override", json!({"story": story.0, "range": {"start": 0, "end": 5}}));
    assert_eq!(run_at(&session, story, 0, "size_pt"), json!(36.0));
    assert_eq!(run_at(&session, story, 0, "style_overrides"), Value::Null);
    assert_eq!(baselines(&session, story), styled);
}

#[test]
fn live_paragraph_style_covers_the_whole_paragraph() {
    let (mut session, story) = live();
    style(&mut session, "text.style.define", json!({"id": "Body", "kind": "paragraph", "props": {"space_before": 18.0}}));
    style(&mut session, "text.style.apply_named", json!({"story": story.0, "id": "Body", "kind": "paragraph", "range": {"start": 2, "end": 3}}));
    let s = session.document.stories.iter().find(|s| s.id == story).unwrap();
    let run = s.paras.iter().find(|p| p["para_style"] == json!("Body")).expect("para run");
    assert_eq!(run["start"], json!(0));
    assert_eq!(run["end"], json!(21));
}

#[test]
fn live_define_from_selection_list_and_remove() {
    let (mut session, story) = live();
    style(&mut session, "text.style.define_from_selection", json!({"story": story.0, "id": "Picked", "range": {"start": 0, "end": 3}}));
    let listed = style(&mut session, "text.styles.list", json!({}));
    let picked = listed["styles"].as_array().unwrap().iter().find(|s| s["name"] == json!("Picked")).expect("listed").clone();
    assert_eq!(picked["props"]["size_pt"], json!(12.0));
    assert!(picked["props"].get("start").is_none());
    let undo = session.undo.clone();
    style(&mut session, "text.style.remove", json!({"id": "Picked"}));
    assert!(session.document.text_styles.styles.is_empty());
    assert_ne!(format!("{:?}", session.undo), format!("{undo:?}"));
    assert!(text_styles_cmd::run(&mut session, &Command::with_params("text.style.remove", json!({"id": "Picked"}))).unwrap().is_err());
}

#[test]
fn live_store_restore_is_not_a_user_command_and_apply_restores() {
    let (mut session, _) = live();
    assert_eq!(text_styles_cmd::can_execute(&session, &Command::new(text_styles_cmd::STORE_RESTORE)), Some(false));
    assert!(text_styles_cmd::run(&mut session, &Command::new(text_styles_cmd::STORE_RESTORE)).unwrap().is_err());
    style(&mut session, "text.style.define", json!({"id": "A", "props": {"size_pt": 9.0}}));
    let empty = Command::with_params(text_styles_cmd::STORE_RESTORE, json!({"store": {"styles": []}}));
    text_styles_cmd::apply(&mut session, &empty).unwrap().unwrap();
    assert!(session.document.text_styles.styles.is_empty());
}
