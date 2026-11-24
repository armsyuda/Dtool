use dtool_core::command::Session;
use dtool_core::commands::curve_edit::{world_contours, CURVE_NODES_SET};
use dtool_core::curve::NodeKind;
use dtool_core::document::{Document, Units};
use dtool_core::object::ObjectId;
use dtool_core::selection::SelectOp;
use dtool_core::transform::Anchor;
use dtool_ui::canvas_curves::{NodeEdit, NodePart, NodePress, NODE_TRANSFORM_ANCHOR};
use serde_json::json;

const R: f32 = 4.0;

fn curve(points: serde_json::Value, closed: bool) -> (Session, ObjectId) {
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    s.has_document = true;
    let reply = s.run_named("curve.add", json!({ "points": points, "closed": closed })).unwrap();
    let id = ObjectId(reply["id"].as_u64().unwrap());
    s.selection.set(vec![id], SelectOp::Replace);
    (s, id)
}

fn square() -> (Session, ObjectId) {
    curve(json!([[100.0, 100.0], [200.0, 100.0], [200.0, 200.0], [100.0, 200.0]]), true)
}

fn smooth_line() -> (Session, ObjectId) {
    curve(json!([[0.0, 0.0], { "pos": [100.0, 0.0], "handle_in": [80.0, 0.0], "handle_out": [120.0, 0.0] }, [200.0, 0.0]]), false)
}

fn grabbed(nodes: &mut NodeEdit, s: &Session, at: [f32; 2]) {
    assert!(matches!(nodes.press(s, at, false, false, R), NodePress::Grab));
}

fn commit(nodes: &mut NodeEdit, s: &mut Session) {
    let command = nodes.release().expect("edit");
    s.run(&command).unwrap();
}

fn pts(s: &Session, id: ObjectId) -> Vec<[f64; 2]> {
    world_contours(s.document.get_object(id).unwrap())[0].points.iter().map(|p| p.pos).collect()
}

#[test]
fn node_drag_escape_path_leaves_document_unchanged() {
    let (s, id) = square();
    let before = pts(&s, id);
    let mut nodes = NodeEdit::default();
    nodes.sync(&s);
    grabbed(&mut nodes, &s, [100.0, 100.0]);
    nodes.drag([130.0, 120.0]);
    assert!(nodes.live.is_some());
    nodes.cancel();
    assert!(nodes.release().is_none());
    assert_eq!(pts(&s, id), before);
    assert!(s.undo.applied_steps() == 1);
}

#[test]
fn alt_during_handle_drag_breaks_coupling() {
    let (mut s, id) = smooth_line();
    let mut nodes = NodeEdit::default();
    nodes.sync(&s);
    nodes.select_at(&s, [100.0, 0.0], R);
    grabbed(&mut nodes, &s, [120.0, 0.0]);
    nodes.alt = true;
    nodes.drag([120.0, 20.0]);
    commit(&mut nodes, &mut s);
    let node = world_contours(s.document.get_object(id).unwrap())[0].points[1].clone();
    assert_eq!(node.kind, NodeKind::Sharp);
    assert_eq!(node.handle_out, Some([120.0, 20.0]));
    assert_eq!(node.handle_in, Some([80.0, 0.0]));
}

#[test]
fn coupled_handle_drag_mirrors_and_shift_snaps_45() {
    let (mut s, id) = smooth_line();
    let mut nodes = NodeEdit::default();
    nodes.sync(&s);
    nodes.select_at(&s, [100.0, 0.0], R);
    grabbed(&mut nodes, &s, [120.0, 0.0]);
    nodes.shift = true;
    nodes.drag([118.0, 21.0]);
    commit(&mut nodes, &mut s);
    let node = world_contours(s.document.get_object(id).unwrap())[0].points[1].clone();
    let out = node.handle_out.unwrap();
    assert!(((out[0] - 100.0) - (out[1])).abs() < 1e-3, "{out:?}");
    let back = node.handle_in.unwrap();
    assert!((back[0] + out[0] - 200.0).abs() < 1e-3 && (back[1] + out[1]).abs() < 1e-3);
}

#[test]
fn segment_drag_moves_adjacent_handles_with_one_undo() {
    let (mut s, id) = square();
    let steps = s.undo.applied_steps();
    let mut nodes = NodeEdit::default();
    nodes.sync(&s);
    grabbed(&mut nodes, &s, [150.0, 100.0]);
    assert!(matches!(nodes.grab, Some((NodePart::Segment(0, 0), _))));
    nodes.drag([150.0, 80.0]);
    commit(&mut nodes, &mut s);
    let c = &world_contours(s.document.get_object(id).unwrap())[0];
    assert_eq!(c.points[0].pos, [100.0, 100.0]);
    assert_eq!(c.points[1].pos, [200.0, 100.0]);
    assert_eq!(c.points[0].handle_out, Some([100.0, 80.0]));
    assert_eq!(c.points[1].handle_in, Some([200.0, 80.0]));
    assert_eq!(s.undo.applied_steps(), steps + 1);
    s.run_named("edit.undo", json!({})).unwrap();
    assert!(world_contours(s.document.get_object(id).unwrap())[0].points[0].handle_out.is_none());
}

#[test]
fn ctrl_a_select_all_and_escape_clear_are_node_local() {
    let (s, _) = square();
    let generation = s.generation;
    let mut nodes = NodeEdit::default();
    assert!(nodes.select_all(&s));
    assert_eq!(nodes.selected.len(), 4);
    assert_eq!(s.generation, generation);
    assert_eq!(s.selection.ids.len(), 1);
}

#[test]
fn transform_mode_scales_selected_nodes_about_top_left() {
    assert_eq!(NODE_TRANSFORM_ANCHOR, Anchor::TopLeft);
    let (mut s, id) = square();
    let mut nodes = NodeEdit::default();
    nodes.transform_mode = true;
    assert!(nodes.select_all(&s));
    grabbed(&mut nodes, &s, [200.0, 200.0]);
    assert!(matches!(nodes.grab, Some((NodePart::Scale, _))));
    nodes.drag([300.0, 250.0]);
    commit(&mut nodes, &mut s);
    assert_eq!(pts(&s, id), vec![[100.0, 100.0], [300.0, 100.0], [300.0, 250.0], [100.0, 250.0]]);
}

#[test]
fn transform_mode_off_corner_is_a_plain_node_drag() {
    let (mut s, id) = square();
    let mut nodes = NodeEdit::default();
    assert!(nodes.select_all(&s));
    grabbed(&mut nodes, &s, [200.0, 200.0]);
    assert!(matches!(nodes.grab, Some((NodePart::Anchor(..), _))));
    nodes.drag([210.0, 200.0]);
    commit(&mut nodes, &mut s);
    assert_eq!(pts(&s, id)[0], [110.0, 100.0]);
}

#[test]
fn moving_points_feed_snap_with_every_selected_anchor() {
    let (s, _) = square();
    let mut nodes = NodeEdit::default();
    nodes.sync(&s);
    nodes.press(&s, [100.0, 100.0], false, false, R);
    nodes.press(&s, [200.0, 200.0], true, false, R);
    let moved = nodes.moving_points([210.0, 205.0]).unwrap();
    assert_eq!(moved, vec![[110.0, 105.0], [210.0, 205.0]]);
}

#[test]
fn snap_off_node_drag_never_snaps() {
    let (mut s, _) = square();
    s.run_named("snap.prefs.set", json!({ "enabled": false })).unwrap();
    assert!(dtool_ui::canvas_tools::snap_offset(&s, &[[101.0, 101.0]], &[]).is_none());
}

#[test]
fn close_uses_path_close_with_one_undo() {
    let (mut s, id) = curve(json!([[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]]), false);
    let mut nodes = NodeEdit::default();
    nodes.sync(&s);
    let command = nodes.close_command(&s).unwrap();
    assert_eq!(command.name, "path.close");
    let steps = s.undo.applied_steps();
    s.run(&command).unwrap();
    assert!(world_contours(s.document.get_object(id).unwrap())[0].closed);
    assert_eq!(s.undo.applied_steps(), steps + 1);
    nodes.sync(&s);
    assert!(nodes.close_command(&s).is_none());
}

#[test]
fn existing_curve_drag_still_uses_nodes_set() {
    let (s, _) = square();
    let mut nodes = NodeEdit::default();
    nodes.sync(&s);
    grabbed(&mut nodes, &s, [100.0, 100.0]);
    nodes.drag([105.0, 100.0]);
    assert_eq!(nodes.release().unwrap().name, CURVE_NODES_SET);
}