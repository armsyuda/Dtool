use dtool_core::colour::{ColourFormat, ColourModel};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::paint::{range_fill, set_range_fill, Colour, Paint};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn session() -> Session {
    let mut document = Document::new([400.0, 300.0], Units::Pt, 72.0, 1);
    let mut coloured = json!({ "start": 0, "end": 5 });
    set_range_fill(&mut coloured, Some(&Paint::Solid { colour: Colour::new(180, 90, 45, 255) }));
    let plain = json!({ "start": 5, "end": 11 });
    document.stories.push(Story { id: StoryId(3), text: "Hello world".to_string(), ranges: vec![coloured, plain], specials: Vec::new(), paras: Vec::new() });
    Session::new(document)
}

fn fill_model(session: &Session) -> Option<ColourModel> {
    match range_fill(&session.document.stories[0].ranges[0]) {
        Some(Paint::Solid { colour }) => Some(colour.model),
        _ => None,
    }
}

#[test]
fn profile_convert_converts_story_range_fills_in_same_undo_step() {
    let mut session = session();
    let before = serde_json::to_value(&session.document.stories).unwrap();
    assert_eq!(fill_model(&session), Some(ColourModel::Rgb));
    let reply = run(&mut session, "color.profile.convert", json!({ "format": ColourFormat::LAB_16.to_string(), "profile": "LAB D50" }));
    assert!(reply["converted"].as_u64().unwrap() >= 1);
    assert_ne!(fill_model(&session), Some(ColourModel::Rgb));
    assert!(fill_model(&session).is_some());
    assert!(range_fill(&session.document.stories[0].ranges[1]).is_none());
    assert_eq!(session.document.stories[0].text, "Hello world");
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(serde_json::to_value(&session.document.stories).unwrap(), before);
    assert_eq!(fill_model(&session), Some(ColourModel::Rgb));
    run(&mut session, "edit.redo", json!({}));
    assert_ne!(fill_model(&session), Some(ColourModel::Rgb));
}

#[test]
fn old_convert_undo_without_stories_still_restores() {
    let mut session = session();
    let before = serde_json::to_value(&session.document.stories).unwrap();
    let snapshot = json!({ "objects": [], "colour": serde_json::to_value(&session.document.colour).unwrap(), "swatches": [] });
    run(&mut session, "color.profile.convert.restore", snapshot);
    assert_eq!(serde_json::to_value(&session.document.stories).unwrap(), before);
}