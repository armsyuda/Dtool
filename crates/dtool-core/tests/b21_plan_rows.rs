use dtool_core::commands::path_nodes::world_curve;
use dtool_core::curve::{Contour, Curve, NodeKind, PathPoint, PointId};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::{Command, ContextId, Scope, Workspace};
use serde_json::{json, Value};

fn contour(first: u64, points: &[[f64; 2]], closed: bool) -> Contour {
    Contour { points: points.iter().enumerate().map(|(i, pos)| PathPoint::sharp(PointId(first + i as u64), *pos)).collect(), closed }
}

fn setup(contours: Vec<Contour>) -> (Workspace, ContextId, u64) {
    let mut workspace = Workspace::default();
    let context = workspace.new_document("paths");
    let mut all = contours.into_iter();
    let curve = Curve::from_contour_and_holes(all.next().unwrap(), all.collect());
    let id = workspace.get_mut(context).unwrap().session.document.create_object(Object { name: "curve".into(), kind: ObjectKind::Curve(curve), ..Object::default() }).0;
    (workspace, context, id)
}

fn run(workspace: &mut Workspace, context: ContextId, name: &str, params: Value) -> Result<Value, String> {
    workspace.run(Scope::of(context), Command::with_params(name, params)).1.map_err(|e| e.code)
}

fn world(workspace: &Workspace, context: ContextId, id: u64) -> Curve {
    let object = workspace.get(context).unwrap().session.document.get_object(ObjectId(id)).unwrap();
    world_curve(object, object.curve().unwrap())
}

fn point(curve: &Curve, id: u64) -> PathPoint {
    curve.contours().flat_map(|c| c.points.iter()).find(|p| p.id.0 == id).unwrap().clone()
}

fn undo(workspace: &mut Workspace, context: ContextId) {
    run(workspace, context, "edit.undo", json!({})).unwrap();
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

fn ids(curve: &Curve, subpath: usize) -> Vec<u64> {
    curve.contours().nth(subpath).unwrap().points.iter().map(|p| p.id.0).collect()
}

#[test]
fn reverse_swaps_order_and_handles_one_undo() {
    let mut c = contour(1, &[[0.0, 0.0], [10.0, 5.0], [20.0, 0.0]], false);
    c.points[1].handle_in = Some([5.0, 5.0]);
    c.points[1].handle_out = Some([15.0, 5.0]);
    let (mut ws, cx, id) = setup(vec![c]);
    let before = world(&ws, cx, id);
    run(&mut ws, cx, "path.reverse", json!({"id": id, "subpath": 0})).unwrap();
    let after = world(&ws, cx, id);
    assert_eq!(ids(&after, 0), vec![3, 2, 1]);
    assert_eq!(point(&after, 2).handle_in, Some([15.0, 5.0]));
    assert_eq!(point(&after, 2).handle_out, Some([5.0, 5.0]));
    undo(&mut ws, cx);
    assert_eq!(world(&ws, cx, id), before);
    assert!(run(&mut ws, cx, "path.reverse", json!({"id": id, "subpath": 4})).is_err());
    assert_eq!(world(&ws, cx, id), before);
}

#[test]
fn bridge_merges_subpaths_inside_one_object_unlike_join() {
    let (mut ws, cx, id) = setup(vec![contour(1, &[[0.0, 0.0], [10.0, 0.0]], false), contour(3, &[[20.0, 0.0], [30.0, 0.0]], false)]);
    let before = world(&ws, cx, id);
    assert!(run(&mut ws, cx, "path.join", json!({"ids": [id]})).is_err());
    let objects = ws.get(cx).unwrap().session.document.objects.len();
    run(&mut ws, cx, "path.points.bridge", json!({"id": id, "points": [2, 3]})).unwrap();
    let after = world(&ws, cx, id);
    assert_eq!(after.holes.len(), 0);
    assert_eq!(ids(&after, 0), vec![1, 2, 3, 4]);
    assert_eq!(ws.get(cx).unwrap().session.document.objects.len(), objects);
    undo(&mut ws, cx);
    assert_eq!(world(&ws, cx, id), before);
    assert!(run(&mut ws, cx, "path.points.bridge", json!({"id": id, "points": [1, 4]})).is_ok());
    assert_eq!(ids(&world(&ws, cx, id), 0), vec![2, 1, 4, 3]);
}

#[test]
fn bridge_closes_own_endpoints_and_rejects_interior_points() {
    let (mut ws, cx, id) = setup(vec![contour(1, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false)]);
    assert_eq!(run(&mut ws, cx, "path.points.bridge", json!({"id": id, "points": [1, 2]})), Err("NOT_AN_ENDPOINT".into()));
    run(&mut ws, cx, "path.points.bridge", json!({"id": id, "points": [1, 3]})).unwrap();
    assert!(world(&ws, cx, id).contour.closed);
}

#[test]
fn smooth_sets_smooth_handles_one_undo() {
    let (mut ws, cx, id) = setup(vec![contour(1, &[[0.0, 0.0], [10.0, 10.0], [20.0, 0.0]], false)]);
    let before = world(&ws, cx, id);
    run(&mut ws, cx, "path.smooth", json!({"id": id, "subpath": 0, "amount": 1.0})).unwrap();
    let mid = point(&world(&ws, cx, id), 2);
    assert_eq!(mid.kind, NodeKind::Smooth);
    let (hin, hout) = (mid.handle_in.unwrap(), mid.handle_out.unwrap());
    assert!(close([hin[0] + hout[0], hin[1] + hout[1]], [20.0, 20.0]));
    assert!(hout[0] > 10.0);
    undo(&mut ws, cx);
    assert_eq!(world(&ws, cx, id), before);
    assert!(run(&mut ws, cx, "path.smooth", json!({"id": id, "amount": 2.0})).is_err());
}

#[test]
fn delete_keep_shape_fits_neighbours_and_plain_delete_aliases_nodes_delete() {
    let mut c = contour(1, &[[0.0, 0.0], [10.0, 10.0], [20.0, 0.0], [30.0, 0.0]], false);
    c.points[1].handle_in = Some([5.0, 10.0]);
    c.points[1].handle_out = Some([15.0, 10.0]);
    c.points[0].handle_out = Some([0.0, 5.0]);
    let (mut ws, cx, id) = setup(vec![c]);
    let before = world(&ws, cx, id);
    run(&mut ws, cx, "path.point.delete", json!({"id": id, "points": [2], "keep_shape": true})).unwrap();
    let after = world(&ws, cx, id);
    assert_eq!(ids(&after, 0), vec![1, 3, 4]);
    let out = point(&after, 1).handle_out.unwrap();
    assert!(out[0].abs() < 1e-6 && out[1] > 5.0);
    assert!(point(&after, 3).handle_in.is_some());
    undo(&mut ws, cx);
    assert_eq!(world(&ws, cx, id), before);
    run(&mut ws, cx, "path.point.delete", json!({"id": id, "points": [2], "keep_shape": false})).unwrap();
    let plain = world(&ws, cx, id);
    assert_eq!(ids(&plain, 0), vec![1, 3, 4]);
    undo(&mut ws, cx);
    assert_eq!(world(&ws, cx, id), before);
    run(&mut ws, cx, "path.point.type", json!({"id": id, "points": [3], "type": "smooth"})).unwrap();
    assert_ne!(world(&ws, cx, id), before);
}

#[test]
fn segment_edit_passes_through_drag_point_one_undo() {
    let (mut ws, cx, id) = setup(vec![contour(1, &[[0.0, 0.0], [30.0, 0.0], [30.0, 30.0]], false)]);
    let before = world(&ws, cx, id);
    run(&mut ws, cx, "path.segment.edit", json!({"id": id, "segment": 0, "pos": [15.0, 9.0]})).unwrap();
    let after = world(&ws, cx, id);
    let (a, b) = (point(&after, 1), point(&after, 2));
    let (p1, p2) = (a.handle_out.unwrap(), b.handle_in.unwrap());
    let mid = [0.125 * a.pos[0] + 0.375 * p1[0] + 0.375 * p2[0] + 0.125 * b.pos[0], 0.125 * a.pos[1] + 0.375 * p1[1] + 0.375 * p2[1] + 0.125 * b.pos[1]];
    assert!(close(mid, [15.0, 9.0]));
    assert!(close(a.pos, [0.0, 0.0]) && close(b.pos, [30.0, 0.0]));
    undo(&mut ws, cx);
    assert_eq!(world(&ws, cx, id), before);
    assert!(run(&mut ws, cx, "path.segment.edit", json!({"id": id, "segment": 5, "pos": [0.0, 0.0]})).is_err());
    assert_eq!(world(&ws, cx, id), before);
}
