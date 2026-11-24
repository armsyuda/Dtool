use dtool_core::command::Session;
use dtool_core::commands::colour_pick;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::Colour;
use dtool_protocol::Command;
use serde_json::{json, Value};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

fn two_colour_image() -> Vec<u8> {
    let mut pixels = Vec::new();
    for _ in 0..10 {
        for x in 0..20 {
            pixels.extend_from_slice(if x < 10 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
        }
    }
    pixels
}

fn session() -> (Session, ObjectId, ObjectId) {
    let path = std::env::temp_dir().join(format!("c07_pick_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    dtool_core::palette::set_app_path(Some(path));
    let mut session = Session::new(Document::new([100.0, 100.0], Units::Px, 72.0, 1));
    let image = session.document.create_object(Object { name: "image".into(), pos: [0.0, 0.0], size: [20.0, 10.0], raster_data: Some(two_colour_image()), raster_size: Some((20, 10)), ..Object::default() });
    let target = session.document.create_object(Object { name: "target".into(), pos: [60.0, 60.0], size: [10.0, 10.0], ..Object::default() });
    (session, image, target)
}

fn call(session: &mut Session, name: &str, params: Value) -> Value {
    colour_pick::run(session, &Command::with_params(name, params)).expect("handled").expect("ok")
}

fn rgba(reply: &Value) -> Vec<i64> {
    reply["colour"]["rgba"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect()
}

fn near(actual: &[i64], expected: [i64; 4]) -> bool {
    actual.iter().zip(expected.iter()).all(|(a, e)| (a - e).abs() <= 2)
}

#[test]
fn c07_pick_point_and_average3_at_boundary() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut s, _, _) = session();
    let red = call(&mut s, "color.pick", json!({ "x": 5.5, "y": 5.5, "mode": "point", "apply": false }));
    assert!(near(&rgba(&red), [255, 0, 0, 255]), "{red}");
    let blue = call(&mut s, "color.pick", json!({ "x": 15.5, "y": 5.5, "apply": false }));
    assert!(near(&rgba(&blue), [0, 0, 255, 255]), "{blue}");
    let mixed = call(&mut s, "color.pick", json!({ "x": 10.5, "y": 5.5, "mode": "average3", "apply": false }));
    assert!(near(&rgba(&mixed), [85, 0, 170, 255]), "{mixed}");
    assert_eq!(mixed["model"], "rgb");
    assert_eq!(mixed["applied"], false);
}

#[test]
fn c07_pick_layer_ignores_objects_above_and_merged_does_not() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut s, image, _) = session();
    let green: Vec<u8> = [0, 255, 0, 255].repeat(100);
    s.document.create_object(Object { name: "cover".into(), pos: [0.0, 0.0], size: [10.0, 10.0], raster_data: Some(green), raster_size: Some((10, 10)), ..Object::default() });
    let merged = call(&mut s, "color.pick", json!({ "x": 5.5, "y": 5.5, "mode": "merged", "apply": false }));
    assert!(near(&rgba(&merged), [0, 255, 0, 255]), "{merged}");
    let layer = call(&mut s, "color.pick", json!({ "x": 5.5, "y": 5.5, "layer": image.0, "apply": false }));
    assert!(near(&rgba(&layer), [255, 0, 0, 255]), "{layer}");
}

#[test]
fn c07_pick_writes_fill_with_one_undo_and_records_recent() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut s, _, target) = session();
    s.run(&Command::with_params("selection.set", json!({ "ids": [target.0] }))).unwrap();
    let before = s.document.get_object(target).unwrap().fill.clone();
    call(&mut s, "ui.color.target.set", json!({ "target": "fill" }));
    let reply = call(&mut s, "color.pick", json!({ "x": 15.5, "y": 5.5 }));
    assert_eq!(reply["applied"], true);
    let after = s.document.get_object(target).unwrap().fill.clone();
    assert_ne!(after, before);
    assert!(format!("{after:?}").contains("b: 255"), "{after:?}");
    s.run(&Command::with_params("edit.undo", json!({}))).unwrap();
    assert_eq!(s.document.get_object(target).unwrap().fill, before);
    let recent = call(&mut s, "color.recent.list", json!({}));
    assert_eq!(recent["recent"][0]["hex"], "#0000ff");
}

#[test]
fn c07_pick_text_target_writes_only_story_range() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut s, _, _) = session();
    let story = StoryId(70);
    s.document.stories.push(Story { id: story, text: "abcdef".into(), ranges: vec![json!({ "start": 0, "end": 6, "size_pt": 12.0 })], specials: Vec::new(), paras: Vec::new() });
    s.document.create_object(Object { kind: ObjectKind::FrameText { story }, pos: [40.0, 40.0], size: [50.0, 20.0], ..Object::default() });
    call(&mut s, "ui.color.target.set", json!({ "target": "text_fill", "story": story.0, "range": { "start": 2, "end": 4 } }));
    let reply = call(&mut s, "color.pick", json!({ "x": 5.5, "y": 5.5 }));
    assert_eq!(reply["applied"], true);
    let ranges = s.document.stories.iter().find(|item| item.id == story).unwrap().ranges.clone();
    for range in &ranges {
        let start = range["start"].as_u64().unwrap();
        let end = range["end"].as_u64().unwrap();
        if start >= 2 && end <= 4 {
            assert!(range.get("fill").is_some(), "{ranges:?}");
        } else {
            assert!(range.get("fill").is_none(), "{ranges:?}");
        }
    }
    assert!(ranges.iter().any(|range| range.get("fill").is_some()));
    let outline = call(&mut s, "color.pick", json!({ "x": 15.5, "y": 5.5, "target": "text_stroke" }));
    assert_eq!(outline["target"]["target"], "text_stroke");
    let ranges = s.document.stories.iter().find(|item| item.id == story).unwrap().ranges.clone();
    assert!(ranges.iter().all(|range| range.get("outline").is_none() || (range["start"].as_u64().unwrap() >= 2 && range["end"].as_u64().unwrap() <= 4)), "{ranges:?}");
    assert!(colour_pick::run(&mut s, &Command::with_params("ui.color.target.set", json!({ "target": "text_fill" }))).unwrap().is_err());
    call(&mut s, "ui.color.target.set", json!({ "target": "fill" }));
}

#[test]
fn c07_contrast_and_recent_ring() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut s, _, _) = session();
    let reply = call(&mut s, "color.contrast", json!({ "a": "#000000", "b": "#FFFFFF" }));
    assert!((reply["ratio"].as_f64().unwrap() - 21.0).abs() < 0.1, "{reply}");
    let mut ring = Vec::new();
    for value in 0..20u8 {
        dtool_core::palette::push_recent_into(&mut ring, Colour::new(value, 0, 0, 255));
    }
    dtool_core::palette::push_recent_into(&mut ring, Colour::new(10, 0, 0, 255));
    assert_eq!(ring.len(), dtool_core::palette::RECENT_COLOURS_MAX);
    assert_eq!(ring[0].r, 10);
    assert_eq!(ring.iter().filter(|c| c.r == 10).count(), 1);
    assert_eq!(ring[1].r, 19);
}
#[test]
fn c07_pick_stroke_on_two_objects_is_one_undo() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut s, _, target) = session();
    let other = s.document.create_object(Object { name: "other".into(), pos: [80.0, 60.0], size: [10.0, 10.0], ..Object::default() });
    s.run(&Command::with_params("selection.set", json!({ "ids": [target.0, other.0] }))).unwrap();
    let before = (s.document.get_object(target).unwrap().stroke.clone(), s.document.get_object(other).unwrap().stroke.clone());
    let reply = call(&mut s, "color.pick", json!({ "x": 5.5, "y": 5.5, "target": "stroke" }));
    assert_eq!(reply["applied"], true);
    for id in [target, other] {
        let stroke = s.document.get_object(id).unwrap().stroke.clone();
        assert!(format!("{:?}", stroke.paint).contains("r: 255"), "{stroke:?}");
        assert_eq!(stroke.width, before.0.width);
    }
    s.run(&Command::with_params("edit.undo", json!({}))).unwrap();
    assert_eq!(s.document.get_object(target).unwrap().stroke, before.0);
    assert_eq!(s.document.get_object(other).unwrap().stroke, before.1);
}

#[test]
fn c07_pick_zoom2_average3_covers_smaller_window() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut s, _, _) = session();
    let implicit = call(&mut s, "color.pick", json!({ "x": 9.0, "y": 5.5, "mode": "average3", "apply": false }));
    assert!(near(&rgba(&implicit), [170, 0, 85, 255]), "{implicit}");
    let zoomed = call(&mut s, "color.pick", json!({ "x": 9.0, "y": 5.5, "mode": "average3", "zoom": 2.0, "apply": false }));
    assert!(near(&rgba(&zoomed), [255, 0, 0, 255]), "{zoomed}");
    s.view.zoom = 2.0;
    let from_view = call(&mut s, "color.pick", json!({ "x": 9.0, "y": 5.5, "mode": "average3", "apply": false }));
    assert!(near(&rgba(&from_view), [255, 0, 0, 255]), "{from_view}");
    let mixed = call(&mut s, "color.pick", json!({ "x": 10.5, "y": 5.5, "mode": "average3", "zoom": 2.0, "apply": false }));
    assert!(near(&rgba(&mixed), [0, 0, 255, 255]), "{mixed}");
}

#[test]
fn c07_colour_target_lives_in_view_state() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut a, _, _) = session();
    let (b, _, _) = session();
    call(&mut a, "ui.color.target.set", json!({ "target": "stroke" }));
    assert_eq!(a.view.colour_target.target, dtool_core::view_state::ColourTarget::Stroke);
    assert_eq!(b.view.colour_target.target, dtool_core::view_state::ColourTarget::Fill);
    assert_eq!(call(&mut a, "ui.color.target.query", json!({}))["target"], "stroke");
    let saved = serde_json::to_value(&a.view).unwrap();
    let restored: dtool_core::ViewState = serde_json::from_value(saved).unwrap();
    assert_eq!(restored.colour_target, a.view.colour_target);
    let mut legacy = serde_json::to_value(&b.view).unwrap();
    legacy.as_object_mut().unwrap().remove("colour_target");
    let old: dtool_core::ViewState = serde_json::from_value(legacy).unwrap();
    assert_eq!(old.colour_target, Default::default());
    let reply = call(&mut a, "color.pick", json!({ "x": 5.5, "y": 5.5, "target": "fill", "apply": false }));
    assert_eq!(reply["target"]["target"], "fill");
    assert_eq!(a.view.colour_target.target, dtool_core::view_state::ColourTarget::Stroke);
}
