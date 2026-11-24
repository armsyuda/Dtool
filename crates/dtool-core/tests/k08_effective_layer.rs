use dtool_core::document::{Document, Units};
use dtool_core::object::Object;
use dtool_core::paint::{Colour, Paint};
use dtool_core::scene::{effective_opacity, effective_pickable, estimate_scene};
use dtool_core::CanvasFlags;

#[test]
fn ancestor_unpickable_hides_child_from_estimated_scene() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let parent = doc.create_object(Object { pickable: false, size: [100.0, 100.0], ..Object::default() });
    let child = doc.create_object(Object {
        pos: [10.0, 10.0],
        size: [20.0, 20.0],
        parent_id: Some(parent),
        fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) },
        ..Object::default()
    });
    assert!(!effective_pickable(&doc, child));
    assert!((effective_opacity(&doc, child) - 1.0).abs() < 1e-6);
    let scene = estimate_scene(&doc, &CanvasFlags::default());
    assert_eq!(scene.counts().fills, 0);
}

#[test]
fn parent_opacity_multiplies() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let parent = doc.create_object(Object { opacity: 0.5, size: [100.0, 100.0], ..Object::default() });
    let child = doc.create_object(Object { parent_id: Some(parent), opacity: 0.5, ..Object::default() });
    assert!((effective_opacity(&doc, child) - 0.25).abs() < 1e-6);
}
