use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathTextData};
use dtool_core::commands::path_nodes::world_curve;
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn session() -> Session {
    Session::new(Document::new([400.0, 200.0], Units::Pt, 72.0, 1))
}

fn contour(points: &[[f64; 2]], closed: bool) -> Contour {
    Contour { points: points.iter().enumerate().map(|(i, pos)| PathPoint::sharp(PointId(i as u64 + 1), *pos)).collect(), closed }
}

fn add_curve(session: &mut Session, points: &[[f64; 2]], closed: bool) -> u64 {
    let curve = Curve::from_contour_and_holes(contour(points, closed), Vec::new());
    session.document.create_object(Object { name: "curve".into(), kind: ObjectKind::Curve(curve), ..Object::default() }).0
}

fn curve(session: &Session, id: u64) -> Curve {
    let object = session.document.get_object(ObjectId(id)).expect("object");
    match &object.kind {
        ObjectKind::Curve(curve) => world_curve(object, curve),
        other => panic!("not a curve: {other:?}"),
    }
}

fn local(session: &Session, id: u64) -> Curve {
    session.document.get_object(ObjectId(id)).and_then(|object| object.curve().cloned()).expect("curve")
}

fn ids(contour: &Contour) -> Vec<u64> {
    contour.points.iter().map(|p| p.id.0).collect()
}

fn xs(contour: &Contour) -> Vec<f64> {
    contour.points.iter().map(|p| p.pos[0]).collect()
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|e| panic!("{name} failed: {}", e.code))
}

fn fails(session: &mut Session, name: &str, params: Value) -> String {
    session.run(&Command::with_params(name, params)).expect_err("command must fail").code
}

fn undo(session: &mut Session) {
    session.run(&Command::new("edit.undo")).expect("undo");
}

fn redo(session: &mut Session) {
    session.run(&Command::new("edit.redo")).expect("redo");
}

#[test]
fn catalog_registers_break_and_join() {
    let session = session();
    for name in ["path.break", "path.join"] {
        assert!(session.catalog.assert_named(name).is_ok(), "{name} missing from catalog");
    }
}

#[test]
fn break_open_curve_splits_objects_with_duplicated_boundary() {
    let mut s = session();
    let path = add_curve(&mut s, &[[0.0, 0.0], [50.0, 0.0], [100.0, 0.0]], false);
    let original = curve(&s, path);
    let reply = run(&mut s, "path.break", json!({ "target": format!("node:{path}"), "point": { "id": "2" }, "split_objects": true }));
    assert_eq!(reply["duplicate"], json!(4));
    let left = curve(&s, path);
    assert_eq!(ids(&left.contour), vec![1, 2]);
    assert!(!left.contour.closed);
    let other = reply["new_object"].as_u64().expect("new object");
    let right = curve(&s, other);
    assert_eq!(ids(&right.contour), vec![4, 3]);
    assert!(!right.contour.closed);
    assert_eq!(right.contour.points[0].pos, left.contour.points[1].pos);
    assert_eq!(xs(&left.contour), vec![0.0, 50.0]);
    assert_eq!(xs(&right.contour), vec![50.0, 100.0]);
    assert_eq!(s.document.get_object(ObjectId(other)).unwrap().pos, [50.0, 0.0]);
    undo(&mut s);
    assert!(s.document.get_object(ObjectId(other)).is_none());
    assert_eq!(curve(&s, path), original);
    redo(&mut s);
    assert_eq!(ids(&curve(&s, other).contour), vec![4, 3]);
}

#[test]
fn break_without_split_keeps_one_object_with_two_subpaths() {
    let mut s = session();
    let path = add_curve(&mut s, &[[0.0, 0.0], [50.0, 0.0], [100.0, 0.0]], false);
    let count = s.document.objects.len();
    let reply = run(&mut s, "path.break", json!({ "id": path, "point": { "id": 2 }, "split_objects": false }));
    assert!(reply["new_object"].is_null());
    assert_eq!(s.document.objects.len(), count);
    let broken = curve(&s, path);
    assert_eq!(ids(&broken.contour), vec![1, 2]);
    assert_eq!(broken.holes.len(), 1);
    assert_eq!(ids(&broken.holes[0]), vec![4, 3]);
}

#[test]
fn break_closed_curve_opens_at_point_and_keeps_locus() {
    let mut s = session();
    let path = add_curve(&mut s, &[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], true);
    run(&mut s, "path.break", json!({ "id": path, "point": { "id": 2 } }));
    let opened = curve(&s, path);
    assert!(!opened.contour.closed);
    assert_eq!(ids(&opened.contour), vec![2, 3, 4, 1, 5]);
    assert_eq!(opened.contour.points[0].pos, [100.0, 0.0]);
    assert_eq!(opened.contour.points[4].pos, [100.0, 0.0]);
    assert_eq!(s.document.objects.len(), 1);
    undo(&mut s);
    assert!(curve(&s, path).contour.closed);
}

#[test]
fn break_rejects_endpoint_unknown_point_and_non_curve() {
    let mut s = session();
    let path = add_curve(&mut s, &[[0.0, 0.0], [50.0, 0.0], [100.0, 0.0]], false);
    let before = curve(&s, path);
    assert_eq!(fails(&mut s, "path.break", json!({ "id": path, "point": { "id": 1 } })), "ENDPOINT");
    assert_eq!(fails(&mut s, "path.break", json!({ "id": path, "point": { "id": 99 } })), "NOT_FOUND");
    assert_eq!(fails(&mut s, "path.break", json!({ "id": path })), "INVALID_PARAMS");
    let shape = s.document.create_object(Object { size: [10.0, 10.0], ..Object::default() }).0;
    assert_eq!(fails(&mut s, "path.break", json!({ "id": shape, "point": { "id": 1 } })), "NOT_A_CURVE");
    assert_eq!(curve(&s, path), before);
    assert!(s.run(&Command::new("edit.undo")).is_err() || curve(&s, path) == before);
}

#[test]
fn join_brief_case_welds_endpoints_into_line_0_to_100() {
    let mut s = session();
    let a = add_curve(&mut s, &[[0.0, 0.0], [50.0, 0.0]], false);
    let b = add_curve(&mut s, &[[50.0, 0.0], [100.0, 0.0]], false);
    let unrelated = add_curve(&mut s, &[[0.0, 150.0], [30.0, 150.0]], false);
    let unrelated_before = curve(&s, unrelated);
    let reply = run(&mut s, "path.join", json!({ "target": [format!("node:{a}"), format!("node:{b}")], "tolerance": "0.5pt" }));
    assert_eq!(reply["id"], json!(a));
    assert_eq!(reply["removed"], json!([b]));
    assert_eq!(reply["welded"], json!([{ "survivor": 2, "removed": 1, "object": b }]));
    assert_eq!(reply["remapped"], json!([{ "object": b, "from": 2, "to": 3 }]));
    let joined = curve(&s, a);
    assert_eq!(xs(&joined.contour), vec![0.0, 50.0, 100.0]);
    assert_eq!(ids(&joined.contour), vec![1, 2, 3]);
    assert!(s.document.get_object(ObjectId(b)).is_none());
    assert_eq!(curve(&s, unrelated), unrelated_before);
    let object = s.document.get_object(ObjectId(a)).unwrap();
    assert_eq!((object.pos, object.size), ([0.0, 0.0], [100.0, 1.0]));
    undo(&mut s);
    assert_eq!(xs(&curve(&s, a).contour), vec![0.0, 50.0]);
    assert_eq!(xs(&curve(&s, b).contour), vec![50.0, 100.0]);
    redo(&mut s);
    assert!(s.document.get_object(ObjectId(b)).is_none());
}

#[test]
fn join_orients_reversed_curves_and_bridges_gaps() {
    let mut s = session();
    let a = add_curve(&mut s, &[[40.0, 0.0], [0.0, 0.0]], false);
    let b = add_curve(&mut s, &[[100.0, 0.0], [60.0, 0.0]], false);
    let reply = run(&mut s, "path.join", json!({ "ids": [a, b], "tolerance": 1.0 }));
    assert_eq!(reply["welded"], json!([]));
    let joined = curve(&s, a);
    assert_eq!(xs(&joined.contour), vec![0.0, 40.0, 60.0, 100.0]);
    assert!(!joined.contour.closed);
}

#[test]
fn join_uses_selection_and_rejects_bad_targets() {
    let mut s = session();
    let a = add_curve(&mut s, &[[0.0, 0.0], [50.0, 0.0]], false);
    let b = add_curve(&mut s, &[[50.0, 0.0], [100.0, 0.0]], false);
    let closed = add_curve(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], true);
    assert_eq!(fails(&mut s, "path.join", json!({ "ids": [a] })), "NEED_TWO_CURVES");
    assert_eq!(fails(&mut s, "path.join", json!({ "ids": [a, closed] })), "NOT_OPEN");
    assert_eq!(fails(&mut s, "path.join", json!({ "ids": [a, a] })), "INVALID_PARAMS");
    assert_eq!(fails(&mut s, "path.join", json!({ "ids": [a, b], "tolerance": "abc" })), "INVALID_PARAMS");
    s.selection.ids = vec![ObjectId(a), ObjectId(b)];
    run(&mut s, "path.join", json!({}));
    assert_eq!(xs(&curve(&s, a).contour), vec![0.0, 50.0, 100.0]);
    assert_eq!(s.selection.ids, vec![ObjectId(a)]);
}

#[test]
fn join_plan_keeps_text_on_survivor_and_ends_other_text() {
    let mut s = session();
    let a = add_curve(&mut s, &[[0.0, 0.0], [50.0, 0.0]], false);
    let b = add_curve(&mut s, &[[50.0, 0.0], [100.0, 0.0]], false);
    let text_a = s.document.create_object(Object { kind: ObjectKind::PathText(PathTextData { path: ObjectId(a), story: StoryId(7), ..PathTextData::default() }), ..Object::default() }).0;
    let text_b = s.document.create_object(Object { kind: ObjectKind::PathText(PathTextData { path: ObjectId(b), story: StoryId(8), ..PathTextData::default() }), ..Object::default() }).0;
    s.document.stories.push(Story { id: StoryId(7), text: "a".into(), ..Story::default() });
    s.document.stories.push(Story { id: StoryId(8), text: "b".into(), ..Story::default() });
    run(&mut s, "path.join", json!({ "ids": [a, b] }));
    assert!(s.document.get_object(ObjectId(text_a)).is_some());
    assert!(s.document.get_object(ObjectId(text_b)).is_none());
    assert!(s.document.stories.iter().any(|story| story.id == StoryId(7)));
    assert!(!s.document.stories.iter().any(|story| story.id == StoryId(8)));
    undo(&mut s);
    assert!(s.document.get_object(ObjectId(text_b)).is_some());
    assert!(s.document.stories.iter().any(|story| story.id == StoryId(8)));
}

#[test]
fn break_plan_keeps_path_text_on_first_part() {
    let mut s = session();
    let path = add_curve(&mut s, &[[0.0, 0.0], [50.0, 0.0], [100.0, 0.0]], false);
    let text = s.document.create_object(Object { kind: ObjectKind::PathText(PathTextData { path: ObjectId(path), story: StoryId(3), ..PathTextData::default() }), ..Object::default() }).0;
    s.document.stories.push(Story { id: StoryId(3), text: "t".into(), ..Story::default() });
    let reply = run(&mut s, "path.break", json!({ "id": path, "point": { "id": 2 } }));
    assert_eq!(reply["path_text_kept"], json!(true));
    match &s.document.get_object(ObjectId(text)).unwrap().kind {
        ObjectKind::PathText(data) => assert_eq!(data.path, ObjectId(path)),
        _ => panic!("text kind changed"),
    }
    assert!(s.document.stories.iter().any(|story| story.id == StoryId(3)));
}
fn curve_add(session: &mut Session, points: &[[f64; 2]]) -> u64 {
    let list: Vec<Value> = points.iter().map(|p| json!([p[0], p[1]])).collect();
    run(session, "curve.add", json!({ "points": list }))["id"].as_u64().expect("curve.add id")
}

#[test]
fn break_and_join_keep_world_positions_of_offset_curves_from_curve_add() {
    let mut s = session();
    let path = curve_add(&mut s, &[[200.0, 40.0], [250.0, 40.0], [300.0, 40.0]]);
    assert_eq!(s.document.get_object(ObjectId(path)).unwrap().pos, [200.0, 40.0]);
    assert_eq!(local(&s, path).contour.points[0].pos, [0.0, 0.0]);
    let reply = run(&mut s, "path.break", json!({ "id": path, "point": { "id": 2 } }));
    let piece = reply["new_object"].as_u64().unwrap();
    assert_eq!(xs(&curve(&s, path).contour), vec![200.0, 250.0]);
    assert_eq!(xs(&curve(&s, piece).contour), vec![250.0, 300.0]);
    assert_eq!(s.document.get_object(ObjectId(piece)).unwrap().pos, [250.0, 40.0]);
    assert_eq!(local(&s, piece).contour.points[0].pos, [0.0, 0.0]);
    run(&mut s, "path.join", json!({ "ids": [path, piece] }));
    let joined = curve(&s, path);
    assert_eq!(xs(&joined.contour), vec![200.0, 250.0, 300.0]);
    assert!(joined.contour.points.iter().all(|p| p.pos[1] == 40.0));
    assert_eq!(s.document.get_object(ObjectId(path)).unwrap().pos, [200.0, 40.0]);
    let mut nodes = Command::new("curve.nodes.set");
    nodes.params = json!({ "id": path, "contour": { "points": [[200.0, 40.0], [300.0, 40.0]] } });
    s.run(&nodes).expect("curve.nodes.set still accepts joined curve");
}

#[test]
fn break_rejects_rotated_curve() {
    let mut s = session();
    let path = curve_add(&mut s, &[[0.0, 0.0], [50.0, 0.0], [100.0, 0.0]]);
    s.document.find_object_mut(path).unwrap().rotation_deg = 30.0;
    assert_eq!(fails(&mut s, "path.break", json!({ "id": path, "point": { "id": 2 } })), "INVALID_PARAMS");
}