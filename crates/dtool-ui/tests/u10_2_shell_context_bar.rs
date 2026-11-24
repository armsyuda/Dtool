#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::selection::SelectOp;
use dtool_core::Object;
use dtool_protocol::Command;
use dtool_ui::context_bar::{ContextBarModel, ContextEdit, ControlState};
use dtool_ui::shell::AppShell;
use serde_json::json;

fn shell_with(tool: &str) -> AppShell {
    let mut shell = AppShell::new();
    shell.workspace.new_document("u10");
    shell.workspace.session_mut().unwrap().run_named("ui.tool.set", json!({ "id": tool })).unwrap();
    shell
}

fn ids(shell: &mut AppShell) -> Vec<String> {
    shell.context_bar_model().unwrap().ids().into_iter().map(String::from).collect()
}

#[test]
fn shell_bar_ids_match_mcp_query() {
    let _home = test_home::enter("shell_bar_ids_match_mcp_query");
    for tool in ["tool.move", "tool.zoom", "tool.shape.polygon"] {
        let mut shell = shell_with(tool);
        let rendered = ids(&mut shell);
        let query = shell.run_and_reconcile(Command::with_params("ui.context_toolbar.query", json!({}))).unwrap();
        let queried: Vec<String> = ContextBarModel::from_query(&query).ids().into_iter().map(String::from).collect();
        assert_eq!(rendered, queried, "{tool}");
        assert!(!rendered.is_empty());
    }
    let mut moving = shell_with("tool.move");
    assert!(ids(&mut moving).iter().all(|id| id.starts_with("move.")));
    let mut zoom = shell_with("tool.zoom");
    assert!(ids(&mut zoom).iter().all(|id| id.starts_with("zoom.")));
}

#[test]
fn shell_bar_align_left_runs_through_run_and_reconcile() {
    let _home = test_home::enter("shell_bar_align_left_runs_through_run_and_reconcile");
    let mut shell = shell_with("tool.move");
    let session = shell.workspace.session_mut().unwrap();
    let a = session.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], ..Object::default() });
    let b = session.document.create_object(Object { name: "b".into(), pos: [30.0, 5.0], size: [10.0, 10.0], ..Object::default() });
    session.selection.set(vec![a, b], SelectOp::Replace);
    let stamp = shell.context_bar_stamp();
    let model = shell.context_bar_model().unwrap().clone();
    assert_eq!(model.item("move.align.left").unwrap().state, ControlState::Enabled);
    let command = model.bind(&ContextEdit { id: "move.align.left".into(), value: json!(null) }, stamp).unwrap();
    assert_eq!(command.name, "layer.arrange.align.left");
    let before = shell.workspace.session().unwrap().generation;
    let refreshes = shell.refresh_count();
    shell.apply_context_command(command).unwrap();
    let session = shell.workspace.session().unwrap();
    assert_eq!(session.generation, before + 1);
    assert_eq!(session.document.get_object(b).unwrap().pos[0], 0.0);
    assert_eq!(shell.refresh_count(), refreshes + 1);
    assert!(model.bind(&ContextEdit { id: "move.align.left".into(), value: json!(null) }, shell.context_bar_stamp()).is_none());
}

#[test]
fn shell_bar_model_rebuilds_only_on_change() {
    let _home = test_home::enter("shell_bar_model_rebuilds_only_on_change");
    let mut shell = shell_with("tool.shape.polygon");
    let first = shell.context_bar_model().unwrap().stamp;
    assert_eq!(shell.context_bar_model().unwrap().stamp, first);
    shell.workspace.session_mut().unwrap().run_named("ui.tool.set", json!({ "id": "tool.move" })).unwrap();
    assert!(ids(&mut shell).iter().all(|id| id.starts_with("move.")));
}

#[test]
fn shell_context_region_renders_headless() {
    let _home = test_home::enter("shell_context_region_renders_headless");
    let mut shell = shell_with("tool.move");
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| shell.show_context_region(ui));
    output.textures_delta.clear();
}