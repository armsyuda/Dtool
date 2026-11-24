#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::dialogs::{DialogKind, DialogOwner};

fn registry_mirrors_document_dialogs() {
    let mut shell = dtool_ui::shell::AppShell::new();
    let a = shell.workspace.new_document("a");
    let b = shell.workspace.new_document("b");
    shell.workspace.activate(a).unwrap();
    assert!(shell.open_guides_manager());
    shell.workspace.activate(b).unwrap();
    shell.sync_dialog_registry();
    assert_eq!(shell.dialogs.owner_for(DialogKind::Guides), Some(&DialogOwner::Document(a)));
    assert_eq!(shell.dialogs.kinds_for(a), vec![DialogKind::Guides]);
    shell.workspace.close_with(a, dtool_core::workspace::CloseAnswer::No).unwrap();
    shell.drop_orphan_dialogs();
    assert!(!shell.guides_dialog.open);
    assert!(shell.dialogs.kinds_for(a).is_empty());
    assert_eq!(shell.status_bar.hint_key, "error.context_not_found");
}

fn registry_tracks_app_dialogs_and_export_owner() {
    let mut shell = dtool_ui::shell::AppShell::new();
    let a = shell.workspace.new_document("a");
    shell.workspace.activate(a).unwrap();
    shell.about.open = true;
    shell.open_export_dialog();
    shell.sync_dialog_registry();
    assert_eq!(shell.dialogs.owner_for(DialogKind::About), Some(&DialogOwner::App));
    assert_eq!(shell.dialogs.owner_for(DialogKind::Export), Some(&DialogOwner::Document(a)));
    shell.workspace.close_with(a, dtool_core::workspace::CloseAnswer::No).unwrap();
    shell.drop_orphan_dialogs();
    assert!(!dtool_ui::shell::export_dialog().lock().unwrap().is_open());
    assert!(shell.dialogs.is_open(DialogKind::About));
}

#[test]
fn shell_registry_mirrors_dialogs_serial() {
    let _home = test_home::enter("shell_registry_mirrors_dialogs_serial");
    registry_mirrors_document_dialogs();
    registry_tracks_app_dialogs_and_export_owner();
}