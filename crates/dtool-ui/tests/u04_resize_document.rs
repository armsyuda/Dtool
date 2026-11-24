#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;

#[test]
fn resize_document_menu_opens_dialog_for_active_document() {
    let _home = test_home::enter("resize_document_menu_opens_dialog_for_active_document");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("u04");
    shell.open_resize_document();
    assert!(shell.resize_document.open);
    assert_eq!(shell.resize_document_owner, Some(id));
}

#[test]
fn resize_document_without_document_stays_closed() {
    let _home = test_home::enter("resize_document_without_document_stays_closed");
    let mut shell = AppShell::new();
    shell.open_resize_document();
    assert!(!shell.resize_document.open);
    assert_eq!(shell.resize_document_owner, None);
}
