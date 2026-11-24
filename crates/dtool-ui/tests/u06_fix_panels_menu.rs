#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::menu_tree::panels_action;
use dtool_ui::shell::AppShell;

#[test]
fn window_panels_rows_are_shell_routed() {
    let _home = test_home::enter("window_panels_rows_are_shell_routed");
    for id in ["window.panels.show_left", "window.panels.show_right", "window.panels.show_top", "window.panels.show_bottom", "window.panels.hide", "window.panels.reset"] {
        assert_eq!(panels_action(id), Some(id));
    }
    assert_eq!(panels_action("window.panels.add_preset"), None);
}

#[test]
fn window_panels_toggle_side_twice_restores() {
    let _home = test_home::enter("window_panels_toggle_side_twice_restores");
    let mut shell = AppShell::new();
    let before = serde_json::to_value(&shell.layout).ok();
    shell.run_panels_action("window.panels.show_right");
    shell.run_panels_action("window.panels.show_right");
    assert_eq!(serde_json::to_value(&shell.layout).ok(), before);
}
