use dtool_core::colour::swatch::SwatchTarget;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

const RED: Colour = Colour::new(255, 0, 0, 255);
const BLUE: Colour = Colour::new(0, 0, 255, 255);

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn solid(colour: Colour) -> Paint {
    Paint::Solid { colour }
}

fn session() -> Session {
    let mut document = Document::new([400.0, 300.0], Units::Pt, 72.0, 1);
    for id in [1, 2] {
        document.objects.push(Object { id: ObjectId(id), fill: solid(RED), ..Object::default() });
    }
    Session::new(document)
}

fn global_red(session: &mut Session) -> u64 {
    run(session, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "global": true }))["id"].as_u64().unwrap()
}

#[test]
fn stable_global_binding_follows_edit_and_undo() {
    let mut session = session();
    let id = global_red(&mut session);
    run(&mut session, "swatch.apply_to_selection", json!({ "swatch": id.to_string(), "to": "fill", "target": ["node:1"] }));
    assert_eq!(session.document.swatch_bindings.len(), 1);
    assert_eq!(session.document.swatch_bindings[0].target, SwatchTarget::Fill);
    run(&mut session, "swatch.rename", json!({ "id": id, "name": "Brand" }));
    let reply = run(&mut session, "swatch.edit", json!({ "id": id, "colour": "#0000FF" }));
    assert_eq!(reply["updated"], json!([1]));
    assert_eq!(session.document.objects[0].fill, solid(BLUE));
    assert_eq!(session.document.objects[1].fill, solid(RED));
    let text = serde_json::to_string(&session.document).unwrap();
    let loaded: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(loaded.swatch_bindings, session.document.swatch_bindings);
    assert_eq!(loaded.swatches[0].id, id);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.objects[0].fill, solid(RED));
    assert_eq!(session.document.swatches[0].colour, RED);
    run(&mut session, "edit.redo", json!({}));
    assert_eq!(session.document.objects[0].fill, solid(BLUE));
}

#[test]
fn non_global_apply_does_not_bind_and_replaces_binding() {
    let mut session = session();
    let global = global_red(&mut session);
    let local = run(&mut session, "swatch.add", json!({ "name": "Green", "colour": "#00FF00" }))["id"].as_u64().unwrap();
    run(&mut session, "swatch.apply", json!({ "id": global, "ids": [1, 2] }));
    assert_eq!(session.document.swatch_bindings.len(), 2);
    run(&mut session, "swatch.apply", json!({ "id": local, "ids": [2] }));
    assert_eq!(session.document.swatch_bindings.len(), 1);
    run(&mut session, "swatch.edit", json!({ "id": global, "colour": "#0000FF" }));
    assert_eq!(session.document.objects[0].fill, solid(BLUE));
    assert_eq!(session.document.objects[1].fill, solid(Colour::new(0, 255, 0, 255)));
}

#[test]
fn repainted_object_is_not_overwritten() {
    let mut session = session();
    let id = global_red(&mut session);
    run(&mut session, "swatch.apply", json!({ "id": id, "ids": [1] }));
    session.document.objects[0].fill = solid(Colour::new(1, 2, 3, 255));
    let reply = run(&mut session, "swatch.edit", json!({ "id": id, "colour": "#0000FF" }));
    assert_eq!(reply["updated"], json!([]));
    assert_eq!(session.document.objects[0].fill, solid(Colour::new(1, 2, 3, 255)));
}

#[test]
fn delete_and_make_local_detach_cleanly() {
    let mut session = session();
    let id = global_red(&mut session);
    run(&mut session, "swatch.apply", json!({ "id": id, "ids": [1], "to": "stroke" }));
    assert_eq!(session.document.swatch_bindings[0].target, SwatchTarget::Stroke);
    run(&mut session, "swatch.make_global", json!({ "id": id, "global": false }));
    assert!(session.document.swatch_bindings.is_empty());
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.swatch_bindings.len(), 1);
    run(&mut session, "swatch.delete", json!({ "id": id }));
    assert!(session.document.swatch_bindings.is_empty());
    assert_eq!(session.document.objects[0].stroke.paint, solid(RED));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.swatch_bindings.len(), 1);
    assert_eq!(session.document.swatches.len(), 1);
}

#[test]
fn add_from_selection_reads_solid_fill() {
    let mut session = session();
    session.selection.ids = vec![ObjectId(2)];
    let id = run(&mut session, "swatch.add_from_selection", json!({ "global": true }))["id"].as_u64().unwrap();
    assert_eq!(session.document.swatches[0].colour, RED);
    assert_eq!(session.document.swatch_bindings[0].object, ObjectId(2));
    run(&mut session, "swatch.edit", json!({ "id": id, "colour": "#0000FF" }));
    assert_eq!(session.document.objects[1].fill, solid(BLUE));
    session.selection.ids.clear();
    assert!(session.run(&Command::with_params("swatch.add_from_selection", json!({}))).is_err());
}