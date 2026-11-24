#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;

#[test]
fn shell_tools_column_draws_cached_icon_textures() {
    let _home = test_home::enter("shell_tools_column_draws_cached_icon_textures");
    let mut shell = AppShell::new();
    let doc = shell.workspace.new_document("icons shell");
    shell.workspace.activate(doc).unwrap();
    shell.persona_tools = dtool_ui::persona::live_tools().expect("default persona tools");
    assert!(shell.tools_column_visible());
    assert!(shell.icon_cache.is_empty());
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show_tools_column(ui));
    out.textures_delta.clear();
    assert!(!shell.icon_cache.is_empty());
    let first = shell.icon_cache.renders();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show_tools_column(ui));
    out.textures_delta.clear();
    assert_eq!(shell.icon_cache.renders(), first);
}
