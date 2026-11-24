use dtool_core::command::Session;
use dtool_core::commands::curve_edit::{world_contours, CURVE_NODES_SET};
use dtool_core::commands::curves::SHAPE_CONVERT;
use dtool_core::document::Units;
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::selection::SelectOp;
use dtool_core::Document;
use dtool_protocol::Command;
use dtool_ui::canvas_curves::{NodeEdit, NodePress};
use serde_json::json;

const R: f32 = 4.0;

fn session_with(object: Object) -> (Session, ObjectId) {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    session.has_document = true;
    let id = session.document.create_object(object);
    session.selection.set(vec![id], SelectOp::Replace);
    (session, id)
}

fn rect() -> (Session, ObjectId) {
    session_with(Object { pos: [10.0, 20.0], size: [100.0, 50.0], ..Object::default() })
}

fn is_shape(session: &Session, id: ObjectId) -> bool {
    matches!(session.document.get_object(id).unwrap().kind, ObjectKind::Shape)
}

fn drag(nodes: &mut NodeEdit, session: &Session, from: [f32; 2], to: [f32; 2]) -> Option<Command> {
    assert!(matches!(nodes.press(session, from, false, false, R), NodePress::Grab));
    nodes.drag(to);
    nodes.release()
}

fn first_node(session: &Session, nodes: &NodeEdit) -> [f32; 2] {
    let p = nodes.contours(session).unwrap()[0].points[0].pos;
    [p[0] as f32, p[1] as f32]
}

#[test]
fn activation_and_selection_change_nothing() {
    let (session, id) = rect();
    let generation = session.generation;
    let mut nodes = NodeEdit::default();
    nodes.sync(&session);
    assert_eq!(nodes.target, Some(id));
    assert!(nodes.primitive);
    let contours = nodes.contours(&session).unwrap();
    assert!(contours[0].points.len() >= 4);
    let p = first_node(&session, &nodes);
    assert!(matches!(nodes.press(&session, p, false, false, R), NodePress::Grab));
    assert_eq!(nodes.selected, vec![(0, 0)]);
    assert!(nodes.release().is_none());
    assert!(nodes.select_at(&session, p, R));
    assert!(!nodes.claims_delete());
    assert!(nodes.delete_commands(&session).is_empty());
    assert!(nodes.break_command(&session).is_none());
    assert!(is_shape(&session, id));
    assert_eq!(session.generation, generation);
    assert!(!session.undo.can_undo());
}

#[test]
fn first_drag_converts_and_edits_in_one_undo_step() {
    let (mut session, id) = rect();
    let before = session.document.get_object(id).unwrap().clone();
    let mut nodes = NodeEdit::default();
    nodes.sync(&session);
    let p = first_node(&session, &nodes);
    let command = drag(&mut nodes, &session, p, [p[0] - 5.0, p[1] - 7.0]).unwrap();
    assert_eq!(command.name, SHAPE_CONVERT);
    let reply = session.run(&command).unwrap();
    assert_eq!(reply["edited"], json!(true));
    let object = session.document.get_object(id).unwrap();
    let contour = &world_contours(object)[0];
    let moved = contour.points[0].pos;
    assert!((moved[0] - f64::from(p[0] - 5.0)).abs() < 1e-3 && (moved[1] - f64::from(p[1] - 7.0)).abs() < 1e-3, "{moved:?}");
    assert!(object.curve().is_some());
    assert_eq!(session.undo.applied_steps(), 1);
    session.run(&Command::new("edit.undo")).unwrap();
    let restored = session.document.get_object(id).unwrap();
    assert!(matches!(restored.kind, ObjectKind::Shape));
    assert_eq!(restored.pos, before.pos);
    assert_eq!(restored.size, before.size);
    assert_eq!(restored.shape, before.shape);
    assert!(!session.undo.can_undo());
}

#[test]
fn second_edit_is_a_normal_curve_edit() {
    let (mut session, id) = rect();
    let mut nodes = NodeEdit::default();
    nodes.sync(&session);
    let p = first_node(&session, &nodes);
    let first = drag(&mut nodes, &session, p, [p[0] - 5.0, p[1]]).unwrap();
    session.run(&first).unwrap();
    nodes.sync(&session);
    assert!(!nodes.primitive);
    let q = {
        let n = &nodes.contours(&session).unwrap()[0].points[1];
        [n.pos[0] as f32, n.pos[1] as f32]
    };
    let second = drag(&mut nodes, &session, q, [q[0], q[1] + 9.0]).unwrap();
    assert_eq!(second.name, CURVE_NODES_SET);
    session.run(&second).unwrap();
    assert_eq!(session.undo.applied_steps(), 2);
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(session.document.get_object(id).unwrap().curve().is_some());
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(is_shape(&session, id));
}

#[test]
fn existing_curve_is_unaffected() {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    session.has_document = true;
    let reply = session.run(&Command::with_params("curve.add", json!({ "points": [[10.0, 10.0], [100.0, 10.0], [100.0, 80.0]], "closed": true }))).unwrap();
    let id = ObjectId(reply["id"].as_u64().unwrap());
    let steps = session.undo.applied_steps();
    let mut nodes = NodeEdit::default();
    nodes.sync(&session);
    assert_eq!(nodes.target, Some(id));
    assert!(!nodes.primitive);
    let command = drag(&mut nodes, &session, [10.0, 10.0], [20.0, 15.0]).unwrap();
    assert_eq!(command.name, CURVE_NODES_SET);
    session.run(&command).unwrap();
    assert_eq!(session.undo.applied_steps(), steps + 1);
    assert_eq!(world_contours(session.document.get_object(id).unwrap())[0].points[0].pos, [20.0, 15.0]);
}

#[test]
fn rotated_primitive_stays_blocked() {
    let (session, _) = session_with(Object { pos: [10.0, 20.0], size: [100.0, 50.0], rotation_deg: 30.0, ..Object::default() });
    let mut nodes = NodeEdit::default();
    nodes.sync(&session);
    assert_eq!(nodes.target, None);
    assert!(nodes.contours(&session).is_none());
}

#[test]
fn convert_with_contours_rejects_bad_requests_without_change() {
    let (mut session, id) = rect();
    let other = session.document.create_object(Object { pos: [200.0, 200.0], size: [20.0, 20.0], ..Object::default() });
    let contours = json!([{ "points": [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], "closed": true }]);
    assert!(session.run(&Command::with_params(SHAPE_CONVERT, json!({ "ids": [id.0, other.0], "contours": contours }))).is_err());
    assert!(session.run(&Command::with_params(SHAPE_CONVERT, json!({ "id": id.0, "contours": [] }))).is_err());
    assert!(is_shape(&session, id) && is_shape(&session, other));
    assert!(!session.undo.can_undo());
}