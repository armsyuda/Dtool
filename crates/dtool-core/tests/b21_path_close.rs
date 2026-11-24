use dtool_core::commands::path_nodes::world_curve;
use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn session() -> Session {
    Session::new(Document::new([400.0, 200.0], Units::Pt, 72.0, 1))
}

fn contour(points: &[[f64; 2]], closed: bool) -> Contour {
    Contour { points: points.iter().enumerate().map(|(i, pos)| PathPoint::sharp(PointId(i as u64 + 1), *pos)).collect(), closed }
}

fn line(session: &mut Session, points: &[[f64; 2]], closed: bool) -> u64 {
    let curve = Curve::from_contour_and_holes(contour(points, closed), Vec::new());
    session.document.create_object(Object { name: "curve".into(), kind: ObjectKind::Curve(curve), ..Object::default() }).0
}

fn world(session: &Session, id: u64) -> Curve {
    let object = session.document.get_object(ObjectId(id)).expect("object");
    world_curve(object, object.curve().expect("curve"))
}

fn ids(contour: &Contour) -> Vec<u64> {
    contour.points.iter().map(|p| p.id.0).collect()
}

fn close(session: &mut Session, params: Value) -> Result<Value, String> {
    session.run(&Command::with_params("path.close", params)).map_err(|e| e.code)
}

#[test]
fn catalog_registers_close() {
    assert!(session().catalog.assert_named("path.close").is_ok());
}

#[test]
fn brief_case_closes_subpath_zero_with_known_closing_segment_and_one_undo() {
    let mut s = session();
    let path = line(&mut s, &[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], false);
    let reply = close(&mut s, json!({ "target": format!("node:{path}"), "subpath": 0 })).unwrap();
    assert_eq!(reply["closed"], json!(true));
    assert_eq!(reply["closing_segment"], json!({ "from": 3, "to": 1 }));
    assert!(reply["welded"].is_null());
    let curve = world(&s, path);
    assert!(curve.contour.closed);
    assert_eq!(ids(&curve.contour), vec![1, 2, 3]);
    assert_eq!(curve.contour.points[2].pos, [100.0, 100.0]);
    s.run(&Command::new("edit.undo")).expect("undo");
    assert!(!world(&s, path).contour.closed);
    assert!(s.run(&Command::new("edit.undo")).is_err() || !world(&s, path).contour.closed);
    s.run(&Command::new("edit.redo")).expect("redo");
    assert!(world(&s, path).contour.closed);
}

#[test]
fn coincident_endpoints_weld_with_explicit_map() {
    let mut s = session();
    let path = line(&mut s, &[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 0.0]], false);
    let reply = close(&mut s, json!({ "id": path })).unwrap();
    assert_eq!(reply["welded"], json!({ "survivor": 1, "removed": 4 }));
    assert_eq!(reply["closing_segment"], json!({ "from": 3, "to": 1 }));
    assert_eq!(ids(&world(&s, path).contour), vec![1, 2, 3]);
}

#[test]
fn tolerance_widens_weld_and_default_does_not_weld_near_points() {
    let mut s = session();
    let a = line(&mut s, &[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.3, 0.0]], false);
    let b = line(&mut s, &[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.3, 0.0]], false);
    assert!(close(&mut s, json!({ "id": a })).unwrap()["welded"].is_null());
    assert_eq!(ids(&world(&s, a).contour).len(), 4);
    assert_eq!(close(&mut s, json!({ "id": b, "tolerance": "0.5pt" })).unwrap()["welded"], json!({ "survivor": 1, "removed": 4 }));
    assert_eq!(ids(&world(&s, b).contour).len(), 3);
}

#[test]
fn closes_second_subpath_and_offset_curve_keeps_world_position() {
    let mut s = session();
    let id = s.run(&Command::with_params("curve.add", json!({ "points": [[200.0, 50.0], [300.0, 50.0], [300.0, 150.0]] }))).unwrap()["id"].as_u64().unwrap();
    close(&mut s, json!({ "id": id })).unwrap();
    let curve = world(&s, id);
    assert_eq!(curve.contour.points[0].pos, [200.0, 50.0]);
    assert_eq!(s.document.get_object(ObjectId(id)).unwrap().pos, [200.0, 50.0]);
    let two = line(&mut s, &[[0.0, 0.0], [50.0, 0.0], [100.0, 0.0]], false);
    s.run(&Command::with_params("path.break", json!({ "id": two, "point": { "id": 2 }, "split_objects": false }))).unwrap();
    close(&mut s, json!({ "id": two, "subpath": 1 })).unwrap();
    let broken = world(&s, two);
    assert!(!broken.contour.closed);
    assert!(broken.holes[0].closed);
}

#[test]
fn rejects_closed_short_missing_and_non_curve() {
    let mut s = session();
    let closed = line(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], true);
    let single = line(&mut s, &[[0.0, 0.0]], false);
    assert_eq!(close(&mut s, json!({ "id": closed })).unwrap_err(), "ALREADY_CLOSED");
    assert_eq!(close(&mut s, json!({ "id": single })).unwrap_err(), "NOT_ENOUGH_POINTS");
    assert_eq!(close(&mut s, json!({ "id": closed, "subpath": 3 })).unwrap_err(), "NOT_FOUND");
    assert_eq!(close(&mut s, json!({ "id": closed, "subpath": "x" })).unwrap_err(), "INVALID_PARAMS");
    let shape = s.document.create_object(Object { size: [10.0, 10.0], ..Object::default() }).0;
    assert_eq!(close(&mut s, json!({ "id": shape })).unwrap_err(), "NOT_A_CURVE");
    assert_eq!(close(&mut s, json!({ "id": 999 })).unwrap_err(), "NOT_FOUND");
}