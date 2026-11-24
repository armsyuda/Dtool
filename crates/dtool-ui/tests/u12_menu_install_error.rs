#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;

#[test]
fn failed_menu_install_is_reported_not_silent() {
    let _home = test_home::enter("failed_menu_install_is_reported_not_silent");
    let shell = AppShell::new();
    let missing = dtool_ui::menu_tree::missing_b11_keys();
    assert_eq!(shell.startup_errors.is_empty(), missing.is_empty());
    if !missing.is_empty() {
        assert!(shell.startup_errors[0].contains(&missing[0]));
        assert_eq!(shell.status_bar.hint_key, "error.internal_error");
    }
}

#[test]
fn note_menu_install_records_error_and_hint() {
    let _home = test_home::enter("note_menu_install_records_error_and_hint");
    let mut shell = AppShell::new();
    shell.startup_errors.clear();
    shell.note_menu_install(Vec::new());
    assert!(shell.startup_errors.is_empty());
    shell.note_menu_install(vec!["tooltip.x".to_string()]);
    assert_eq!(shell.startup_errors, vec!["menu_provider_missing: tooltip.x".to_string()]);
    assert_eq!(shell.status_bar.hint_key, "error.internal_error");
}