#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;
use dtool_ui::tools_panel::tool_column_ids;

#[test]
fn tool_column_opens_only_active_flyout() {
    let _home = test_home::enter("tool_column_opens_only_active_flyout");
    let closed = tool_column_ids("tool.move");
    assert!(closed.contains(&"tool.move") && closed.contains(&"tool.shapes"));
    assert!(!closed.contains(&"tool.shape.ellipse"));
    let open = tool_column_ids("tool.shape.rectangle");
    assert!(open.contains(&"tool.shape.ellipse"));
    assert!(!open.contains(&"tool.pen.pencil"));
}

#[test]
fn pick_tool_sets_active_session_tool() {
    let _home = test_home::enter("pick_tool_sets_active_session_tool");
    let mut shell = AppShell::new();
    assert!(shell.pick_tool("tool.move").is_err() || shell.workspace.session().is_some());
    let id = shell.workspace.new_document("tools");
    shell.workspace.activate(id).unwrap();
    shell.pick_tool("tool.shape.rectangle").unwrap();
    assert_eq!(shell.workspace.session().unwrap().active_tool, "tool.shape.rectangle");
}

#[test]
fn regions_render_headless() {
    let _home = test_home::enter("regions_render_headless");
    let ctx = egui::Context::default();
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("regions");
    shell.workspace.activate(id).unwrap();
    for _ in 0..2 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            shell.show_context_region(ui);
            shell.show_tools_column(ui);
        });
        output.textures_delta.clear();
    }
}
