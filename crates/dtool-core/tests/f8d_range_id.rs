use dtool_core::colour::swatch::{range_id, SwatchBinding, RANGE_ID_KEY};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::paint::{range_fill, set_range_fill, Colour, Paint};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

const RED: Colour = Colour::new(255, 0, 0, 255);
const BLUE: Colour = Colour::new(0, 0, 255, 255);
const WHITE: Colour = Colour::new(255, 255, 255, 255);

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn session() -> Session {
    let mut document = Document::new([400.0, 300.0], Units::Pt, 72.0, 1);
    let mut ranges = vec![json!({ "start": 0, "end": 5 }), json!({ "start": 5, "end": 11 })];
    for range in &mut ranges {
        set_range_fill(range, Some(&Paint::Solid { colour: WHITE }));
    }
    document.stories.push(Story { id: StoryId(7), text: "Hello world".to_string(), ranges, specials: Vec::new(), paras: Vec::new() });
    Session::new(document)
}

fn fill(session: &Session, index: usize) -> Option<Paint> {
    range_fill(&session.document.stories[0].ranges[index])
}

#[test]
fn text_binding_follows_stable_range_id_after_ranges_shift() {
    let mut session = session();
    let id = run(&mut session, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "global": true }))["id"].as_u64().unwrap();
    let reply = run(&mut session, "swatch.apply", json!({ "id": id, "to": "text", "story": 7, "range": 1 }));
    let stable = reply["range_id"].as_u64().unwrap();
    assert_eq!(range_id(&session.document.stories[0].ranges[1]), Some(stable));
    assert_eq!(session.document.swatch_bindings[0].range_id, Some(stable));
    let mut inserted = json!({ "start": 0, "end": 0 });
    set_range_fill(&mut inserted, Some(&Paint::Solid { colour: WHITE }));
    session.document.stories[0].ranges.insert(0, inserted);
    let reply = run(&mut session, "swatch.edit", json!({ "id": id, "colour": "#0000FF" }));
    assert_eq!(reply["updated_stories"], json!([7]));
    assert_eq!(fill(&session, 2), Some(Paint::Solid { colour: BLUE }));
    assert_eq!(fill(&session, 1), Some(Paint::Solid { colour: WHITE }));
    assert_eq!(fill(&session, 0), Some(Paint::Solid { colour: WHITE }));
    let text = serde_json::to_string(&session.document).unwrap();
    let loaded: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(loaded.swatch_bindings[0].range_id, Some(stable));
    assert_eq!(range_id(&loaded.stories[0].ranges[2]), Some(stable));
}

#[test]
fn apply_undo_removes_range_id_and_binding() {
    let mut session = session();
    let id = run(&mut session, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "global": true }))["id"].as_u64().unwrap();
    run(&mut session, "swatch.apply", json!({ "id": id, "to": "text", "story": 7, "range": 0 }));
    assert_eq!(fill(&session, 0), Some(Paint::Solid { colour: RED }));
    run(&mut session, "edit.undo", json!({}));
    assert!(session.document.stories[0].ranges[0].get(RANGE_ID_KEY).is_none());
    assert!(session.document.swatch_bindings.is_empty());
    assert_eq!(fill(&session, 0), Some(Paint::Solid { colour: WHITE }));
}

#[test]
fn reapplying_same_range_keeps_one_binding_and_id() {
    let mut session = session();
    let id = run(&mut session, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "global": true }))["id"].as_u64().unwrap();
    let first = run(&mut session, "swatch.apply", json!({ "id": id, "to": "text", "story": 7, "range": 1 }))["range_id"].clone();
    let second = run(&mut session, "swatch.apply", json!({ "id": id, "to": "text", "story": 7, "range": 1 }))["range_id"].clone();
    assert_eq!(first, second);
    assert_eq!(session.document.swatch_bindings.len(), 1);
    let other = run(&mut session, "swatch.apply", json!({ "id": id, "to": "text", "story": 7, "range": 0 }))["range_id"].clone();
    assert_ne!(other, first);
    assert_eq!(session.document.swatch_bindings.len(), 2);
}

#[test]
fn legacy_index_binding_still_loads_and_follows_edit() {
    let mut session = session();
    let id = run(&mut session, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "global": true }))["id"].as_u64().unwrap();
    set_range_fill(&mut session.document.stories[0].ranges[1], Some(&Paint::Solid { colour: RED }));
    let legacy: SwatchBinding = serde_json::from_value(json!({ "object": 0, "target": "Text", "swatch": id, "story": 7, "range": 1 })).unwrap();
    assert_eq!(legacy.range_id, None);
    session.document.swatch_bindings.push(legacy);
    run(&mut session, "swatch.edit", json!({ "id": id, "colour": "#0000FF" }));
    assert_eq!(fill(&session, 1), Some(Paint::Solid { colour: BLUE }));
    assert_eq!(fill(&session, 0), Some(Paint::Solid { colour: WHITE }));
}