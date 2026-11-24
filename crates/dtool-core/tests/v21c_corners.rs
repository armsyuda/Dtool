use dtool_core::commands::curve_nodes;
use dtool_core::curve::{corner_eligible, CornerKind, Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::{Command, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 200.0], Units::Pt, 72.0, 1))
}

fn open_l(session: &mut Session) -> u64 {
    let mut points = vec![
        PathPoint::sharp(PointId(1), [0.0, 0.0]),
        PathPoint::sharp(PointId(2), [40.0, 0.0]),
        PathPoint::sharp(PointId(3), [40.0, 40.0]),
    ];
    points[2].kind = dtool_core::curve::NodeKind::Smooth;
    let curve = Curve::from_contour_and_holes(Contour { points, closed: false }, Vec::new());
    session.document.create_object(Object { name: "curve".into(), kind: ObjectKind::Curve(curve), ..Object::default() }).0
}

fn run(session: &mut Session, name: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
    curve_nodes::run(session, &Command::with_params(name, params)).ok_or_else(|| "unhandled".to_string())?.map_err(|error| error.code)
}

#[test]
fn corner_spec_round_trips_and_old_points_have_no_corner() {
    let point = PathPoint::sharp(PointId(1), [1.0, 2.0]);
    let encoded = serde_json::to_value(&point).unwrap();
    assert!(encoded.get("corner").is_none() || encoded["corner"].is_null());
    let decoded: PathPoint = serde_json::from_value(json!({"id": 7, "pos": [3.0, 4.0], "kind": "sharp"})).unwrap();
    assert_eq!(decoded.corner, None);
    let with_corner = json!({
        "id": 2,
        "pos": [40.0, 0.0],
        "kind": "sharp",
        "corner": { "kind": "rounded", "radius": 10.0 }
    });
    let loaded: PathPoint = serde_json::from_value(with_corner.clone()).unwrap();
    assert_eq!(loaded.corner.unwrap().kind, CornerKind::Rounded);
    assert_eq!(loaded.corner.unwrap().radius, 10.0);
    assert_eq!(serde_json::to_value(&loaded).unwrap()["corner"], with_corner["corner"]);
}

#[test]
fn corner_query_eligibility_sharp_smooth_endpoint() {
    let mut session = session();
    let id = open_l(&mut session);
    let reply = run(&mut session, "corner.query", json!({ "target": id })).unwrap();
    let points = reply["points"].as_array().unwrap();
    assert_eq!(points[0]["id"], json!(1));
    assert_eq!(points[0]["eligible"], json!(false));
    assert_eq!(points[1]["id"], json!(2));
    assert_eq!(points[1]["eligible"], json!(true));
    assert_eq!(points[2]["id"], json!(3));
    assert_eq!(points[2]["eligible"], json!(false));
    let curve = session.document.get_object(dtool_core::object::ObjectId(id)).unwrap().curve().unwrap();
    assert!(!corner_eligible(&curve.contour, 0));
    assert!(corner_eligible(&curve.contour, 1));
    assert!(!corner_eligible(&curve.contour, 2));
}

#[test]
fn corner_set_refuses_non_sharp_without_change() {
    let mut session = session();
    let id = open_l(&mut session);
    let before = serde_json::to_value(&session.document).unwrap();
    let error = run(&mut session, "corner.set", json!({ "target": id, "points": [3], "kind": "rounded", "radius": 10.0 })).unwrap_err();
    assert_eq!(error, "INVALID_PARAMS");
    assert_eq!(serde_json::to_value(&session.document).unwrap(), before);
    assert!(!session.undo.can_undo());
}

#[test]
fn corner_set_one_undo_and_native_round_trip() {
    let mut session = session();
    let id = open_l(&mut session);
    let reply = run(&mut session, "corner.set", json!({ "target": id, "points": [2], "kind": "rounded", "radius": 10.0 })).unwrap();
    assert_eq!(reply["points"][0]["radius"], json!(10.0));
    let curve = session.document.get_object(dtool_core::object::ObjectId(id)).unwrap().curve().unwrap().clone();
    let spec = curve.contour.points[1].corner.unwrap();
    assert_eq!(spec.kind, CornerKind::Rounded);
    assert_eq!(spec.radius, 10.0);
    let restored: PathPoint = serde_json::from_value(serde_json::to_value(&curve.contour.points[1]).unwrap()).unwrap();
    assert_eq!(restored.corner, curve.contour.points[1].corner);
    assert!(session.undo.can_undo());
    let undo = session.undo.undo().expect("undo");
    dtool_core::commands::arrange::apply(&mut session, &undo).unwrap().unwrap();
    let after = session.document.get_object(dtool_core::object::ObjectId(id)).unwrap().curve().unwrap();
    assert_eq!(after.contour.points[1].corner, None);
    assert!(!session.undo.can_undo());
}

#[test]
fn corner_endpoint_and_adjacent_clamp() {
    let mut session = session();
    let points = vec![
        PathPoint::sharp(PointId(1), [0.0, 0.0]),
        PathPoint::sharp(PointId(2), [20.0, 0.0]),
        PathPoint::sharp(PointId(3), [20.0, 20.0]),
        PathPoint::sharp(PointId(4), [0.0, 20.0]),
    ];
    let curve = Curve::from_contour_and_holes(Contour { points, closed: true }, Vec::new());
    let id = session.document.create_object(Object { name: "curve".into(), kind: ObjectKind::Curve(curve), ..Object::default() }).0;
    let reply = run(&mut session, "corner.set", json!({ "target": id, "points": [2, 3], "kind": "rounded", "radius": 100.0 })).unwrap();
    assert_eq!(reply["points"][0]["radius"], json!(10.0));
    assert_eq!(reply["points"][1]["radius"], json!(10.0));
}
