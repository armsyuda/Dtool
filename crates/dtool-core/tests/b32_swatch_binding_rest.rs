use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{range_fill, set_range_fill, Colour, GradientGeom, GradientKind, GradientStop, Paint};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

const RED: Colour = Colour::new(255, 0, 0, 255);
const BLUE: Colour = Colour::new(0, 0, 255, 255);
const WHITE: Colour = Colour::new(255, 255, 255, 255);

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn gradient(first: Colour) -> Paint {
    Paint::Gradient {
        kind: GradientKind::Linear,
        stops: vec![GradientStop { offset: 0.0, colour: first }, GradientStop { offset: 1.0, colour: WHITE }],
        geom: GradientGeom::default(),
    }
}

fn session() -> Session {
    let mut document = Document::new([400.0, 300.0], Units::Pt, 72.0, 1);
    document.objects.push(Object { id: ObjectId(1), fill: gradient(WHITE), ..Object::default() });
    document.objects.push(Object { id: ObjectId(2), fill: gradient(RED), ..Object::default() });
    let mut ranges = vec![json!({ "start": 0, "end": 5 }), json!({ "start": 5, "end": 11 })];
    for range in &mut ranges {
        set_range_fill(range, Some(&Paint::Solid { colour: WHITE }));
    }
    document.stories.push(Story { id: StoryId(7), text: "Hello world".to_string(), ranges, specials: Vec::new(), paras: Vec::new() });
    Session::new(document)
}

fn global_red(session: &mut Session) -> u64 {
    run(session, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "global": true }))["id"].as_u64().unwrap()
}

fn stop0(session: &Session, index: usize) -> Colour {
    match &session.document.objects[index].fill {
        Paint::Gradient { stops, .. } => stops[0].colour,
        _ => panic!("gradient expected"),
    }
}

fn story_fill(session: &Session, range: usize) -> Option<Paint> {
    range_fill(&session.document.stories[0].ranges[range])
}

#[test]
fn linked_gradient_stop_follows_global_edit_and_unlinked_stays() {
    let mut session = session();
    let id = global_red(&mut session);
    run(&mut session, "swatch.apply", json!({ "id": id, "ids": [1], "stop": 0 }));
    assert_eq!(stop0(&session, 0), RED);
    assert_eq!(session.document.swatch_bindings[0].stop, Some(0));
    let reply = run(&mut session, "swatch.edit", json!({ "id": id, "colour": "#0000FF" }));
    assert_eq!(reply["updated"], json!([1]));
    assert_eq!(stop0(&session, 0), BLUE);
    assert_eq!(stop0(&session, 1), RED);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(stop0(&session, 0), RED);
    assert!(session.run(&Command::with_params("swatch.apply", json!({ "id": id, "ids": [1], "stop": 9 }))).is_err());
}

#[test]
fn linked_text_range_follows_global_edit_with_undo() {
    let mut session = session();
    let id = global_red(&mut session);
    run(&mut session, "swatch.apply", json!({ "id": id, "to": "text", "story": 7, "range": 1 }));
    assert_eq!(story_fill(&session, 1), Some(Paint::Solid { colour: RED }));
    assert_eq!(story_fill(&session, 0), Some(Paint::Solid { colour: WHITE }));
    let reply = run(&mut session, "swatch.edit", json!({ "id": id, "colour": "#0000FF" }));
    assert_eq!(reply["updated_stories"], json!([7]));
    assert_eq!(story_fill(&session, 1), Some(Paint::Solid { colour: BLUE }));
    assert_eq!(story_fill(&session, 0), Some(Paint::Solid { colour: WHITE }));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(story_fill(&session, 1), Some(Paint::Solid { colour: RED }));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(story_fill(&session, 1), Some(Paint::Solid { colour: WHITE }));
    assert!(session.document.swatch_bindings.is_empty());
}

#[test]
fn whole_story_binding_and_delete_detaches() {
    let mut session = session();
    let id = global_red(&mut session);
    run(&mut session, "swatch.apply", json!({ "id": id, "to": "text", "story": 7 }));
    assert_eq!(story_fill(&session, 0), Some(Paint::Solid { colour: RED }));
    run(&mut session, "swatch.delete", json!({ "id": id }));
    assert!(session.document.swatch_bindings.is_empty());
    assert_eq!(story_fill(&session, 0), Some(Paint::Solid { colour: RED }));
    assert!(session.run(&Command::with_params("swatch.apply", json!({ "id": 99, "to": "text", "story": 7 }))).is_err());
    let text = serde_json::to_string(&session.document).unwrap();
    let loaded: Document = serde_json::from_str(&text).unwrap();
    assert!(loaded.swatch_bindings.is_empty());
}

#[test]
fn old_bindings_without_new_fields_load() {
    let value = json!({ "object": 1, "target": "Fill", "swatch": 3 });
    let binding: dtool_core::colour::swatch::SwatchBinding = serde_json::from_value(value).unwrap();
    assert_eq!(binding.stop, None);
    assert_eq!(binding.story, None);
}