#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;

#[test]
fn tab_hide_hides_tools_column_and_restores_it() {
    let _home = test_home::enter("tab_hide_hides_tools_column_and_restores_it");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("tools");
    shell.workspace.activate(id).unwrap();
    let status = shell.settings.status_bar_visible;
    assert!(shell.tools_column_visible());
    shell.toggle_ui();
    assert!(shell.ui_hidden());
    assert!(!shell.tools_column_visible());
    shell.toggle_ui();
    assert!(!shell.ui_hidden());
    assert!(shell.tools_column_visible());
    assert_eq!(shell.settings.status_bar_visible, status);
}

#[test]
fn tools_column_remains_visible_without_document() {
    let _home = test_home::enter("tools_column_remains_visible_without_document");
    let shell = AppShell::new();
    if shell.workspace.session().is_none() {
        assert!(shell.tools_column_visible());
    }
}
