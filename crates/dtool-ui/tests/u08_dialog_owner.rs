#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::{CloseAnswer, Workspace};
use dtool_ui::dialogs::{
    dialog_session, resolve_close_answer, CloseResolution, DialogKind, DialogOwner, DialogRegistry, FileDialogMode,
    FileDialogState,
};
use dtool_ui::shell::AppShell;

#[test]
fn dialog_session_resolves_owner_only() {
    let _home = test_home::enter("dialog_session_resolves_owner_only");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("a");
    let b = shell.workspace.new_document("b");
    shell.workspace.activate(b).unwrap();
    dialog_session(&mut shell.workspace, &DialogOwner::Document(a)).unwrap().view.zoom = 3.0;
    assert_eq!(shell.workspace.get(a).unwrap().session.view.zoom, 3.0);
    assert_ne!(shell.workspace.get(b).unwrap().session.view.zoom, 3.0);
    assert_eq!(dialog_session(&mut shell.workspace, &DialogOwner::App).err().unwrap().code, "NO_DOCUMENT");
    shell.workspace.close_with(a, CloseAnswer::No).unwrap();
    assert_eq!(dialog_session(&mut shell.workspace, &DialogOwner::Document(a)).err().unwrap().code, "CONTEXT_NOT_FOUND");
}

#[test]
fn guides_manager_stays_on_opening_document() {
    let _home = test_home::enter("guides_manager_stays_on_opening_document");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("a");
    let b = shell.workspace.new_document("b");
    shell.workspace.activate(a).unwrap();
    assert!(shell.open_guides_manager());
    shell.workspace.activate(b).unwrap();
    assert_eq!(shell.guides_owner, Some(a));
    shell.workspace.close_with(a, CloseAnswer::No).unwrap();
    shell.drop_orphan_dialogs();
    assert!(!shell.guides_dialog.open);
    assert_eq!(shell.guides_owner, None);
    assert_eq!(shell.status_bar.hint_key, "error.context_not_found");
}

#[test]
fn guides_manager_needs_a_document() {
    let _home = test_home::enter("guides_manager_needs_a_document");
    let mut shell = AppShell::new();
    assert!(!shell.open_guides_manager());
    assert!(!shell.guides_dialog.open);
}

#[test]
fn registry_drops_document_entries_when_context_gone() {
    let _home = test_home::enter("registry_drops_document_entries_when_context_gone");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("a");
    let mut registry = DialogRegistry::default();
    registry.insert(DialogOwner::Document(a), DialogKind::File);
    registry.insert(DialogOwner::App, DialogKind::About);
    registry.confirm_session(&mut shell.workspace, DialogKind::File).unwrap().view.zoom = 2.5;
    assert_eq!(shell.workspace.get(a).unwrap().session.view.zoom, 2.5);
    shell.workspace.close_with(a, CloseAnswer::No).unwrap();
    assert!(registry.drop_gone(&shell.workspace));
    assert!(registry.owner_for(DialogKind::File).is_none());
    assert_eq!(registry.owner_for(DialogKind::About), Some(&DialogOwner::App));
    assert_eq!(
        registry.confirm_session(&mut shell.workspace, DialogKind::File).err().unwrap().code,
        "CONTEXT_NOT_FOUND"
    );
}

#[test]
fn save_as_opened_on_a_does_not_write_b_after_switch() {
    let _home = test_home::enter("save_as_opened_on_a_does_not_write_b_after_switch");
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    let path = std::env::temp_dir().join(format!("dtool_u08_fix_a_{}.dtool", a.0));
    let _ = std::fs::remove_file(&path);
    let mut state = FileDialogState::for_document(FileDialogMode::SaveAs, a);
    state.path = path.to_string_lossy().into_owned();
    workspace.activate(b).unwrap();
    assert!(state.apply(&mut workspace));
    assert_eq!(workspace.get(a).and_then(|context| context.path.clone()), Some(path.clone()));
    assert!(workspace.get(b).unwrap().path.is_none());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_as_on_closed_a_does_not_write_b() {
    let _home = test_home::enter("save_as_on_closed_a_does_not_write_b");
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    let path = std::env::temp_dir().join(format!("dtool_u08_fix_closed_{}.dtool", a.0));
    let mut state = FileDialogState::for_document(FileDialogMode::SaveAs, a);
    state.path = path.to_string_lossy().into_owned();
    workspace.activate(b).unwrap();
    workspace.close_with(a, CloseAnswer::No).unwrap();
    assert!(!state.apply(&mut workspace));
    assert_eq!(state.error_message.as_deref(), Some(dtool_strings::text("error.context_not_found")));
    assert!(workspace.get(b).is_some());
    assert!(workspace.get(b).unwrap().path.is_none());
    assert!(!path.exists());
}

#[test]
fn close_untitled_save_as_stays_on_same_context() {
    let _home = test_home::enter("close_untitled_save_as_stays_on_same_context");
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    assert_eq!(resolve_close_answer(&mut workspace, a, CloseAnswer::Yes).unwrap(), CloseResolution::NeedsSaveAs);
    let path = std::env::temp_dir().join(format!("dtool_u08_fix_close_{}.dtool", a.0));
    let _ = std::fs::remove_file(&path);
    let mut state = FileDialogState::for_document(FileDialogMode::SaveAs, a).closing();
    state.path = path.to_string_lossy().into_owned();
    workspace.activate(b).unwrap();
    assert!(state.apply(&mut workspace));
    assert!(workspace.get(a).is_none());
    assert!(workspace.get(b).is_some());
    assert!(workspace.get(b).unwrap().path.is_none());
    let _ = std::fs::remove_file(&path);
}
