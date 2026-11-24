use dtool_core::document::{Document, Units};
use dtool_core::object::Object;
use dtool_core::paint::{Colour, Paint};
use dtool_export::{export_visible, DocumentExportExt, ObjectExportExt};

#[test]
fn hidden_object_is_not_visible_and_does_not_rasterize() {
    let mut doc = Document::new([40.0, 40.0], Units::Px, 72.0, 1);
    let parent = doc.create_object(Object { pickable: false, size: [40.0, 40.0], ..Object::default() });
    let child = doc.create_object(Object {
        pos: [0.0, 0.0],
        size: [40.0, 40.0],
        parent_id: Some(parent),
        fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) },
        ..Object::default()
    });
    let object = doc.get_object(child).unwrap();
    assert!(object.is_visible());
    assert!(!export_visible(&doc, object));
    let rgba = doc.render_rgba_region((0.0, 0.0, 40.0, 40.0), 8, 8);
    assert!(rgba.chunks(4).all(|px| px[3] == 0));
}

#[test]
fn unpickable_self_is_not_visible() {
    let mut obj = Object::default();
    obj.pickable = false;
    obj.size = [10.0, 10.0];
    assert!(!obj.is_visible());
}
