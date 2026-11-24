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

#[test]
fn points_transform_moves_selected_only_one_undo() {
    let (mut ws, cx, id) = setup(vec![contour(1, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false)]);
    let before = world(&ws, cx, id);
    run(&mut ws, cx, "path.points.transform", json!({"id": id, "points": [2, 3], "dx": 5.0, "dy": -2.0})).unwrap();
    let after = world(&ws, cx, id);
    assert!(close(point(&after, 1).pos, [0.0, 0.0]));
    assert!(close(point(&after, 2).pos, [15.0, -2.0]));
    assert!(close(point(&after, 3).pos, [15.0, 8.0]));
    run(&mut ws, cx, "path.point.move", json!({"id": id, "point": 1, "pos": [1.0, 1.0]})).unwrap();
    assert!(close(point(&world(&ws, cx, id), 1).pos, [1.0, 1.0]));
    undo(&mut ws, cx);
    undo(&mut ws, cx);
    assert_eq!(world(&ws, cx, id), before);
    let snapshot = world(&ws, cx, id);
    assert!(run(&mut ws, cx, "path.points.transform", json!({"id": id, "points": [2, 99], "dx": 1.0})).is_err());
    assert_eq!(world(&ws, cx, id), snapshot);
}

#[test]
fn handle_set_mirror_angle_keeps_opposite_angle() {
    let mut c = contour(1, &[[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]], false);
    c.points[1].handle_in = Some([6.0, 0.0]);
    c.points[1].handle_out = Some([14.0, 0.0]);
    let (mut ws, cx, id) = setup(vec![c]);
    run(&mut ws, cx, "path.point.handle", json!({"id": id, "point": 2, "side": "out", "pos": [10.0, 10.0], "mirror": "angle"})).unwrap();
    let p = point(&world(&ws, cx, id), 2);
    assert!(close(p.handle_out.unwrap(), [10.0, 10.0]));
    assert!(close(p.handle_in.unwrap(), [10.0, -4.0]));
    assert_eq!(p.kind, NodeKind::Smooth);
    run(&mut ws, cx, "path.point.handle", json!({"id": id, "point": 2, "side": "in", "pos": [7.0, 3.0], "mirror": "full"})).unwrap();
    let p = point(&world(&ws, cx, id), 2);
    assert!(close(p.handle_out.unwrap(), [13.0, -3.0]));
    run(&mut ws, cx, "path.point.handle", json!({"id": id, "point": 2, "side": "out", "pos": [12.0, 5.0], "mirror": false})).unwrap();
    let p = point(&world(&ws, cx, id), 2);
    assert!(close(p.handle_in.unwrap(), [7.0, 3.0]));
    assert_eq!(p.kind, NodeKind::Sharp);
    undo(&mut ws, cx);
    assert!(close(point(&world(&ws, cx, id), 2).handle_out.unwrap(), [13.0, -3.0]));
    assert!(run(&mut ws, cx, "path.point.handle", json!({"id": id, "point": 2, "side": "up", "pos": [0.0, 0.0]})).is_err());
}

#[test]
fn path_selection_set_is_non_mutating() {
    let (mut ws, cx, id) = setup(vec![contour(1, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false)]);
    let generation = ws.get(cx).unwrap().session.generation;
    let document = serde_json::to_value(&ws.get(cx).unwrap().session.document).unwrap();
    let reply = run(&mut ws, cx, "ui.path.selection.set", json!({"id": id, "points": [1, 3]})).unwrap();
    assert_eq!(reply["points"], json!([1, 3]));
    assert_eq!(ws.get(cx).unwrap().session.generation, generation);
    assert_eq!(serde_json::to_value(&ws.get(cx).unwrap().session.document).unwrap(), document);
    assert!(!ws.get(cx).unwrap().session.undo.can_undo());
    assert!(run(&mut ws, cx, "ui.path.selection.set", json!({"id": id, "points": [42]})).is_err());
}
