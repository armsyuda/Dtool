#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::ContextId;
use dtool_ui::shell::AppShell;
use serde_json::json;

fn dirty(shell: &mut AppShell, title: &str) -> ContextId {
    let id = shell.workspace.new_document(title);
    shell.workspace.session_mut().unwrap().run_named("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    id
}

#[test]
fn close_all_prompts_for_each_modified_tab_in_order() {
    let _home = test_home::enter("close_all_prompts_for_each_modified_tab_in_order");
    let mut shell = AppShell::new();
    let a = dirty(&mut shell, "a");
    let clean = shell.workspace.new_document("clean");
    let b = dirty(&mut shell, "b");
    shell.close_all_documents();
    assert!(shell.workspace.get(clean).is_none());
    assert_eq!(shell.pending_close_id, Some(a));
    assert_eq!(shell.close_queue.iter().copied().collect::<Vec<_>>(), vec![b]);
    shell.workspace.close_with(a, dtool_core::workspace::CloseAnswer::No).unwrap();
    shell.pending_close_id = None;
    shell.advance_close_queue();
    assert_eq!(shell.pending_close_id, Some(b));
    assert!(shell.close_queue.is_empty());
}

#[test]
fn file_close_menu_closes_active_tab() {
    let _home = test_home::enter("file_close_menu_closes_active_tab");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("clean");
    shell.close_active();
    assert!(shell.workspace.get(id).is_none());
    assert_eq!(shell.pending_close_id, None);
}

#[test]
fn save_on_untitled_is_detected() {
    let _home = test_home::enter("save_on_untitled_is_detected");
    let mut shell = AppShell::new();
    shell.workspace.new_document("untitled");
    assert!(shell.active_untitled());
}

#[test]
fn document_setup_menu_opens_dialog() {
    let _home = test_home::enter("document_setup_menu_opens_dialog");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("setup");
    shell.open_document_setup();
    assert!(shell.document_setup.open);
    assert_eq!(shell.document_setup_owner, Some(id));
}
