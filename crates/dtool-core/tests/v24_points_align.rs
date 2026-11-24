use dtool_core::commands::align::{align_deltas, distribute_deltas, union_rects};
use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::{Command, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 200.0], Units::Pt, 72.0, 1))
}

fn open_points(session: &mut Session, points: &[[f64; 2]]) -> u64 {
    let contour = Contour {
        points: points.iter().enumerate().map(|(i, pos)| PathPoint::sharp(PointId(i as u64 + 1), *pos)).collect(),
        closed: false,
    };
    let curve = Curve::from_contour_and_holes(contour, Vec::new());
    session.document.create_object(Object { name: "curve".into(), kind: ObjectKind::Curve(curve), ..Object::default() }).0
}

fn run(session: &mut Session, params: serde_json::Value) -> Result<serde_json::Value, String> {
    dtool_core::commands::align::run(session, &Command::with_params("path.points.align", params))
        .ok_or_else(|| "unhandled".to_string())?
        .map_err(|error| error.code)
}

fn world_points(session: &Session, id: u64) -> Vec<[f64; 2]> {
    let object = session.document.get_object(ObjectId(id)).unwrap();
    let delta = [f64::from(object.pos[0]), f64::from(object.pos[1])];
    object.curve().unwrap().contour.points.iter().map(|point| [point.pos[0] + delta[0], point.pos[1] + delta[1]]).collect()
}

fn ids(session: &Session, id: u64) -> Vec<u64> {
    session.document.get_object(ObjectId(id)).unwrap().curve().unwrap().contour.points.iter().map(|point| point.id.0).collect()
}

fn world_handles(session: &Session, id: u64, index: usize) -> (Option<[f64; 2]>, Option<[f64; 2]>) {
    let object = session.document.get_object(ObjectId(id)).unwrap();
    let delta = [f64::from(object.pos[0]), f64::from(object.pos[1])];
    let point = &object.curve().unwrap().contour.points[index];
    let shift = |h: [f64; 2]| [h[0] + delta[0], h[1] + delta[1]];
    (point.handle_in.map(shift), point.handle_out.map(shift))
}

#[test]
fn align_math_refactor_keeps_zero_size_and_object_formulas() {
    let rects = [[0.0, 0.0, 0.0, 0.0], [40.0, 20.0, 0.0, 0.0]];
    let target = union_rects(&rects).unwrap();
    assert_eq!(target, [0.0, 0.0, 40.0, 20.0]);
    assert_eq!(align_deltas(&rects, "top", target), vec![[0.0, 0.0], [0.0, -20.0]]);
    let boxes = [[0.0, 0.0, 10.0, 10.0], [30.0, 0.0, 10.0, 10.0], [80.0, 0.0, 10.0, 10.0]];
    let deltas = distribute_deltas(&boxes, true, None);
    assert_eq!(deltas[0], [0.0, 0.0]);
    assert!(deltas[2][0].abs() < 1e-4);
}

#[test]
fn path_points_align_top_card_align_03() {
    let mut session = session();
    let id = open_points(&mut session, &[[0.0, 0.0], [40.0, 20.0], [100.0, 0.0]]);
    run(&mut session, json!({ "target": id, "point_ids": [1, 2], "align": "top", "relative_to": "selection" })).unwrap();
    assert_eq!(world_points(&session, id), vec![[0.0, 0.0], [40.0, 0.0], [100.0, 0.0]]);
    assert_eq!(ids(&session, id), vec![1, 2, 3]);
    assert!(session.undo.can_undo());
    let undo = session.undo.undo().expect("undo");
    dtool_core::commands::arrange::apply(&mut session, &undo).unwrap().unwrap();
    assert_eq!(world_points(&session, id), vec![[0.0, 0.0], [40.0, 20.0], [100.0, 0.0]]);
    assert_eq!(ids(&session, id), vec![1, 2, 3]);
}

#[test]
fn path_points_align_single_point_is_invalid_and_unchanged() {
    let mut session = session();
    let id = open_points(&mut session, &[[0.0, 0.0], [40.0, 20.0], [100.0, 0.0]]);
    let before = serde_json::to_value(&session.document).unwrap();
    let error = run(&mut session, json!({ "target": id, "point_ids": [2], "align": "top" })).unwrap_err();
    assert_eq!(error, "INVALID_PARAMS");
    assert_eq!(serde_json::to_value(&session.document).unwrap(), before);
    assert!(!session.undo.can_undo());
}

#[test]
fn path_points_align_moves_handles_with_anchor() {
    let mut session = session();
    let mut points = vec![
        PathPoint::sharp(PointId(1), [0.0, 0.0]),
        PathPoint::sharp(PointId(2), [40.0, 20.0]),
        PathPoint::sharp(PointId(3), [100.0, 0.0]),
    ];
    points[1].handle_in = Some([30.0, 20.0]);
    points[1].handle_out = Some([50.0, 20.0]);
    let curve = Curve::from_contour_and_holes(Contour { points, closed: false }, Vec::new());
    let id = session.document.create_object(Object { name: "curve".into(), kind: ObjectKind::Curve(curve), ..Object::default() }).0;
    run(&mut session, json!({ "target": id, "point_ids": [1, 2], "align": "top" })).unwrap();
    assert_eq!(world_points(&session, id)[1], [40.0, 0.0]);
    assert_eq!(world_handles(&session, id, 1), (Some([30.0, 0.0]), Some([50.0, 0.0])));
    assert_eq!(world_points(&session, id)[2], [100.0, 0.0]);
}
