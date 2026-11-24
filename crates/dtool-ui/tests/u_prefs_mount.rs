#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::menu_tree::shell_action;
use dtool_ui::shell::{AppShell, PREFERENCES_MENU_ID};

#[test]
fn shell_menu_ids_are_live_actions() {
    let _home = test_home::enter("shell_menu_ids_are_live_actions");
    for id in ["edit.settings", "window.scripting.log_console", "view.snapping", "view.guides_manager", "help.feature_finder"] {
        assert!(shell_action(id), "{id}");
    }
    assert!(!shell_action("edit.cut"));
    assert_eq!(PREFERENCES_MENU_ID, "edit.settings");
}

#[test]
fn prefs_mcp_switch_reaches_connection() {
    let _home = test_home::enter("prefs_mcp_switch_reaches_connection");
    let mut shell = AppShell::new();
    shell.prefs.mcp_enabled = false;
    shell.sync_mcp_switch(&shell.prefs.clone());
    assert!(!shell.ai_connection().enabled());
    shell.prefs.mcp_enabled = true;
    shell.sync_mcp_switch(&shell.prefs.clone());
    assert!(shell.ai_connection().enabled());
    assert!(!shell.prefs_dialog.open);
}
