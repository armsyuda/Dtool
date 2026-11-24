use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::edit_session::EditEnd;
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 400.0], Units::Px, 96.0, 1))
}

fn with_slice() -> (Session, u64) {
    let mut s = session();
    let out = s.run(&Command::with_params("slice.create", json!({ "rect": [0.0, 0.0, 10.0, 10.0] }))).unwrap();
    let id = out["id"].as_u64().unwrap();
    (s, id)
}

fn slice_bounds(s: &Session) -> [f32; 4] {
    s.document.slices[0].bounds
}

fn slice_move(id: u64, x: f32) -> Command {
    Command::with_params("slice.update", json!({ "id": id, "bounds": [x, 0.0, x + 10.0, 10.0] }))
}

fn zoom_level(s: &Session) -> serde_json::Value {
    let query = dtool_core::commands::context_toolbar::query_with_state(s, "tool.zoom");
    let options = query["options"].as_array().unwrap();
    let zoom = options.iter().find(|option| option["id"] == "zoom.level").unwrap();
    zoom["value"].clone()
}

fn zoom_set(v: f64) -> Command {
    Command::with_params("ui.context_option.set", json!({ "tool": "tool.zoom", "id": "zoom.level", "value": v }))
}

#[test]
fn preview_many_then_commit_is_one_undo_step() {
    let (mut s, id) = with_slice();
    let steps = s.undo.applied_steps();
    for i in 1..=10 {
        s.preview("slice.drag", &slice_move(id, i as f32)).unwrap();
        assert_eq!(s.undo.applied_steps(), steps);
    }
    assert!(s.edit_active());
    assert_eq!(slice_bounds(&s), [10.0, 0.0, 20.0, 10.0]);
    s.end_edit(EditEnd::Commit).unwrap();
    assert!(!s.edit_active());
    assert_eq!(s.undo.applied_steps(), steps + 1);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(slice_bounds(&s), [0.0, 0.0, 10.0, 10.0]);
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(slice_bounds(&s), [10.0, 0.0, 20.0, 10.0]);
}

#[test]
fn preview_then_cancel_restores_and_pushes_nothing() {
    let (mut s, id) = with_slice();
    let steps = s.undo.applied_steps();
    for i in 1..=4 {
        s.preview("slice.drag", &slice_move(id, i as f32 * 5.0)).unwrap();
    }
    s.end_edit(EditEnd::Cancel).unwrap();
    assert_eq!(slice_bounds(&s), [0.0, 0.0, 10.0, 10.0]);
    assert_eq!(s.undo.applied_steps(), steps);
    assert!(!s.edit_active());
    s.end_edit(EditEnd::Commit).unwrap();
    assert_eq!(s.undo.applied_steps(), steps);
}

#[test]
fn option_set_drag_is_one_undoable_step() {
    let mut s = session();
    s.run(&Command::with_params("view.zoom.set", json!({ "zoom": 1.25 }))).unwrap();
    let first = zoom_level(&s);
    let steps = s.undo.applied_steps();
    for v in [150.0, 200.0, 250.0, 300.0, 350.0] {
        let mut command = zoom_set(v);
        command.params["params"] = json!({ "around": [100.0, 80.0] });
        s.preview("zoom.level", &command).unwrap();
    }
    assert_eq!(zoom_level(&s), json!(350.0));
    s.end_edit(EditEnd::Commit).unwrap();
    assert_eq!(s.undo.applied_steps(), steps + 1);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(zoom_level(&s), first);
    assert!(s.view.scroll.iter().all(|axis| axis.abs() < 0.001));
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(zoom_level(&s), json!(350.0));
}

#[test]
fn tool_switch_mid_edit_commits() {
    let (mut s, id) = with_slice();
    let steps = s.undo.applied_steps();
    s.preview("slice.drag", &slice_move(id, 3.0)).unwrap();
    s.preview("slice.drag", &slice_move(id, 6.0)).unwrap();
    s.run(&Command::with_params("ui.tool.set", json!({ "id": "tool.zoom" }))).unwrap();
    assert!(!s.edit_active());
    assert_eq!(s.undo.applied_steps(), steps + 1);
    assert_eq!(slice_bounds(&s), [6.0, 0.0, 16.0, 10.0]);
}

#[test]
fn undo_mid_edit_commits_first() {
    let (mut s, id) = with_slice();
    let steps = s.undo.applied_steps();
    s.preview("slice.drag", &slice_move(id, 3.0)).unwrap();
    s.preview("slice.drag", &slice_move(id, 7.0)).unwrap();
    assert!(s.can_execute_named("edit.undo"));
    s.run(&Command::new("edit.undo")).unwrap();
    assert!(!s.edit_active());
    assert_eq!(slice_bounds(&s), [0.0, 0.0, 10.0, 10.0]);
    assert_eq!(s.undo.applied_steps(), steps);
    assert!(s.undo.can_redo());
}

#[test]
fn different_key_commits_previous_session() {
    let (mut s, id) = with_slice();
    let steps = s.undo.applied_steps();
    s.preview("slice.drag", &slice_move(id, 2.0)).unwrap();
    s.preview("slice.drag", &slice_move(id, 4.0)).unwrap();
    s.preview("zoom.level", &zoom_set(400.0)).unwrap();
    assert_eq!(s.undo.applied_steps(), steps + 1);
    assert_eq!(s.edit.as_ref().unwrap().key, "zoom.level");
    s.end_edit(EditEnd::Commit).unwrap();
    assert_eq!(s.undo.applied_steps(), steps + 2);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(zoom_level(&s), json!(100.0));
    assert_eq!(slice_bounds(&s), [4.0, 0.0, 14.0, 10.0]);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(slice_bounds(&s), [0.0, 0.0, 10.0, 10.0]);
}

#[test]
fn view_rotate_set_preview_merge() {
    let mut s = session();
    let steps = s.undo.applied_steps();
    for d in [10.0, 20.0, 30.0, 45.0] {
        s.preview("view.rotate", &Command::with_params("view.rotate.set", json!({ "degrees": d }))).unwrap();
    }
    assert_eq!(s.view.rotation_deg, 45.0);
    assert_eq!(s.undo.applied_steps(), steps);
    s.end_edit(EditEnd::Commit).unwrap();
    assert_eq!(s.undo.applied_steps(), steps + 1);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.view.rotation_deg, 0.0);
    s.preview("view.rotate", &Command::with_params("view.rotate.set", json!({ "degrees": 90.0 }))).unwrap();
    s.end_edit(EditEnd::Cancel).unwrap();
    assert_eq!(s.view.rotation_deg, 0.0);
}

#[test]
fn workspace_activate_commits_open_edit() {
    let mut ws = dtool_core::workspace::Workspace::default();
    let a = ws.insert_context("a", None, Document::new([400.0, 400.0], Units::Px, 96.0, 1), 0);
    let b = ws.insert_context("b", None, Document::new([400.0, 400.0], Units::Px, 96.0, 1), 0);
    ws.activate(a).unwrap();
    let s = &mut ws.get_mut(a).unwrap().session;
    let steps = s.undo.applied_steps();
    s.preview("zoom.level", &zoom_set(500.0)).unwrap();
    ws.activate(b).unwrap();
    let s = &ws.get_mut(a).unwrap().session;
    assert!(!s.edit_active());
    assert_eq!(s.undo.applied_steps(), steps + 1);
}
