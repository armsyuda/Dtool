use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint, StrokeStyle};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn session() -> (Session, ObjectId, ObjectId) {
    let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
    let source = session.document.create_object(Object {
        size: [20.0, 20.0],
        opacity: 0.5,
        fill: Paint::Solid { colour: Colour::new(200, 10, 20, 255) },
        stroke: StrokeStyle { paint: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, width: 4.0, ..StrokeStyle::default() },
        ..Object::default()
    });
    let target = session.document.create_object(Object { size: [20.0, 20.0], ..Object::default() });
    (session, source, target)
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|e| panic!("{name}: {}", e.code))
}

fn undo(session: &mut Session) {
    session.run(&Command::new("edit.undo")).expect("undo");
}

#[test]
fn create_from_selection_keeps_fill_stroke_opacity_as_one_undo_step() {
    let (mut session, source, _) = session();
    session.selection.ids = vec![source];
    let generation = session.generation;
    let reply = run(&mut session, "style.create", json!({ "name": "Red" }));
    assert_eq!(reply["style_id"], json!(1));
    assert_eq!(session.generation, generation + 1);
    let listed = run(&mut session, "style.list", json!({}));
    let styles = listed["styles"].as_array().unwrap();
    assert_eq!(styles.len(), 1);
    assert_eq!(styles[0]["name"], "Red");
    let object = session.document.get_object(source).unwrap();
    assert_eq!(styles[0]["style"]["fill"], serde_json::to_value(&object.fill).unwrap());
    assert_eq!(styles[0]["style"]["stroke"], serde_json::to_value(&object.stroke).unwrap());
    assert_eq!(styles[0]["style"]["opacity"], json!(0.5));
    assert!(styles[0]["style"].get("effects").is_some());
    let second = run(&mut session, "style.create", json!({ "id": source.0 }));
    assert_eq!((second["style_id"].clone(), second["name"].clone()), (json!(2), json!("Style 2")));
    undo(&mut session);
    assert_eq!(run(&mut session, "style.list", json!({}))["styles"].as_array().unwrap().len(), 1);
    undo(&mut session);
    assert!(run(&mut session, "style.list", json!({}))["styles"].as_array().unwrap().is_empty());
}

#[test]
fn apply_by_style_id_copies_the_stored_appearance_and_undoes() {
    let (mut session, source, target) = session();
    let id = run(&mut session, "style.create", json!({ "id": source.0, "name": "Red" }))["style_id"].clone();
    let reply = run(&mut session, "style.apply", json!({ "style_id": id, "ids": [target.0] }));
    assert_eq!(reply["applied_count"], json!(1));
    let (from, to) = (session.document.get_object(source).unwrap().clone(), session.document.get_object(target).unwrap().clone());
    assert_eq!((to.fill.clone(), to.stroke.clone(), to.opacity), (from.fill.clone(), from.stroke.clone(), from.opacity));
    undo(&mut session);
    assert_eq!(session.document.get_object(target).unwrap().opacity, 1.0);
    let missing = session.run(&Command::with_params("style.apply", json!({ "style_id": 99, "ids": [target.0] })));
    assert!(missing.is_err());
}

#[test]
fn rename_and_delete_change_the_store_and_reject_bad_input_without_change() {
    let (mut session, source, _) = session();
    let id = run(&mut session, "style.create", json!({ "id": source.0, "name": "A" }))["style_id"].clone();
    let renamed = run(&mut session, "style.rename", json!({ "style_id": id, "name": "B" }));
    assert_eq!(renamed["name"], "B");
    assert_eq!(run(&mut session, "style.list", json!({}))["styles"][0]["name"], "B");
    let version = session.document.version;
    assert!(session.run(&Command::with_params("style.rename", json!({ "style_id": id, "name": "  " }))).is_err());
    assert!(session.run(&Command::with_params("style.rename", json!({ "style_id": 42, "name": "C" }))).is_err());
    assert!(session.run(&Command::with_params("style.delete", json!({ "style_id": 42 }))).is_err());
    assert!(session.run(&Command::with_params("style.create", json!({ "id": 9999 }))).is_err());
    assert_eq!(session.document.version, version);
    run(&mut session, "style.delete", json!({ "style_id": id }));
    assert!(run(&mut session, "style.list", json!({}))["styles"].as_array().unwrap().is_empty());
    undo(&mut session);
    assert_eq!(run(&mut session, "style.list", json!({}))["styles"][0]["name"], "B");
}

#[test]
fn styles_survive_document_save_and_load() {
    let (mut session, source, _) = session();
    run(&mut session, "style.create", json!({ "id": source.0, "name": "Keep" }));
    let saved = serde_json::to_value(&session.document).unwrap();
    let loaded: Document = serde_json::from_value(saved).unwrap();
    let mut reopened = Session::new(loaded);
    assert_eq!(run(&mut reopened, "style.list", json!({}))["styles"][0]["name"], "Keep");
}

#[test]
fn create_keeps_an_optional_category_and_rejects_a_non_string_one() {
    let (mut session, source, _) = session();
    session.selection.ids = vec![source];
    run(&mut session, "style.create", json!({ "name": "Red", "category": " Brand " }));
    run(&mut session, "style.create", json!({ "name": "Plain", "category": "" }));
    let styles = run(&mut session, "style.list", json!({}))["styles"].as_array().unwrap().clone();
    assert_eq!(styles[0]["category"], json!("Brand"));
    assert!(styles[1].get("category").is_none());
    let error = session.run(&Command::with_params("style.create", json!({ "category": 3 }))).unwrap_err();
    assert_eq!(error.code, "INVALID_PARAMS");
    assert_eq!(run(&mut session, "style.list", json!({}))["styles"].as_array().unwrap().len(), 2);
}