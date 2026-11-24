use dtool_core::commands::text_styles_cmd;

use dtool_core::document::{Document, Units};
use dtool_core::text_styles::StyleKind;
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

fn routed(session: &Session) -> bool {
    session.can_execute(&Command::with_params("text.style.define", json!({"id": "probe"})))
}

fn style(session: &mut Session, name: &str, params: Value) -> Value {
    let cmd = Command::with_params(name, params);
    if routed(session) {
        session.run(&cmd).unwrap()
    } else {
        text_styles_cmd::run(session, &cmd).expect("handled").expect("ok")
    }
}

fn undo(session: &mut Session) {
    if routed(session) {
        session.run(&Command::new("edit.undo")).unwrap();
        return;
    }
    let cmd = session.undo.undo().expect("undo step");
    match text_styles_cmd::apply(session, &cmd) {
        Some(result) => result.unwrap(),
        None => {
            session.undo.redo();
            session.run(&Command::new("edit.undo")).unwrap();
        }
    }
}

fn story(session: &Session, id: StoryId) -> dtool_core::document::Story {
    session.document.stories.iter().find(|s| s.id == id).unwrap().clone()
}

fn canonical(story: dtool_core::document::Story) -> dtool_core::document::Story {
    serde_json::from_value(serde_json::to_value(story).unwrap()).unwrap()
}

fn at(session: &Session, id: StoryId, pos: usize, key: &str) -> Value {
    story(session, id).ranges.iter().find(|r| r["start"].as_u64().unwrap() as usize <= pos && r["end"].as_u64().unwrap() as usize > pos).and_then(|r| r.get(key).cloned()).unwrap_or(Value::Null)
}

fn para_at(session: &Session, id: StoryId, pos: usize, key: &str) -> Value {
    story(session, id).paras.iter().find(|r| r["start"].as_u64().unwrap() as usize <= pos && r["end"].as_u64().unwrap() as usize > pos).and_then(|r| r.get(key).cloned()).unwrap_or(Value::Null)
}

fn baselines(session: &Session, id: StoryId) -> Vec<f32> {
    let lib = FontLibrary::system();
    layout_doc_flow(&lib, &session.document, id).expect("flow").layout.per_frame.iter().flatten().map(|l| l.baseline).collect()
}

fn styled() -> (Session, StoryId) {
    let (mut session, id) = live();
    style(&mut session, "text.style.define", json!({"id": "Head", "props": {"size_pt": 24.0, "fill": "red"}}));
    style(&mut session, "text.style.apply_named", json!({"story": id.0, "id": "Head", "range": {"start": 0, "end": 5}}));
    style(&mut session, "text.style.apply_named", json!({"story": id.0, "id": "Head", "range": {"start": 21, "end": 27}}));
    (session, id)
}

#[test]
fn redefine_updates_every_run_using_the_style_in_one_undo_step_and_reflows() {
    let (mut session, id) = styled();
    let before = baselines(&session, id);
    let snapshot = story(&session, id);
    let steps = session.undo.applied_steps();
    let reply = style(&mut session, "text.style.define", json!({"id": "Head", "props": {"size_pt": 40.0, "fill": "red"}}));
    assert_eq!(reply["relinked_stories"], json!([id.0]));
    assert_eq!(session.undo.applied_steps(), steps + 1);
    assert_eq!(at(&session, id, 0, "size_pt"), json!(40.0));
    assert_eq!(at(&session, id, 22, "size_pt"), json!(40.0));
    assert_eq!(at(&session, id, 8, "size_pt"), json!(12.0));
    let after = baselines(&session, id);
    assert!(after[0] > before[0] + 5.0, "{before:?} {after:?}");
    undo(&mut session);
    assert_eq!(canonical(story(&session, id)), canonical(snapshot.clone()));
    assert_eq!(session.document.text_styles.resolve(StyleKind::Character, "Head").unwrap()["size_pt"], json!(24.0));
    assert_eq!(baselines(&session, id), before);
}

#[test]
fn redefine_keeps_explicit_and_local_overrides() {
    let (mut session, id) = styled();
    let mut s = story(&session, id);
    for run in s.ranges.iter_mut().filter(|r| r["char_style"] == json!("Head")) {
        if run["start"] == json!(0) {
            run["fill"] = json!("blue");
            run["style_overrides"] = json!({"fill": "blue"});
        } else {
            run["size_pt"] = json!(30.0);
        }
    }
    session.document.stories.iter_mut().find(|x| x.id == id).unwrap().ranges = s.ranges;
    style(&mut session, "text.style.define", json!({"id": "Head", "props": {"size_pt": 40.0, "fill": "green", "weight": 700}}));
    assert_eq!(at(&session, id, 0, "fill"), json!("blue"));
    assert_eq!(at(&session, id, 0, "size_pt"), json!(40.0));
    assert_eq!(at(&session, id, 0, "weight"), json!(700));
    assert_eq!(at(&session, id, 22, "size_pt"), json!(30.0));
    assert_eq!(at(&session, id, 22, "fill"), json!("green"));
    assert_eq!(at(&session, id, 22, "style_overrides"), json!({"size_pt": 30.0}));
    style(&mut session, "text.style.define", json!({"id": "Head", "props": {"size_pt": 40.0}}));
    assert_eq!(at(&session, id, 22, "fill"), Value::Null);
    assert_eq!(at(&session, id, 0, "fill"), json!("blue"));
}

#[test]
fn redefining_a_parent_flows_to_text_using_a_child_style() {
    let (mut session, id) = live();
    style(&mut session, "text.style.define", json!({"id": "Base", "props": {"size_pt": 14.0}}));
    style(&mut session, "text.style.define", json!({"id": "Child", "based_on": "Base", "props": {"fill": "red"}}));
    style(&mut session, "text.style.apply_named", json!({"story": id.0, "id": "Child", "range": {"start": 0, "end": 5}}));
    style(&mut session, "text.style.define", json!({"id": "Base", "props": {"size_pt": 28.0}}));
    assert_eq!(at(&session, id, 0, "size_pt"), json!(28.0));
    assert_eq!(at(&session, id, 0, "fill"), json!("red"));
}

#[test]
fn remove_keeps_the_look_as_local_formatting_and_undo_relinks() {
    let (mut session, id) = styled();
    let before = baselines(&session, id);
    let snapshot = story(&session, id);
    let steps = session.undo.applied_steps();
    style(&mut session, "text.style.remove", json!({"id": "Head"}));
    assert_eq!(session.undo.applied_steps(), steps + 1);
    assert!(story(&session, id).ranges.iter().all(|r| r.get("char_style").is_none() && r.get("style_overrides").is_none()));
    assert_eq!(at(&session, id, 0, "size_pt"), json!(24.0));
    assert_eq!(at(&session, id, 0, "fill"), json!("red"));
    assert_eq!(baselines(&session, id), before);
    undo(&mut session);
    assert_eq!(canonical(story(&session, id)), canonical(snapshot.clone()));
    assert!(session.document.text_styles.get(StyleKind::Character, "Head").is_some());
    style(&mut session, "text.style.define", json!({"id": "Head", "props": {"size_pt": 36.0, "fill": "red"}}));
    assert_eq!(at(&session, id, 0, "size_pt"), json!(36.0));
    assert_eq!(at(&session, id, 0, "style_overrides"), Value::Null);
}

#[test]
fn removing_a_parent_keeps_child_text_unchanged() {
    let (mut session, id) = live();
    style(&mut session, "text.style.define", json!({"id": "Base", "props": {"size_pt": 14.0}}));
    style(&mut session, "text.style.define", json!({"id": "Child", "based_on": "Base", "props": {"fill": "red"}}));
    style(&mut session, "text.style.apply_named", json!({"story": id.0, "id": "Child", "range": {"start": 0, "end": 5}}));
    let snapshot = story(&session, id);
    let reply = style(&mut session, "text.style.remove", json!({"id": "Base"}));
    assert_eq!(reply["relinked_stories"], json!([]));
    assert_eq!(canonical(story(&session, id)), canonical(snapshot.clone()));
    assert_eq!(session.document.text_styles.resolve(StyleKind::Character, "Child").unwrap()["size_pt"], json!(14.0));
}

#[test]
fn paragraph_style_redefine_and_remove_follow_the_same_rules() {
    let (mut session, id) = live();
    style(&mut session, "text.style.define", json!({"id": "Body", "kind": "paragraph", "props": {"space_before": 6.0}}));
    style(&mut session, "text.style.apply_named", json!({"story": id.0, "id": "Body", "kind": "paragraph", "range": {"start": 2, "end": 3}}));
    style(&mut session, "text.style.define", json!({"id": "Body", "kind": "paragraph", "props": {"space_before": 18.0}}));
    assert_eq!(para_at(&session, id, 0, "space_before"), json!(18.0));
    style(&mut session, "text.style.remove", json!({"id": "Body", "kind": "paragraph"}));
    assert_eq!(para_at(&session, id, 0, "space_before"), json!(18.0));
    assert_eq!(para_at(&session, id, 0, "para_style"), Value::Null);
}
