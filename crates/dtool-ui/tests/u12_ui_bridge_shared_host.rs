#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::ObjectId;
use dtool_ui::shell::AppShell;
use dtool_ui::ui_bridge::{UiBridge, CONTEXT_NOT_ACTIVE};
use serde_json::json;

fn shell_with_two_selected() -> (AppShell, ObjectId, ObjectId) {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("bridge");
    shell.workspace.activate(id).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    session.has_document = true;
    let a = ObjectId(session.document.next_object_id);
    session.run_named("object.create", json!({"name": "a", "pos": [10.0, 10.0], "size": [10.0, 10.0]})).unwrap();
    let b = ObjectId(session.document.next_object_id);
    session.run_named("object.create", json!({"name": "b", "pos": [40.0, 10.0], "size": [10.0, 10.0]})).unwrap();
    session.run_named("selection.set", json!({"ids": [a.0, b.0]})).unwrap();
    (shell, a, b)
}

#[test]
fn ui_bridge_has_no_session_copy() {
    let _home = test_home::enter("ui_bridge_has_no_session_copy");
    assert_eq!(std::mem::size_of::<UiBridge>(), 0);
}

#[test]
fn invoke_action_without_active_doc_is_context_not_active() {
    let _home = test_home::enter("invoke_action_without_active_doc_is_context_not_active");
    let mut shell = AppShell::new();
    while let Some(id) = shell.workspace.active() {
        shell.workspace.close_with(id, dtool_core::workspace::CloseAnswer::No).unwrap();
    }
    let count = shell.workspace.contexts().len();
    let error = UiBridge::invoke_action(&mut shell.workspace, &mut shell.locked_delete, "group").unwrap_err();
    assert_eq!(error.code, CONTEXT_NOT_ACTIVE);
    assert_eq!(UiBridge::reset_warp_mesh(&mut shell.workspace).unwrap_err().code, CONTEXT_NOT_ACTIVE);
    assert_eq!(shell.workspace.contexts().len(), count);
}

#[test]
fn invoke_action_changes_the_shell_workspace() {
    let _home = test_home::enter("invoke_action_changes_the_shell_workspace");
    let (mut shell, _, _) = shell_with_two_selected();
    let before = shell.workspace.session_mut().unwrap().generation;
    let result = UiBridge::invoke_action(&mut shell.workspace, &mut shell.locked_delete, "group");
    assert!(result.is_ok(), "{result:?}");
    let session = shell.workspace.session_mut().unwrap();
    assert!(session.generation > before);
    assert!(session.can_execute_named("edit.undo"));
}

#[test]
fn unknown_action_is_not_implemented_and_menu_query_is_static() {
    let _home = test_home::enter("unknown_action_is_not_implemented_and_menu_query_is_static");
    let (mut shell, _, _) = shell_with_two_selected();
    assert!(UiBridge::invoke_action(&mut shell.workspace, &mut shell.locked_delete, "nope").is_err());
    assert!(UiBridge::query_menu("layer").contains(&"menu.layer.group".to_string()));
    assert!(UiBridge::query_menu("unknown").is_empty());
}
