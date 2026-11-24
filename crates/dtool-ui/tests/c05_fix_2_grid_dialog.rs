use dtool_core::grid::GridKind;
use dtool_core::workspace::Workspace;
use dtool_ui::grid_dialog::{
    angles_from_grid_value, grid_query_value, origin_from_grid_value, with_origin_angles, GridAndAxisDialog,
};
use serde_json::json;

#[test]
fn origin_and_axonometric_angles_survive_grid_set_undo() {
    let mut workspace = Workspace::default();
    workspace.new_document("c05");
    let mut dialog = GridAndAxisDialog::default();
    dialog.open_from_session(workspace.session_mut().unwrap());
    assert_eq!(dialog.origin, [0.0, 0.0]);
    assert_eq!(dialog.angles, [30.0, 30.0]);
    dialog.draft.kind = GridKind::Axonometric;
    dialog.origin = [12.0, 24.0];
    dialog.angles = [15.0, 25.0];
    let params = with_origin_angles(json!({ "kind": "Axonometric" }), dialog.origin, dialog.angles);
    assert_eq!(params["origin"], json!([12.0, 24.0]));
    assert_eq!(params["angles"], json!([15.0, 25.0]));
    assert!(dialog.apply(workspace.session_mut().unwrap()));
    let grid = grid_query_value(workspace.session_mut().unwrap());
    assert_eq!(origin_from_grid_value(&grid), [12.0, 24.0]);
    assert_eq!(angles_from_grid_value(&grid), [15.0, 25.0]);
    assert_eq!(grid["kind"], json!("Axonometric"));
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    let after = grid_query_value(workspace.session_mut().unwrap());
    assert_eq!(origin_from_grid_value(&after), [0.0, 0.0]);
    assert_eq!(angles_from_grid_value(&after), [30.0, 30.0]);
}

#[test]
fn origin_and_axonometric_angle_fields_build_grid_set_params() {
    let mut dialog = GridAndAxisDialog::default();
    dialog.draft.kind = GridKind::Axonometric;
    dialog.origin = [12.0, 24.0];
    dialog.angles = [15.0, 25.0];
    let params = with_origin_angles(json!({ "kind": "Axonometric" }), dialog.origin, dialog.angles);
    assert_eq!(params["kind"], json!("Axonometric"));
    assert_eq!(params["origin"], json!([12.0, 24.0]));
    assert_eq!(params["angles"], json!([15.0, 25.0]));
    assert_eq!(origin_from_grid_value(&params), [12.0, 24.0]);
    assert_eq!(angles_from_grid_value(&params), [15.0, 25.0]);
}
