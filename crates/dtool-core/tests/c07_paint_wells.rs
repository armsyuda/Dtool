use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint, StrokeStyle};
use dtool_core::selection::SelectOp;
use dtool_core::{Object, ObjectId, Session};
use serde_json::json;

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: Colour::new(r, g, b, 255) }
}

fn session_with_two() -> (Session, ObjectId, ObjectId) {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    let a = s.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], fill: solid(255, 0, 0), stroke: StrokeStyle { paint: solid(0, 0, 255), width: 1.0, ..StrokeStyle::default() }, ..Object::default() });
    let b = s.document.create_object(Object { name: "b".into(), size: [10.0, 10.0], fill: solid(0, 255, 0), ..Object::default() });
    s.selection.set(vec![a, b], SelectOp::Replace);
    (s, a, b)
}

#[test]
fn c07_swap_keeps_each_object_pair_and_undoes_once() {
    let (mut s, a, b) = session_with_two();
    let depth = s.undo.applied_steps();
    s.run_named("paint.swap", json!({})).unwrap();
    assert_eq!(s.document.get_object(a).unwrap().fill, solid(0, 0, 255));
    assert_eq!(s.document.get_object(a).unwrap().stroke.paint, solid(255, 0, 0));
    assert_eq!(s.document.get_object(b).unwrap().fill, Paint::None);
    assert_eq!(s.document.get_object(b).unwrap().stroke.paint, solid(0, 255, 0));
    assert_eq!(s.undo.applied_steps(), depth + 1);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.document.get_object(a).unwrap().fill, solid(255, 0, 0));
    assert_eq!(s.document.get_object(b).unwrap().fill, solid(0, 255, 0));
    s.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(s.document.get_object(b).unwrap().stroke.paint, solid(0, 255, 0));
}

#[test]
fn c07_default_applies_app_defaults() {
    let (mut s, a, _) = session_with_two();
    let defaults = dtool_core::commands::defaults::current();
    s.run_named("paint.default", json!({"ids": [a.0]})).unwrap();
    let object = s.document.get_object(a).unwrap();
    assert_eq!(object.fill, defaults.fill);
    assert_eq!(object.stroke, defaults.stroke_for(96.0));
    assert!((object.stroke.width - defaults.stroke.width * 96.0 / 72.0).abs() < 1e-4);
    let factory = dtool_core::ObjectDefaults::default();
    assert_eq!(factory.fill, Paint::None);
    assert_eq!((factory.stroke.paint, factory.stroke.width), (solid(0, 0, 0), 1.0));
}

#[test]
fn c07_none_clears_only_the_named_role() {
    let (mut s, a, b) = session_with_two();
    s.run_named("paint.none", json!({"role": "fill"})).unwrap();
    assert_eq!(s.document.get_object(a).unwrap().fill, Paint::None);
    assert_eq!(s.document.get_object(a).unwrap().stroke.paint, solid(0, 0, 255));
    assert_eq!(s.document.get_object(b).unwrap().fill, Paint::None);
    s.run_named("paint.none", json!({"object_id": a.0, "role": "stroke"})).unwrap();
    assert_eq!(s.document.get_object(a).unwrap().stroke.paint, Paint::None);
    assert!(s.run_named("paint.none", json!({})).is_err());
    assert!(s.run_named("paint.none", json!({"role": "text_stroke"})).is_err());
    assert!(s.run_named("paint.swap", json!({"role": "fill"})).is_err());
}

#[test]
fn c07_none_text_fill_clears_story_ranges_with_one_undo() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    let id = s.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "ab"})).unwrap()["id"].as_u64().unwrap();
    let story = s.document.stories.last().unwrap().id;
    s.run_named("text.edit.end", json!({})).ok();
    let before = s.document.stories.iter().find(|item| item.id == story).unwrap().clone();
    s.run_named("paint.none", json!({"ids": [id], "role": "text_fill"})).unwrap();
    let after = s.document.stories.iter().find(|item| item.id == story).unwrap().clone();
    assert!(after.ranges.iter().all(|range| dtool_core::paint::range_fill(range).is_none()));
    if before != after {
        s.run_named("edit.undo", json!({})).unwrap();
        assert_eq!(s.document.stories.iter().find(|item| item.id == story).unwrap(), &before);
    }
}

fn text_session() -> (Session, u64, dtool_core::StoryId) {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    let id = s.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "abcd"})).unwrap()["id"].as_u64().unwrap();
    let story = s.document.stories.last().unwrap().id;
    s.run_named("text.edit.end", json!({})).ok();
    (s, id, story)
}

fn has_outline(s: &Session, story: dtool_core::StoryId) -> bool {
    s.document.stories.iter().find(|item| item.id == story).unwrap().ranges.iter().any(|range| range.get("outline").is_some_and(|value| !value.is_null()))
}

#[test]
fn c07_none_text_stroke_clears_outline_with_one_undo() {
    let (mut s, id, story) = text_session();
    s.run_named("text.style", json!({"story": story.0, "range": {"start": 0, "end": 4}, "patch": {"outline": ["#ff0000", 2.0]}})).unwrap();
    assert!(has_outline(&s, story));
    let before = s.document.stories.iter().find(|item| item.id == story).unwrap().clone();
    let depth = s.undo.applied_steps();
    let version = s.document.version;
    s.run_named("paint.none", json!({"ids": [id], "role": "text_stroke"})).unwrap();
    assert!(!has_outline(&s, story));
    assert_eq!(s.undo.applied_steps(), depth + 1);
    assert_eq!(s.document.version, version + 1);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.document.stories.iter().find(|item| item.id == story).unwrap(), &before);
    assert!(has_outline(&s, story));
    s.run_named("edit.redo", json!({})).unwrap();
    assert!(!has_outline(&s, story));
}

#[test]
fn c07_none_text_stroke_without_outline_is_a_no_op() {
    let (mut s, id, story) = text_session();
    assert!(!has_outline(&s, story));
    let depth = s.undo.applied_steps();
    let version = s.document.version;
    s.run_named("paint.none", json!({"ids": [id], "role": "text_stroke"})).unwrap();
    assert_eq!(s.undo.applied_steps(), depth);
    assert_eq!(s.document.version, version);
}

#[test]
fn c07_none_text_stroke_rejects_shapes_missing_and_mixed_targets() {
    let (mut s, id, _) = text_session();
    let shape = s.document.create_object(Object { name: "shape".into(), size: [10.0, 10.0], fill: solid(255, 0, 0), ..Object::default() });
    let version = s.document.version;
    for ids in [json!([shape.0]), json!([id, shape.0]), json!([9999])] {
        let err = s.run_named("paint.none", json!({"ids": ids, "role": "text_stroke"})).unwrap_err();
        assert_eq!(err.code, "INVALID_PARAMS");
        let err = s.run_named("paint.none", json!({"ids": ids, "role": "text_fill"})).unwrap_err();
        assert_eq!(err.code, "INVALID_PARAMS");
    }
    assert_eq!(s.document.version, version);
    assert!(s.run_named("paint.swap", json!({"ids": [id], "role": "text_stroke"})).is_err());
}
