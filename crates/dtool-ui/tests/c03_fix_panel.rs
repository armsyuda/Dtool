use dtool_core::transform::{FlipAxis, RotateDirection};
use dtool_core::workspace::Workspace;
use dtool_ui::transform_panel::{PanelEdit, TransformPanel};
use serde_json::json;

fn primed() -> (Workspace, u64, u64) {
    let mut workspace = Workspace::default();
    workspace.new_document("c03");
    let a = workspace
        .session_mut()
        .unwrap()
        .run_named("object.create", json!({"name": "a", "pos": [10.0, 20.0], "size": [40.0, 30.0]}))
        .unwrap();
    let b = workspace
        .session_mut()
        .unwrap()
        .run_named("object.create", json!({"name": "b", "pos": [80.0, 20.0], "size": [80.0, 30.0]}))
        .unwrap();
    let a = a.get("id").and_then(|v| v.as_u64()).unwrap_or_else(|| workspace.session().unwrap().document.objects.iter().find(|o| o.name == "a").unwrap().id.0);
    let b = b.get("id").and_then(|v| v.as_u64()).unwrap_or_else(|| workspace.session().unwrap().document.objects.iter().find(|o| o.name == "b").unwrap().id.0);
    workspace
        .session_mut()
        .unwrap()
        .run_named("selection.set", json!({"ids": [a, b], "op": "Replace"}))
        .unwrap();
    (workspace, a, b)
}

fn obj(workspace: &Workspace, id: u64) -> ( [f32; 2], [f32; 2], f32, f32 ) {
    let o = workspace.session().unwrap().document.get_object(dtool_core::object::ObjectId(id)).unwrap();
    (o.pos, o.size, o.rotation_deg, o.shear_deg)
}

fn apply(panel: &mut TransformPanel, workspace: &mut Workspace) {
    panel.apply_edits(workspace.session_mut().unwrap());
}

fn command_name(edit: &PanelEdit) -> &'static str {
    match edit {
        PanelEdit::Set(_) => "transform.set",
        PanelEdit::SelectionBox(_) => "transform.selection_box",
        PanelEdit::Flip(_) => "transform.flip",
        PanelEdit::Rotate90(_) => "transform.rotate_90",
    }
}

#[test]
fn relative_xy_scale_rotation_call_existing_commands_and_undo_once() {
    let (mut workspace, a, _b) = primed();
    workspace.session_mut().unwrap().run_named("selection.set", json!({"ids": [a], "op": "Replace"})).unwrap();
    let before = obj(&workspace, a);
    let mut panel = TransformPanel::default();
    panel.sync_from_session(workspace.session().unwrap());

    panel.commit_relative(0, 15.0);
    assert_eq!(command_name(&panel.edits[0]), "transform.selection_box");
    apply(&mut panel, &mut workspace);
    let after_x = obj(&workspace, a);
    assert!((after_x.0[0] - (before.0[0] + 15.0)).abs() < 0.01);
    assert_eq!(after_x.1, before.1);
    assert_eq!(after_x.2, before.2);
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(obj(&workspace, a), before);

    panel.commit_relative(1, 8.0);
    apply(&mut panel, &mut workspace);
    let after_y = obj(&workspace, a);
    assert!((after_y.0[1] - (before.0[1] + 8.0)).abs() < 0.01);
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(obj(&workspace, a), before);

    panel.commit_relative(2, 200.0);
    apply(&mut panel, &mut workspace);
    let after_s = obj(&workspace, a);
    assert!((after_s.1[0] - before.1[0] * 2.0).abs() < 0.01);
    assert!((after_s.1[1] - before.1[1] * 2.0).abs() < 0.01);
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(obj(&workspace, a), before);

    panel.commit_relative(3, 30.0);
    apply(&mut panel, &mut workspace);
    let after_r = obj(&workspace, a);
    assert!((after_r.2 - 30.0).abs() < 0.01);
    assert_eq!(after_r.1, before.1);
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(obj(&workspace, a), before);
}

#[test]
fn mixed_selection_leaves_unprovided_fields_unchanged() {
    let (mut workspace, a, b) = primed();
    let before_a = obj(&workspace, a);
    let before_b = obj(&workspace, b);
    assert_ne!(before_a.1[0], before_b.1[0]);
    let mut panel = TransformPanel::default();
    panel.sync_from_session(workspace.session().unwrap());
    assert!(panel.mixed_w);
    assert!(!panel.mixed_y);
    panel.commit_relative(0, 10.0);
    let edit = panel.edits.last().unwrap();
    assert_eq!(command_name(edit), "transform.selection_box");
    if let PanelEdit::SelectionBox(params) = edit {
        assert!(params.get("scale").is_none());
        assert!(params.get("rotation_deg").is_none());
        assert!(params.get("shear_deg").is_none());
    }
    apply(&mut panel, &mut workspace);
    let after_a = obj(&workspace, a);
    let after_b = obj(&workspace, b);
    assert!((after_a.0[0] - (before_a.0[0] + 10.0)).abs() < 0.01);
    assert!((after_b.0[0] - (before_b.0[0] + 10.0)).abs() < 0.01);
    assert_eq!(after_a.1, before_a.1);
    assert_eq!(after_b.1, before_b.1);
    assert_eq!(after_a.0[1], before_a.0[1]);
    assert_eq!(after_b.0[1], before_b.0[1]);
    assert_eq!(after_a.2, before_a.2);
    assert_eq!(after_a.3, before_a.3);
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(obj(&workspace, a), before_a);
    assert_eq!(obj(&workspace, b), before_b);

    panel.commit_value(dtool_ui::transform_panel::TransformField::X, 5.0);
    assert_eq!(command_name(panel.edits.last().unwrap()), "transform.set");
    apply(&mut panel, &mut workspace);
    let set_a = obj(&workspace, a);
    let set_b = obj(&workspace, b);
    assert_eq!(set_a.1, before_a.1);
    assert_eq!(set_b.1, before_b.1);
}

#[test]
fn flip_and_rotate90_menus_undo_once() {
    let (mut workspace, a, _b) = primed();
    workspace.session_mut().unwrap().run_named("selection.set", json!({"ids": [a], "op": "Replace"})).unwrap();
    let before = obj(&workspace, a);
    let mut panel = TransformPanel::default();
    panel.sync_from_session(workspace.session().unwrap());
    assert_eq!(panel.query_command().name, "transform.query");
    panel.menu_flip(FlipAxis::Horizontal);
    assert_eq!(command_name(&panel.edits[0]), "transform.flip");
    apply(&mut panel, &mut workspace);
    assert_ne!(obj(&workspace, a), before);
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(obj(&workspace, a), before);
    panel.menu_rotate_90(RotateDirection::Left);
    assert_eq!(command_name(&panel.edits[0]), "transform.rotate_90");
    apply(&mut panel, &mut workspace);
    let rotated = obj(&workspace, a);
    assert!((rotated.2 - 90.0).abs() < 0.01 || (rotated.2 + 90.0).abs() < 0.01 || (rotated.1 != before.1));
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(obj(&workspace, a), before);
}
