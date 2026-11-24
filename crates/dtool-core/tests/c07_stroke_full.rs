use dtool_core::document::{Document, Units};
use dtool_core::paint::{Arrow, Cap, Colour, Join, Paint, StrokeAlign};
use dtool_core::selection::SelectOp;
use dtool_core::{Object, ObjectId, Session};
use serde_json::json;

fn session_with_two() -> (Session, ObjectId, ObjectId) {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    let a = s.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], ..Object::default() });
    let b = s.document.create_object(Object { name: "b".into(), pos: [20.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    s.selection.set(vec![a, b], SelectOp::Replace);
    (s, a, b)
}

#[test]
fn c07_stroke_set_applies_every_key_to_the_selection_with_one_undo() {
    let (mut s, a, b) = session_with_two();
    let before_a = s.document.get_object(a).unwrap().stroke.clone();
    let before_b = s.document.get_object(b).unwrap().stroke.clone();
    let depth = s.undo.applied_steps();
    s.run_named("stroke.set", json!({
        "width": 2.0,
        "dash": [3.0, 1.0],
        "cap": "Round",
        "join": "Bevel",
        "align": "Inside",
        "end_arrow": {"name": "triangle", "scale": 1.5},
        "scale_with_object": false,
        "pressure": [[0.0, 0.5], [1.0, 1.0]]
    })).unwrap();
    for id in [a, b] {
        let stroke = &s.document.get_object(id).unwrap().stroke;
        assert_eq!(stroke.width, 2.0);
        assert_eq!(stroke.dash, vec![3.0, 1.0]);
        assert_eq!(stroke.cap, Cap::Round);
        assert_eq!(stroke.join, Join::Bevel);
        assert_eq!(stroke.align, StrokeAlign::Inside);
        assert_eq!(stroke.end_arrow, Some(Arrow { name: "triangle".into(), scale: 1.5 }));
        assert_eq!(stroke.start_arrow, None);
        assert!(!stroke.scale_with_object);
        assert_eq!(stroke.pressure, vec![[0.0, 0.5], [1.0, 1.0]]);
        assert_eq!(stroke.paint, Paint::Solid { colour: Colour::new(0, 0, 0, 255) });
        let queried = s.run_named("stroke.query", json!({"id": id.0})).unwrap();
        assert_eq!(queried["stroke"]["cap"], "Round");
    }
    assert_eq!(s.undo.applied_steps(), depth + 1);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.document.get_object(a).unwrap().stroke, before_a);
    assert_eq!(s.document.get_object(b).unwrap().stroke, before_b);
    s.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(s.document.get_object(b).unwrap().stroke.cap, Cap::Round);
}

#[test]
fn c07_stroke_set_absent_keys_keep_values_and_null_arrow_clears() {
    let (mut s, a, _) = session_with_two();
    s.run_named("stroke.set", json!({"id": a.0, "width": 1.0, "cap": "Square", "start_arrow": {"name": "circle", "scale": 1.0}})).unwrap();
    s.run_named("stroke.set", json!({"id": a.0, "start_arrow": null})).unwrap();
    let stroke = &s.document.get_object(a).unwrap().stroke;
    assert_eq!(stroke.cap, Cap::Square);
    assert_eq!(stroke.width, 1.0);
    assert_eq!(stroke.start_arrow, None);
}

#[test]
fn c07_stroke_set_explicit_paint_and_bad_values() {
    let (mut s, a, b) = session_with_two();
    let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
    s.run_named("stroke.set", json!({"ids": [a.0], "width": 1.0, "paint": red.clone()})).unwrap();
    assert_eq!(s.document.get_object(a).unwrap().stroke.paint, red);
    assert_eq!(s.document.get_object(b).unwrap().stroke.width, 0.0);
    let depth = s.undo.applied_steps();
    assert!(s.run_named("stroke.set", json!({"ids": [a.0, b.0], "width": -1.0})).is_err());
    assert!(s.run_named("stroke.set", json!({"ids": [a.0], "cap": "Wavy"})).is_err());
    assert!(s.run_named("stroke.set", json!({"ids": [a.0, 9999], "width": 3.0})).is_err());
    assert_eq!(s.document.get_object(a).unwrap().stroke.width, 1.0);
    assert_eq!(s.undo.applied_steps(), depth);
}