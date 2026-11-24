#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::context_bar::{ContextEdit, ControlState};
use dtool_ui::shell::AppShell;
use serde_json::json;

fn shell_on(tool: &str, view: [f32; 2]) -> AppShell {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("a");
    shell.workspace.activate(id).unwrap();
    shell.pick_tool(tool).unwrap();
    shell.canvas_size = view;
    shell
}

#[test]
fn shell_context_bar_fit_enabled_and_fits_canvas() {
    let _home = test_home::enter("shell_context_bar_fit_enabled_and_fits_canvas");
    for (tool, id) in [("tool.zoom", "zoom.fit"), ("tool.view", "view.zoom_fit")] {
        let mut shell = shell_on(tool, [800.0, 600.0]);
        shell.workspace.session_mut().unwrap().run_named("view.zoom.set", json!({ "zoom": 7.0 })).unwrap();
        let stamp = shell.context_bar_stamp();
        let model = shell.context_bar_model().unwrap().clone();
        assert_eq!(model.item(id).unwrap().state, ControlState::Enabled, "{tool}");
        let command = model.bind(&ContextEdit { id: id.into(), value: json!(null) }, stamp).unwrap();
        shell.apply_context_pick(command).unwrap();
        let session = shell.workspace.session().unwrap();
        let page = session.document.pages.first().unwrap().size;
        let want = dtool_core::view::fit_zoom(page, [800.0, 600.0], session.view.rotation_deg);
        assert!((session.view.zoom - want).abs() < 1e-4, "{tool}");
    }
}

#[test]
fn shell_context_bar_model_refreshes_when_canvas_size_changes() {
    let _home = test_home::enter("shell_context_bar_model_refreshes_when_canvas_size_changes");
    let mut shell = shell_on("tool.zoom", [0.0, 0.0]);
    assert_eq!(shell.context_bar_model().unwrap().item("zoom.fit").unwrap().state, ControlState::Disabled);
    shell.canvas_size = [800.0, 600.0];
    assert_eq!(shell.context_bar_model().unwrap().item("zoom.fit").unwrap().state, ControlState::Enabled);
}