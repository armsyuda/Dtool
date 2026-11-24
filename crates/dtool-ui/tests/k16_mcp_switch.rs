#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::prefs::Prefs;
use dtool_core::{Command, Workspace};
use dtool_ui::headless::create_headless_ui_harness;
use dtool_ui::shell::{AppShell, CONSOLE_MENU_ID};

#[test]
fn prefs_mcp_enabled_drives_the_shared_connection() {
    let _home = test_home::enter("prefs_mcp_enabled_drives_the_shared_connection");
    let shell = AppShell::new();
    let mut prefs = Prefs::default();
    prefs.mcp_enabled = false;
    shell.sync_mcp_switch(&prefs);
    assert!(!shell.ai_connection().enabled());
    assert!(!shell.workspace.ai_connection().enabled());
    prefs.mcp_enabled = true;
    assert!(shell.sync_mcp_switch(&prefs));
    assert!(shell.ai_connection().enabled());
    assert!(!shell.sync_mcp_switch(&prefs));
}

#[test]
fn headless_harness_runs_on_shared_workspace() {
    let _home = test_home::enter("headless_harness_runs_on_shared_workspace");
    let shared = std::sync::Arc::new(std::sync::Mutex::new(Workspace::default()));
    let mut harness = create_headless_ui_harness(shared);
    assert!(harness.run_command(Command::with_params("app.about", serde_json::json!({}))).is_ok());
}

#[test]
fn console_menu_id_is_catalogued() {
    let _home = test_home::enter("console_menu_id_is_catalogued");
    assert_eq!(CONSOLE_MENU_ID, "window.scripting.log_console");
    let shell = AppShell::new();
    assert!(!shell.console_open);
}
