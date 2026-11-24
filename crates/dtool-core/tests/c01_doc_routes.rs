use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session};
use serde_json::json;

#[test]
fn doc_units_set_routes_with_one_undo() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    let generation = s.generation;
    s.run(&Command::with_params("doc.units.set", json!({"units": "Mm"}))).unwrap();
    assert_eq!(s.document.units, Units::Mm);
    assert_eq!(s.generation, generation + 1);
    s.run(&Command::with_params("edit.undo", json!({}))).unwrap();
    assert_eq!(s.document.units, Units::Px);
    assert!(s.run(&Command::with_params("doc.units.set", json!({"units": "Furlong"}))).is_err());
}

#[test]
fn doc_scale_set_rejects_bad_scale_and_undoes() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    let before = s.document.drawing_scale;
    assert!(s.run(&Command::with_params("doc.scale.set", json!({"drawing_scale": [0.0, 1.0]}))).is_err());
    assert_eq!(s.document.drawing_scale, before);
    s.run(&Command::with_params("doc.scale.set", json!({"drawing_scale": [1.0, 100.0]}))).unwrap();
    assert_eq!(s.document.drawing_scale, [1.0, 100.0]);
    s.run(&Command::with_params("edit.undo", json!({}))).unwrap();
    assert_eq!(s.document.drawing_scale, before);
}

#[test]
fn shortcut_commands_without_registered_action_are_not_available() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    assert!(s.run(&Command::with_params("ui.shortcuts.list", json!({}))).is_err());
}