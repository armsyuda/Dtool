use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session};
use serde_json::json;

#[test]
fn quick_grid_sets_the_document_grid_in_one_undo_step() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    let before = s.document.grid.clone();
    let r = s.run(&Command::with_params("grid.quick.set", json!({"preset": "isometric", "divisions": 3}))).unwrap();
    assert_eq!(r["grid"]["kind"], json!("Isometric"));
    assert_ne!(s.document.grid, before);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.document.grid, before);
    assert!(s.run(&Command::with_params("grid.quick.set", json!({"preset": "hex"}))).is_err());
    assert_eq!(s.document.grid, before);
}

#[test]
fn object_update_sets_transparency() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    let id = doc.create_object(dtool_core::Object { name: "t".into(), pos: [0.0, 0.0], size: [10.0, 10.0], ..dtool_core::Object::default() });
    let args: dtool_core::object::ObjectUpdateArgs = serde_json::from_value(json!({"id": id.0})).unwrap();
    assert!(args.transparency.is_none());
    dtool_core::command::execute_object_update(&mut doc, args).unwrap();
    assert_eq!(doc.get_object(id).unwrap().transparency, dtool_core::paint::Transparency::None);
}