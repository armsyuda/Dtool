use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint, StrokeStyle};
use dtool_core::selection::SelectOp;
use dtool_core::{Object, ObjectDefaults, Session};
use serde_json::json;

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn c08_fix2_factory_stroke_is_one_point_in_document_pixels() {
    let factory = ObjectDefaults::default();
    assert!(near(factory.stroke_for(72.0).width, 1.0));
    assert!(near(factory.stroke_for(96.0).width, 96.0 / 72.0));
    assert!(near(factory.stroke_for(300.0).width, 300.0 / 72.0));
    assert_eq!(factory.stroke_for(300.0).paint, Paint::Solid { colour: Colour::new(0, 0, 0, 255) });
    let mut object = Object::default();
    factory.apply_in_document(&mut object, 144.0);
    assert_eq!(object.fill, Paint::None);
    assert!(near(object.stroke.width, 2.0));
}

#[test]
fn c08_fix2_sync_stores_points_and_round_trips() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 144.0, 1));
    let id = s.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], stroke: StrokeStyle { paint: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, width: 4.0, ..StrokeStyle::default() }, ..Object::default() });
    s.selection.set(vec![id], SelectOp::Replace);
    let mut defaults = ObjectDefaults::default();
    defaults.sync_from_selection(&s.document, &s.selection);
    assert!(near(defaults.stroke.width, 2.0));
    assert!(near(defaults.stroke_for(144.0).width, 4.0));
    assert!(near(defaults.stroke_for(72.0).width, 2.0));
}

#[test]
fn c08_fix2_paint_default_converts_width_to_millimetres() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Mm, 300.0, 1));
    let id = s.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], ..Object::default() });
    s.selection.set(vec![id], SelectOp::Replace);
    s.run_named("paint.default", json!({})).unwrap();
    let stroke = &s.document.get_object(id).unwrap().stroke;
    assert!(near(stroke.width, 25.4 / 72.0));
    assert_eq!(stroke.paint, ObjectDefaults::default_stroke().paint);
}
