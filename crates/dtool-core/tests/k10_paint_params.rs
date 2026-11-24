use dtool_core::document::{Document, Units};
use dtool_core::object::{node_icc_set, Object, ObjectId};
use dtool_core::paint::{transform_paint_matrix, Colour, GradientGeom, GradientKind, GradientStop, Hatch, HatchUnits, Paint};
use dtool_core::Session;
use serde_json::json;

fn session() -> (Session, u64) {
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
    let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, ..Object::default() });
    (session, id.0)
}

fn fill(session: &Session, id: u64) -> Paint {
    session.document.get_object(ObjectId(id)).unwrap().fill.clone()
}

#[test]
fn update_accepts_hex_fill_as_solid() {
    let (mut s, id) = session();
    s.run_named("object.update", json!({"id": id, "fill": "#FF0000"})).unwrap();
    assert_eq!(fill(&s, id), Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
}

#[test]
fn unknown_paint_type_is_invalid_and_keeps_fill() {
    let (mut s, id) = session();
    let before = fill(&s, id);
    let err = s.run_named("object.update", json!({"id": id, "fill": {"type": "bogus"}})).unwrap_err();
    assert_eq!(err.code, "INVALID_PARAMS");
    assert_eq!(fill(&s, id), before);
}

#[test]
fn null_fill_means_no_change_and_none_string_clears() {
    let (mut s, id) = session();
    let before = fill(&s, id);
    s.run_named("object.update", json!({"id": id, "fill": null, "opacity": 0.5})).unwrap();
    assert_eq!(fill(&s, id), before);
    s.run_named("object.update", json!({"id": id, "fill": "none"})).unwrap();
    assert_eq!(fill(&s, id), Paint::None);
}

#[test]
fn empty_stop_gradient_and_negative_stroke_are_rejected() {
    let (mut s, id) = session();
    let before = s.document.get_object(ObjectId(id)).unwrap().clone();
    let bad = Paint::Gradient { kind: GradientKind::Linear, stops: Vec::new(), geom: GradientGeom::default() };
    assert_eq!(s.run_named("object.update", json!({"id": id, "fill": bad})).unwrap_err().code, "INVALID_PARAMS");
    let mut stroke = before.stroke.clone();
    stroke.width = -1.0;
    assert_eq!(s.run_named("object.update", json!({"id": id, "stroke": stroke})).unwrap_err().code, "INVALID_PARAMS");
    let after = s.document.get_object(ObjectId(id)).unwrap();
    assert_eq!(after.fill, before.fill);
    assert_eq!(after.stroke, before.stroke);
    let hatch = Paint::Hatch(Hatch { lines: vec![dtool_core::paint::HatchLine { angle_rad: 0.5, origin: [0.0, 0.0], shift: 0.0, spacing: 4.0, dash: Vec::new() }], colour: Colour::new(0, 0, 0, 255), background: None, weight: 1.0, units: HatchUnits::Document, matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] });
    s.run_named("object.update", json!({"id": id, "fill": hatch.clone()})).unwrap();
    assert_eq!(fill(&s, id), hatch);
}

#[test]
fn paste_style_with_bad_fill_changes_nothing() {
    let (mut s, id) = session();
    let before = fill(&s, id);
    s.selection.ids = vec![ObjectId(id)];
    s.clipboard.set_style(json!({"fill": {"type": "bogus"}, "opacity": 0.2}));
    assert!(s.run_named("edit.paste_style", json!({})).is_err());
    let obj = s.document.get_object(ObjectId(id)).unwrap();
    assert_eq!(obj.fill, before);
    assert_eq!(obj.opacity, 1.0);
}

#[test]
fn legacy_object_fill_strings_open_as_solid_and_save_typed() {
    let mut value = serde_json::to_value(Object::default()).unwrap();
    value["fill"] = json!("#FF0000");
    value["stroke"] = json!({"colour": "red", "width": 2.0});
    let object: Object = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(object.fill, Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
    assert_eq!(object.stroke.width, 2.0);
    value["fill"] = json!("red");
    let object: Object = serde_json::from_value(value).unwrap();
    assert_eq!(object.fill, Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
    let saved = serde_json::to_value(&object).unwrap();
    assert_eq!(saved["fill"]["type"], "solid");
    let typed: Object = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(serde_json::to_value(&typed).unwrap(), saved);
}

#[test]
fn hatch_and_gradient_follow_transform_in_same_order() {
    let scale = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
    let mut hatch = Paint::Hatch(Hatch { lines: Vec::new(), colour: Colour::new(0, 0, 0, 255), background: None, weight: 1.0, units: HatchUnits::Document, matrix: [1.0, 0.0, 0.0, 1.0, 5.0, 7.0] });
    transform_paint_matrix(&mut hatch, scale);
    let Paint::Hatch(h) = &hatch else { panic!() };
    assert_eq!([h.matrix[4], h.matrix[5]], [10.0, 14.0]);
    let mut gradient = Paint::Gradient { kind: GradientKind::Linear, stops: Vec::<GradientStop>::new(), geom: GradientGeom { start: [5.0, 7.0], end: [6.0, 7.0], ratio: 1.0 } };
    transform_paint_matrix(&mut gradient, scale);
    let Paint::Gradient { geom, .. } = &gradient else { panic!() };
    assert_eq!(geom.start, [10.0, 14.0]);
}

#[test]
fn node_icc_set_checks_profile_and_intent() {
    let mut objects = vec![Object { id: ObjectId(3), ..Object::default() }];
    assert_eq!(node_icc_set(&mut objects, &json!({"id": 3, "profile": "nope"})).unwrap_err().code, "PROFILE_NOT_FOUND");
    assert!(objects[0].icc.is_none());
    assert_eq!(node_icc_set(&mut objects, &json!({"id": 3, "intent": "Weird"})).unwrap_err().code, "UNSUPPORTED_INTENT");
    node_icc_set(&mut objects, &json!({"id": 3, "profile": "srgb"})).unwrap();
    assert_eq!(objects[0].icc.as_ref().and_then(|icc| icc.profile.clone()).as_deref(), Some("sRGB IEC61966-2.1"));
}
