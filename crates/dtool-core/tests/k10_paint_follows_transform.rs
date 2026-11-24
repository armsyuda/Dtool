use dtool_core::document::Units;
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Hatch, HatchLine, HatchUnits, Paint};
use dtool_core::{Command, Document, Session};
use serde_json::json;

fn setup() -> (Session, ObjectId, ObjectId) {
    let mut s = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    let hatch = Paint::Hatch(Hatch { lines: vec![HatchLine { angle_rad: 0.5, origin: [0.0, 0.0], shift: 0.0, spacing: 4.0, dash: vec![] }], colour: Colour::new(1, 2, 3, 255), background: None, weight: 1.0, units: HatchUnits::Document, matrix: [1.0, 0.0, 0.0, 1.0, 5.0, 7.0] });
    let gradient = Paint::Gradient { kind: GradientKind::Linear, stops: vec![GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 255) }], geom: GradientGeom::from_legacy_matrix([1.0, 0.0, 0.0, 1.0, 5.0, 7.0]) };
    let a = s.document.create_object(Object { name: "h".into(), pos: [0.0, 0.0], size: [10.0, 10.0], fill: hatch, ..Object::default() });
    let b = s.document.create_object(Object { name: "g".into(), pos: [50.0, 0.0], size: [10.0, 10.0], fill: gradient, ..Object::default() });
    (s, a, b)
}

fn hatch_origin(s: &Session, id: ObjectId) -> [f64; 2] {
    match &s.document.get_object(id).unwrap().fill { Paint::Hatch(h) => [h.matrix[4], h.matrix[5]], other => panic!("{other:?}") }
}

fn gradient_start(s: &Session, id: ObjectId) -> [f64; 2] {
    match &s.document.get_object(id).unwrap().fill { Paint::Gradient { geom, .. } => geom.start, other => panic!("{other:?}") }
}

#[test]
fn k10_scale_x2_moves_hatch_origin_like_gradient_start_and_undoes() {
    let (mut s, a, b) = setup();
    for id in [a, b] {
        s.run_named("transform.set", json!({"ids": [id.0], "w": 20.0, "h": 20.0})).unwrap();
    }
    assert_eq!(hatch_origin(&s, a), [10.0, 14.0]);
    assert_eq!(gradient_start(&s, b), [10.0, 14.0]);
    s.run(&Command::new("edit.undo")).unwrap();
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(hatch_origin(&s, a), [5.0, 7.0]);
    assert_eq!(gradient_start(&s, b), [5.0, 7.0]);
}

#[test]
fn k10_selection_box_scale_scales_paint() {
    let (mut s, a, _) = setup();
    s.run_named("transform.selection_box", json!({"ids": [a.0], "origin": [0.0, 0.0], "translate": [0.0, 0.0], "scale": [2.0, 3.0], "rotation_deg": 0.0, "shear_deg": 0.0})).unwrap();
    assert_eq!(hatch_origin(&s, a), [10.0, 21.0]);
}

#[test]
fn k10_move_and_rotate_leave_paint_matrix_unchanged() {
    let (mut s, a, b) = setup();
    let before = (s.document.get_object(a).unwrap().fill.clone(), s.document.get_object(b).unwrap().fill.clone());
    for id in [a, b] {
        s.run_named("transform.set", json!({"ids": [id.0], "x": 100.0, "y": 80.0})).unwrap();
        s.run_named("transform.set", json!({"ids": [id.0], "rotation_deg": 90.0})).unwrap();
    }
    assert_eq!(s.document.get_object(a).unwrap().fill, before.0);
    assert_eq!(s.document.get_object(b).unwrap().fill, before.1);
}
