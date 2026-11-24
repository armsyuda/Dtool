use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session};
use serde_json::json;

#[test]
fn text_wrap_ignore_round_trips_through_undo() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    let created = s.run(&Command::with_params("object.create", json!({"pos": [0.0, 0.0], "size": [10.0, 10.0]}))).unwrap();
    let id = s.document.objects.last().map(|o| o.id.0).or_else(|| created["id"].as_u64()).unwrap();
    s.run(&Command::with_params("object.text_wrap.ignore", json!({"id": id, "on": true}))).unwrap();
    let q = s.run(&Command::with_params("object.text_wrap.query", json!({"id": id}))).unwrap();
    assert_eq!(q["ignore"], json!(true));
    s.run(&Command::new("edit.undo")).unwrap();
    let q = s.run(&Command::with_params("object.text_wrap.query", json!({"id": id}))).unwrap();
    assert_eq!(q["ignore"], json!(false));
    assert!(s.run(&Command::with_params("object.text_wrap.query", json!({"id": 999999}))).is_err());
}

#[test]
fn hub_ask_rows_are_catalogued() {
    let catalog = dtool_protocol::CommandCatalog::default();
    for name in ["brush.vector.stroke", "blend.expand", "vector.blob.erase", "path.points.align", "object.text_wrap.set", "perspective.apply"] {
        assert!(catalog.get(name).is_some_and(|spec| spec.implemented), "{name}");
    }
    assert!(catalog.get("text.style.define").is_some_and(|spec| spec.implemented));
}