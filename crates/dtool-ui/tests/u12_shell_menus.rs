#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::menu_tree::{apply_bindings, bridge_action, bridge_action_enabled, MenuTree, BRIDGE_MENU_ACTIONS};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn find<'a>(items: &'a Value, id: &str) -> Option<&'a Value> {
    for item in items.as_array()? {
        if item.get("id").and_then(Value::as_str) == Some(id) { return Some(item); }
        if let Some(found) = item.get("children").and_then(|c| find(c, id)) { return Some(found); }
    }
    None
}

fn shell_with_selection() -> AppShell {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("menus");
    shell.workspace.activate(id).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    session.has_document = true;
    let a = session.document.next_object_id;
    session.run_named("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    let b = session.document.next_object_id;
    session.run_named("object.create", json!({"name": "b", "pos": [20.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    session.run_named("selection.set", json!({"ids": [a, b]})).unwrap();
    shell
}

#[test]
fn bridge_ids_exist_in_menu_tree() {
    let _home = test_home::enter("bridge_ids_exist_in_menu_tree");
    let tree = MenuTree::for_persona("vector");
    for (id, _) in BRIDGE_MENU_ACTIONS {
        assert!(tree.find(id).is_some(), "{id}");
    }
    assert_eq!(bridge_action("layer.group"), Some("group"));
    assert_eq!(bridge_action("layer.rename_layer"), None);
}

#[test]
fn bridge_rows_enabled_by_selection() {
    let _home = test_home::enter("bridge_rows_enabled_by_selection");
    let mut shell = shell_with_selection();
    let session = shell.workspace.session_mut().unwrap();
    assert!(bridge_action_enabled("group", session));
    let mut tree = MenuTree::for_persona("vector").resolved_json(session);
    apply_bindings(&mut tree, session);
    assert_eq!(find(&tree, "layer.group").unwrap()["planned"], json!(false));
    assert_eq!(find(&tree, "layer.group").unwrap()["enabled"], json!(true));
    assert_eq!(find(&tree, "layer.rename_layer").unwrap()["enabled"], json!(false));
    session.run_named("selection.set", json!({"ids": []})).unwrap();
    assert!(!bridge_action_enabled("group", session));
    assert_eq!(bridge_action_enabled("show_all", session), session.document.objects().iter().any(|object| !object.pickable));
}

#[test]
fn shell_runs_bridge_on_live_workspace() {
    let _home = test_home::enter("shell_runs_bridge_on_live_workspace");
    let mut shell = shell_with_selection();
    let before = shell.workspace.session().unwrap().generation;
    shell.run_bridge_action("group").unwrap();
    assert!(shell.workspace.session().unwrap().generation > before);
    let mut empty = AppShell::new();
    while let Some(id) = empty.workspace.active() {
        empty.workspace.close_with(id, dtool_core::workspace::CloseAnswer::No).unwrap();
    }
    assert_eq!(empty.run_bridge_action("group").unwrap_err().code, "CONTEXT_NOT_ACTIVE");
}
