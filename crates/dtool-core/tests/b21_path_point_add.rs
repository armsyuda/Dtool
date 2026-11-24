use dtool_core::commands::path_nodes::world_curve;
use dtool_core::curve::{Contour, Curve, NodeKind, PathPoint, PointId};
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn session() -> Session {
    Session::new(Document::new([400.0, 200.0], Units::Pt, 72.0, 1))
}

fn add_contour(session: &mut Session, contour: Contour) -> u64 {
    let curve = Curve::from_contour_and_holes(contour, Vec::new());
    session.document.create_object(Object { name: "curve".into(), kind: ObjectKind::Curve(curve), ..Object::default() }).0
}

fn line(session: &mut Session, points: &[[f64; 2]], closed: bool) -> u64 {
    let points = points.iter().enumerate().map(|(i, pos)| PathPoint::sharp(PointId(i as u64 + 1), *pos)).collect();
    add_contour(session, Contour { points, closed })
}

fn world(session: &Session, id: u64) -> Contour {
    let object = session.document.get_object(ObjectId(id)).expect("object");
    world_curve(object, object.curve().expect("curve")).contour
}

fn ids(contour: &Contour) -> Vec<u64> {
    contour.points.iter().map(|p| p.id.0).collect()
}

fn add_point(session: &mut Session, params: Value) -> Result<Value, String> {
    session.run(&Command::with_params("path.point.add", params)).map_err(|e| e.code)
}

fn cubic(p: [[f64; 2]; 4], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    [
        w[0] * p[0][0] + w[1] * p[1][0] + w[2] * p[2][0] + w[3] * p[3][0],
        w[0] * p[0][1] + w[1] * p[1][1] + w[2] * p[2][1] + w[3] * p[3][1],
    ]
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

#[test]
fn catalog_registers_point_add() {
    assert!(session().catalog.assert_named("path.point.add").is_ok());
}

#[test]
fn brief_case_inserts_midpoint_without_breaking_the_curve() {
    let mut s = session();
    let other = line(&mut s, &[[0.0, 150.0], [10.0, 150.0], [20.0, 150.0], [30.0, 150.0], [40.0, 150.0]], false);
    let path = line(&mut s, &[[0.0, 0.0], [100.0, 0.0]], false);
    let objects = s.document.objects.len();
    let reply = add_point(&mut s, json!({ "target": format!("node:{path}"), "segment": { "from_id": "1", "to_id": "2" }, "t": 0.5, "unit": "pt" })).unwrap();
    assert_eq!(reply["pos"], json!([50.0, 0.0]));
    assert_eq!(reply["reversed"], json!(false));
    let fresh = reply["point"].as_u64().unwrap();
    assert_eq!(fresh, 6);
    assert!(world(&s, other).points.iter().all(|p| p.id.0 != fresh));
    let contour = world(&s, path);
    assert_eq!(ids(&contour), vec![1, fresh, 2]);
    assert_eq!(contour.points[0].pos, [0.0, 0.0]);
    assert_eq!(contour.points[1].pos, [50.0, 0.0]);
    assert_eq!(contour.points[2].pos, [100.0, 0.0]);
    assert!(!contour.closed);
    assert_eq!(s.document.objects.len(), objects);
    let object = s.document.get_object(ObjectId(path)).unwrap();
    assert!(object.curve().unwrap().holes.is_empty());
    s.run(&Command::new("edit.undo")).expect("undo");
    assert_eq!(ids(&world(&s, path)), vec![1, 2]);
    s.run(&Command::new("edit.redo")).expect("redo");
    assert_eq!(ids(&world(&s, path)), vec![1, fresh, 2]);
}

#[test]
fn reversed_segment_measures_t_from_from_id_and_keeps_orientation() {
    let mut s = session();
    let path = line(&mut s, &[[0.0, 0.0], [100.0, 0.0]], false);
    let reply = add_point(&mut s, json!({ "id": path, "segment": { "from_id": 2, "to_id": 1 }, "t": 0.25 })).unwrap();
    assert_eq!(reply["reversed"], json!(true));
    assert_eq!(reply["pos"], json!([75.0, 0.0]));
    let contour = world(&s, path);
    assert_eq!(ids(&contour)[0], 1);
    assert_eq!(ids(&contour)[2], 2);
    assert_eq!(contour.points[1].pos, [75.0, 0.0]);
}

#[test]
fn bezier_subdivision_keeps_curve_locus() {
    let mut s = session();
    let p = [[0.0, 0.0], [0.0, 60.0], [100.0, 60.0], [100.0, 0.0]];
    let a = PathPoint { id: PointId(1), pos: p[0], handle_in: None, handle_out: Some(p[1]), kind: NodeKind::Smooth, corner: None };
    let b = PathPoint { id: PointId(2), pos: p[3], handle_in: Some(p[2]), handle_out: None, kind: NodeKind::Smooth, corner: None };
    let path = add_contour(&mut s, Contour { points: vec![a, b], closed: false });
    let reply = add_point(&mut s, json!({ "id": path, "segment": { "from_id": 1, "to_id": 2 }, "t": 0.5 })).unwrap();
    let contour = world(&s, path);
    let (left, mid, right) = (&contour.points[0], &contour.points[1], &contour.points[2]);
    assert_eq!(mid.id.0, reply["point"].as_u64().unwrap());
    assert_eq!(mid.kind, NodeKind::Smooth);
    assert!(close(mid.pos, cubic(p, 0.5)));
    let first = [left.pos, left.handle_out.unwrap(), mid.handle_in.unwrap(), mid.pos];
    let second = [mid.pos, mid.handle_out.unwrap(), right.handle_in.unwrap(), right.pos];
    for k in 1..10 {
        let s_t = k as f64 / 10.0;
        assert!(close(cubic(first, s_t), cubic(p, s_t * 0.5)));
        assert!(close(cubic(second, s_t), cubic(p, 0.5 + s_t * 0.5)));
    }
    assert_eq!(left.pos, p[0]);
    assert_eq!(right.pos, p[3]);
}

#[test]
fn closing_segment_of_closed_curve_gets_point_at_end() {
    let mut s = session();
    let path = line(&mut s, &[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], true);
    let reply = add_point(&mut s, json!({ "id": path, "segment": { "from_id": 3, "to_id": 1 }, "t": 0.5 })).unwrap();
    assert_eq!(reply["pos"], json!([50.0, 50.0]));
    let contour = world(&s, path);
    assert_eq!(ids(&contour), vec![1, 2, 3, 4]);
    assert!(contour.closed);
}

#[test]
fn offset_curve_from_curve_add_inserts_in_world_space() {
    let mut s = session();
    let path = s.run(&Command::with_params("curve.add", json!({ "points": [[200.0, 50.0], [300.0, 50.0]] }))).unwrap()["id"].as_u64().unwrap();
    let reply = add_point(&mut s, json!({ "id": path, "segment": { "from_id": 1, "to_id": 2 }, "t": 0.5 })).unwrap();
    assert_eq!(reply["pos"], json!([250.0, 50.0]));
    assert_eq!(world(&s, path).points[1].pos, [250.0, 50.0]);
    assert_eq!(s.document.get_object(ObjectId(path)).unwrap().pos, [200.0, 50.0]);
}

#[test]
fn rejects_non_segments_bad_t_and_unknown_points() {
    let mut s = session();
    let path = line(&mut s, &[[0.0, 0.0], [50.0, 0.0], [100.0, 0.0]], false);
    let before = world(&s, path);
    assert_eq!(add_point(&mut s, json!({ "id": path, "segment": { "from_id": 1, "to_id": 3 } })).unwrap_err(), "NOT_A_SEGMENT");
    assert_eq!(add_point(&mut s, json!({ "id": path, "segment": { "from_id": 3, "to_id": 1 }, "t": 0.5 })).unwrap_err(), "NOT_A_SEGMENT");
    assert_eq!(add_point(&mut s, json!({ "id": path, "segment": { "from_id": 1, "to_id": 2 }, "t": 0.0 })).unwrap_err(), "INVALID_PARAMS");
    assert_eq!(add_point(&mut s, json!({ "id": path, "segment": { "from_id": 1, "to_id": 2 }, "t": 1.0 })).unwrap_err(), "INVALID_PARAMS");
    assert_eq!(add_point(&mut s, json!({ "id": path, "segment": { "from_id": 1, "to_id": 9 } })).unwrap_err(), "NOT_FOUND");
    assert_eq!(add_point(&mut s, json!({ "id": path })).unwrap_err(), "INVALID_PARAMS");
    let shape = s.document.create_object(Object { size: [10.0, 10.0], ..Object::default() }).0;
    assert_eq!(add_point(&mut s, json!({ "id": shape, "segment": { "from_id": 1, "to_id": 2 } })).unwrap_err(), "NOT_A_CURVE");
    assert_eq!(world(&s, path), before);
}