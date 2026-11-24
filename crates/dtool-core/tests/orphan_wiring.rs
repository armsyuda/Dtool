use dtool_core::command::Session;
use dtool_core::document::Document;
use dtool_core::paint::PaintOrder;
use dtool_core::shape::ShapeKind;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::default())
}

#[test]
fn shape_add_creates_object_and_one_undo_removes_it() {
    let mut s = session();
    let before = s.document.objects.len();
    let out = s.run_named("shape.add", json!({"kind": ShapeKind::Rectangle, "size": [40.0, 20.0], "pos": [5.0, 5.0]})).unwrap();
    let id = out["id"].as_u64().unwrap();
    assert_eq!(s.document.objects.len(), before + 1);
    let query = s.run_named("shape.params.query", json!({"id": id})).unwrap();
    assert_eq!(query["size"], json!([40.0, 20.0]));
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.document.objects.len(), before);
    s.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(s.document.objects.len(), before + 1);
}

#[test]
fn shape_add_triangle_defaults_points() {
    let mut s = session();
    let out = s.run_named("shape.add", json!({"kind": ShapeKind::Triangle, "size": [10.0, 10.0]})).unwrap();
    let query = s.run_named("shape.params.query", json!({"id": out["id"]})).unwrap();
    assert_eq!(query["triangle"]["points"], json!([0.5, 1.0, 0.5]));
}

#[test]
fn paint_order_set_changes_object_and_undo_restores() {
    let mut s = session();
    let out = s.run_named("shape.add", json!({"kind": ShapeKind::Ellipse, "size": [10.0, 10.0]})).unwrap();
    let id = out["id"].as_u64().unwrap();
    let original = s.document.objects.iter().find(|o| o.id.0 == id).unwrap().paint_order;
    let target = if original == PaintOrder::StrokeOver { PaintOrder::StrokeUnder } else { PaintOrder::StrokeOver };
    s.run_named("paint.order.set", json!({"order": target, "ids": [id]})).unwrap();
    assert_eq!(s.document.objects.iter().find(|o| o.id.0 == id).unwrap().paint_order, target);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.document.objects.iter().find(|o| o.id.0 == id).unwrap().paint_order, original);
}

#[test]
fn feature_restore_without_document_param_is_refused() {
    let mut s = session();
    assert!(s.run_named("core.feature.restore", json!({})).is_err());
}
