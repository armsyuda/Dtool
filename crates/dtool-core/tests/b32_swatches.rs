use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn session() -> Session {
    let mut document = Document::new([400.0, 300.0], Units::Pt, 72.0, 1);
    document.colour.profile = "sRGB IEC61966-2.1".to_string();
    for id in [1, 2] {
        document.objects.push(Object { id: ObjectId(id), fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    }
    Session::new(document)
}

#[test]
fn old_documents_load_with_empty_swatches() {
    let document = Document::new([100.0, 100.0], Units::Pt, 72.0, 1);
    let mut value = serde_json::to_value(&document).unwrap();
    let map = value.as_object_mut().unwrap();
    map.remove("swatches");
    map.remove("next_swatch_id");
    let loaded: Document = serde_json::from_value(value).unwrap();
    assert!(loaded.swatches.is_empty());
}

#[test]
fn add_rename_edit_delete_each_undo_once() {
    let mut session = session();
    let reply = run(&mut session, "swatch.add", json!({ "name": "Doc red", "color": "#FF0000", "scope": "document", "global": true }));
    let id = reply["id"].as_u64().unwrap();
    assert_eq!(session.document.swatches.len(), 1);
    assert!(session.document.swatches[0].global);
    assert_eq!(session.document.swatches[0].colour, Colour::new(255, 0, 0, 255));
    run(&mut session, "swatch.rename", json!({ "id": id, "name": "Brand red" }));
    assert_eq!(session.document.swatches[0].name, "Brand red");
    run(&mut session, "swatch.edit", json!({ "id": id, "colour": "#0000FF" }));
    assert_eq!(session.document.swatches[0].colour, Colour::new(0, 0, 255, 255));
    run(&mut session, "swatch.make_global", json!({ "id": id, "global": false }));
    assert!(!session.document.swatches[0].global);
    run(&mut session, "swatch.delete", json!({ "id": id }));
    assert!(session.document.swatches.is_empty());
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.swatches[0].id, id);
    run(&mut session, "edit.undo", json!({}));
    assert!(session.document.swatches[0].global);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.swatches[0].colour, Colour::new(255, 0, 0, 255));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.swatches[0].name, "Doc red");
    run(&mut session, "edit.undo", json!({}));
    assert!(session.document.swatches.is_empty());
    run(&mut session, "edit.redo", json!({}));
    assert_eq!(session.document.swatches[0].id, id);
}

#[test]
fn ids_stay_stable_after_delete_and_through_save() {
    let mut session = session();
    let first = run(&mut session, "swatch.add", json!({ "colour": "#112233" }))["id"].as_u64().unwrap();
    let second = run(&mut session, "swatch.add", json!({ "colour": "#445566" }))["id"].as_u64().unwrap();
    assert_eq!(session.document.swatches[0].name, "#112233");
    run(&mut session, "swatch.delete", json!({ "id": second }));
    let third = run(&mut session, "swatch.add", json!({ "colour": "#778899" }))["id"].as_u64().unwrap();
    assert!(third > second && second > first);
    let text = serde_json::to_string(&session.document).unwrap();
    let loaded: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(loaded.swatches, session.document.swatches);
    assert_eq!(loaded.next_swatch_id, session.document.next_swatch_id);
}

#[test]
fn apply_to_selection_sets_only_targets_and_undoes() {
    let mut session = session();
    let id = run(&mut session, "swatch.add", json!({ "name": "Blue", "colour": "#0000FF", "global": true }))["id"].as_u64().unwrap();
    run(&mut session, "swatch.apply_to_selection", json!({ "swatch": id.to_string(), "to": "fill", "target": ["node:1"] }));
    let blue = Paint::Solid { colour: Colour::new(0, 0, 255, 255) };
    let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
    assert_eq!(session.document.objects[0].fill, blue);
    assert_eq!(session.document.objects[1].fill, red);
    run(&mut session, "swatch.apply", json!({ "id": id, "to": "stroke", "ids": [2] }));
    assert_eq!(session.document.objects[1].stroke.paint, blue);
    run(&mut session, "edit.undo", json!({}));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.objects[0].fill, red);
    assert_eq!(session.document.swatches.len(), 1);
}

#[test]
fn bad_swatch_params_fail() {
    let mut session = session();
    assert_eq!(session.run(&Command::with_params("swatch.rename", json!({ "id": 9, "name": "x" }))).unwrap_err().code, "SWATCH_NOT_FOUND");
    assert!(session.run(&Command::with_params("swatch.add", json!({ "colour": "nope" }))).is_err());
    assert!(session.run(&Command::with_params("swatch.spot.add", json!({ "colour": "#FF0000" }))).is_err());
    assert_eq!(session.run(&Command::with_params("swatch.add", json!({ "colour": "#FF0000", "scope": "system" }))).unwrap_err().code, "NOT_IMPLEMENTED");
    assert!(!session.undo.can_undo());
}

#[test]
fn spot_add_and_list() {
    let mut session = session();
    run(&mut session, "swatch.spot.add", json!({ "name": "PANTONE 185 C", "colour": "#E4002B" }));
    let listed = run(&mut session, "swatch.list", json!({}));
    assert_eq!(listed["swatches"][0]["name"], "PANTONE 185 C");
    assert_eq!(listed["swatches"][0]["spot"], true);
    run(&mut session, "swatch.update", json!({ "id": listed["swatches"][0]["id"], "overprint": true }));
    assert!(session.document.swatches[0].overprint);
}

#[test]
fn profile_convert_converts_swatches_in_same_undo_step() {
    let mut session = session();
    run(&mut session, "swatch.add", json!({ "name": "Orange", "colour": "#B45A2D" }));
    let before = serde_json::to_value(&session.document.swatches).unwrap();
    run(&mut session, "color.profile.convert", json!({ "format": dtool_core::colour::ColourFormat::LAB_16.to_string(), "profile": "LAB D50" }));
    assert_eq!(session.document.colour.profile, "LAB D50");
    assert_ne!(serde_json::to_value(&session.document.swatches).unwrap(), before);
    assert_ne!(session.document.swatches[0].colour.model, dtool_core::colour::ColourModel::Rgb);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(serde_json::to_value(&session.document.swatches).unwrap(), before);
    assert_eq!(session.document.colour.profile, "sRGB IEC61966-2.1");
    run(&mut session, "edit.undo", json!({}));
    assert!(session.document.swatches.is_empty());
}