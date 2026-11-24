#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::menu_tree::{bridge_command, install, menu_tree_json, BRIDGE_MENU_ACTIONS};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn find<'a>(items: &'a Value, id: &str) -> Option<&'a Value> {
    for item in items.as_array()? {
        if item.get("id").and_then(Value::as_str) == Some(id) { return Some(item); }
        if let Some(found) = item.get("children").and_then(|children| find(children, id)) { return Some(found); }
    }
    None
}

#[test]
fn provider_has_real_commands_and_default_params_for_every_bridge_row() {
    let _home = test_home::enter("provider_has_real_commands_and_default_params_for_every_bridge_row");
    let tree = menu_tree_json("vector");
    for (id, action) in BRIDGE_MENU_ACTIONS {
        let row = find(&tree, id).unwrap();
        let (name, params) = bridge_command(action).unwrap();
        assert_eq!(row["command"], json!(name), "{id}");
        assert_eq!(row["params"], params, "{id}");
    }
}

#[test]
fn mcp_menu_invoke_uses_bridge_params_on_same_session() {
    let _home = test_home::enter("mcp_menu_invoke_uses_bridge_params_on_same_session");
    assert!(install());
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("menus");
    shell.workspace.activate(id).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    let node = session.document.next_object_id;
    session.run_named("object.create", json!({ "name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0] })).unwrap();
    session.run_named("selection.set", json!({ "ids": [node] })).unwrap();
    let tree = session.run_named("ui.menu.query", json!({ "persona": "vector" })).unwrap();
    assert_eq!(find(&tree, "layer.lock").unwrap()["planned"], json!(false));
    session.run_named("ui.menu.invoke", json!({ "id": "layer.lock", "persona": "vector" })).unwrap();
    assert!(session.document.objects().iter().find(|object| object.id.0 == node).unwrap().locked);
    session.run_named("ui.menu.invoke", json!({ "id": "layer.unlock", "persona": "vector" })).unwrap();
    assert!(!session.document.objects().iter().find(|object| object.id.0 == node).unwrap().locked);
}
