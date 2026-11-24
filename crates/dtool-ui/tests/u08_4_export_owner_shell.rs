#[path = "support/test_home.rs"]
mod test_home;
#[test]
fn shell_export_dialog_stays_on_opening_document() {
    let _home = test_home::enter("shell_export_dialog_stays_on_opening_document");
    let mut shell = dtool_ui::shell::AppShell::new();
    let a = shell.workspace.new_document("a");
    let b = shell.workspace.new_document("b");
    shell.workspace.activate(a).unwrap();
    shell.open_export_dialog();
    assert!(dtool_ui::shell::export_dialog().lock().unwrap().is_open());
    shell.workspace.activate(b).unwrap();
    assert_eq!(dtool_ui::shell::export_dialog().lock().unwrap().owner(), Some(a));
    shell.workspace.close_with(a, dtool_core::workspace::CloseAnswer::No).unwrap();
    shell.drop_orphan_dialogs();
    assert!(!dtool_ui::shell::export_dialog().lock().unwrap().is_open());
    assert_eq!(shell.status_bar.hint_key, "error.context_not_found");
}