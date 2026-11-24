use dtool_core::workspace::{CloseAnswer, Workspace};
use dtool_ui::dialogs::{owner_alive, resolve_close_answer, CloseResolution, DialogOwner, FileDialogMode, FileDialogState};

fn temp_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_k02_dialog_owner_{}_{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}.dtool"))
}

#[test]
fn save_as_applies_to_owner_after_tab_switch() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    let mut state = FileDialogState::for_document(FileDialogMode::SaveAs, a);
    workspace.activate(b).unwrap();
    let path = temp_path("switch");
    state.path = path.display().to_string();
    assert!(state.apply(&mut workspace));
    assert_eq!(workspace.get(a).unwrap().path.as_deref(), Some(path.as_path()));
    assert!(workspace.get(b).unwrap().path.is_none());
}

#[test]
fn save_as_errors_when_owner_closed() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let _b = workspace.new_document("b");
    let mut state = FileDialogState::for_document(FileDialogMode::SaveAs, a);
    workspace.close_with(a, CloseAnswer::No).unwrap();
    let path = temp_path("closed");
    let _ = std::fs::remove_file(&path);
    state.path = path.display().to_string();
    assert!(!state.apply(&mut workspace));
    assert_eq!(state.error_message.as_deref(), Some(dtool_strings::text("error.context_not_found")));
    assert!(!path.exists());
}

#[test]
fn close_yes_untitled_then_save_as_closes_owner() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    workspace.activate(a).unwrap();
    workspace.session_mut().unwrap().run_named("object.create", serde_json::json!({"name": "x", "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    assert_eq!(resolve_close_answer(&mut workspace, a, CloseAnswer::Yes).unwrap(), CloseResolution::NeedsSaveAs);
    workspace.activate(b).unwrap();
    let mut state = FileDialogState::for_document(FileDialogMode::SaveAs, a).closing();
    let path = temp_path("closing");
    state.path = path.display().to_string();
    assert!(state.apply(&mut workspace));
    assert!(workspace.get(a).is_none());
    assert!(workspace.get(b).is_some());
    assert!(path.exists());
}

#[test]
fn owner_alive_false_after_close() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let owner = DialogOwner::Document(a);
    assert!(owner_alive(&workspace, &owner));
    workspace.close_with(a, CloseAnswer::No).unwrap();
    assert!(!owner_alive(&workspace, &owner));
    assert!(owner_alive(&workspace, &DialogOwner::App));
}
