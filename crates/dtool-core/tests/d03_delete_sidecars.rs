use dtool_core::command::Session;
use dtool_core::document::Document;
use dtool_core::object::ObjectId;
use dtool_core::document::Units;
use serde_json::json;

fn placed_masked() -> (Session, ObjectId) {
    let mut s = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    let id = s.run_named("image.place", json!({"pixel_size": [2, 1], "rgba": [255, 0, 0, 255, 0, 255, 0, 200], "pos": [10, 20]})).unwrap()["id"].as_u64().unwrap();
    s.run_named("layer.mask.set", json!({"id": id, "mask": {"width": 2, "height": 1, "coverage": [255, 0]}})).unwrap();
    (s, ObjectId(id))
}

fn assert_no_orphans(doc: &Document) {
    let live: std::collections::HashSet<ObjectId> = doc.objects.iter().map(|o| o.id).collect();
    assert!(doc.layer_masks.iter().all(|m| live.contains(&m.object)));
    assert!(doc.placed_images.iter().all(|p| live.contains(&p.object_id)));
    assert!(doc.pixel_layers.entries.iter().all(|e| live.contains(&e.object)));
}

#[test]
fn d03_delete_object_helper_drops_sidecars_and_keeps_id_high_water() {
    let (mut s, id) = placed_masked();
    let next = s.document.next_object_id;
    s.document.delete_object(id).unwrap();
    assert!(s.document.layer_masks.is_empty());
    assert!(s.document.placed_images.is_empty());
    assert!(!s.document.pixel_layers.contains(id));
    assert_eq!(s.document.next_object_id, next);
    assert_no_orphans(&s.document);
    let reopened: Document = serde_json::from_value(serde_json::to_value(&s.document).unwrap()).unwrap();
    assert_no_orphans(&reopened);
}

#[test]
fn d03_object_delete_uses_same_policy_and_undo_restores() {
    let (mut s, id) = placed_masked();
    s.run_named("object.delete", json!({"id": id.0})).unwrap();
    assert_no_orphans(&s.document);
    assert!(s.document.layer_masks.is_empty() && s.document.placed_images.is_empty());
    s.run(&dtool_protocol::Command::new("edit.undo")).unwrap();
    assert_eq!(s.document.layer_masks.len(), 1);
    assert_eq!(s.document.placed_images.len(), 1);
    assert!(s.document.pixel_layers.contains(id));
}
