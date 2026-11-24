#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::object::ObjectKind;
use dtool_core::workspace::Workspace;
use dtool_core::ObjectId;
use dtool_protocol::{Command, CommandError, CommandResult};
use dtool_ui::headless::{create_headless_ui_harness, run_on_workspace, UI_HOST_COMMANDS};
use dtool_ui::menu_tree::{install, missing_b11_keys};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn ready() {
    assert!(install(), "menu provider not installed, missing keys {:?}", missing_b11_keys());
}

fn shell_with(names: &[&str]) -> (AppShell, Vec<u64>) {
    ready();
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("u12 acceptance");
    shell.workspace.activate(id).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    let mut ids = Vec::new();
    for (index, name) in names.iter().enumerate() {
        ids.push(session.document.next_object_id);
        session.run_named("object.create", json!({ "name": name, "pos": [index as f64 * 20.0, 0.0], "size": [10.0, 10.0] })).unwrap();
    }
    session.run_named("selection.set", json!({ "ids": ids })).unwrap();
    (shell, ids)
}

fn invoke(workspace: &mut Workspace, id: &str) -> CommandResult {
    run_on_workspace(workspace, Command::with_params("ui.menu.invoke", json!({ "id": id, "persona": "vector" })))
}

fn generation(shell: &AppShell) -> u64 {
    shell.workspace.session().unwrap().generation
}

fn object<'a>(shell: &'a AppShell, id: u64) -> &'a dtool_core::object::Object {
    shell.workspace.session().unwrap().document.get_object(ObjectId(id)).expect("object")
}

fn find<'a>(items: &'a Value, id: &str) -> Option<&'a Value> {
    for item in items.as_array()? {
        if item.get("id").and_then(Value::as_str) == Some(id) { return Some(item); }
        if let Some(found) = item.get("children").and_then(|children| find(children, id)) { return Some(found); }
    }
    None
}

fn planned_leaf(items: &Value) -> Option<String> {
    for item in items.as_array()? {
        let children = item.get("children").and_then(Value::as_array).filter(|children| !children.is_empty());
        match children {
            Some(_) => { if let Some(found) = planned_leaf(&item["children"]) { return Some(found); } }
            None => {
                if item.get("planned") == Some(&Value::Bool(true)) {
                    if let Some(id) = item.get("id").and_then(Value::as_str) { return Some(id.to_string()); }
                }
            }
        }
    }
    None
}

fn menu(workspace: &mut Workspace) -> Value {
    run_on_workspace(workspace, Command::with_params("ui.menu.query", json!({ "persona": "vector" }))).expect("ui.menu.query")
}

fn assert_error(result: CommandResult, code: &str) -> CommandError {
    let error = result.expect_err(code);
    assert_eq!(error.code, code);
    assert_eq!(error.message_key, format!("error.{}", code.to_lowercase()));
    error
}

#[test]
fn menu_lock_unlock_acts_on_the_live_shell_document_with_one_undo_each() {
    let _home = test_home::enter("menu_lock_unlock_acts_on_the_live_shell_document_with_one_undo_each");
    let (mut shell, ids) = shell_with(&["a"]);
    let node = ids[0];
    let tree = menu(&mut shell.workspace);
    assert_eq!(find(&tree, "layer.lock").unwrap()["planned"], json!(false));
    assert_eq!(find(&tree, "layer.lock").unwrap()["enabled"], json!(true));
    let before = generation(&shell);
    invoke(&mut shell.workspace, "layer.lock").expect("layer.lock");
    assert!(object(&shell, node).locked);
    assert!(generation(&shell) > before);
    assert_eq!(shell.workspace.session().unwrap().selection.ids, vec![ObjectId(node)]);
    let tree = menu(&mut shell.workspace);
    assert_eq!(find(&tree, "layer.unlock").unwrap()["enabled"], json!(true));
    assert_eq!(find(&tree, "layer.unlock_all").unwrap()["enabled"], json!(true));
    run_on_workspace(&mut shell.workspace, Command::new("edit.undo")).expect("edit.undo");
    assert!(!object(&shell, node).locked);
    run_on_workspace(&mut shell.workspace, Command::new("edit.redo")).expect("edit.redo");
    assert!(object(&shell, node).locked);
    invoke(&mut shell.workspace, "layer.unlock").expect("layer.unlock");
    assert!(!object(&shell, node).locked);
}

#[test]
fn menu_group_through_the_headless_shared_transport_lands_in_the_shell_document() {
    let _home = test_home::enter("menu_group_through_the_headless_shared_transport_lands_in_the_shell_document");
    let (mut shell, ids) = shell_with(&["a", "b"]);
    let context = shell.workspace.active();
    let before = generation(&shell);
    let live = std::mem::take(&mut shell.workspace).into_shared();
    {
        let mut harness = create_headless_ui_harness(live.clone());
        assert_eq!(harness.kind(), "headless");
        harness.run_command(Command::with_params("ui.menu.invoke", json!({ "id": "layer.group", "persona": "vector" }))).expect("layer.group");
    }
    shell.workspace = std::sync::Arc::try_unwrap(live).ok().expect("single owner").into_inner().expect("workspace");
    assert_eq!(shell.workspace.active(), context);
    assert!(generation(&shell) > before);
    let session = shell.workspace.session().unwrap();
    let group = session.document.objects().iter().find(|object| matches!(object.kind, ObjectKind::Group)).expect("group object");
    for id in &ids {
        assert_eq!(object(&shell, *id).parent_id, Some(group.id));
    }
}

#[test]
fn menu_hide_show_all_and_arrange_change_the_live_document() {
    let _home = test_home::enter("menu_hide_show_all_and_arrange_change_the_live_document");
    let (mut shell, ids) = shell_with(&["a", "b"]);
    let (a, b) = (ids[0], ids[1]);
    let order = |shell: &AppShell, id: u64| shell.workspace.session().unwrap().document.objects().iter().position(|object| object.id.0 == id).unwrap();
    assert!(order(&shell, a) < order(&shell, b));
    run_on_workspace(&mut shell.workspace, Command::with_params("selection.set", json!({ "ids": [b] }))).unwrap();
    invoke(&mut shell.workspace, "layer.arrange.move.to_back").expect("to_back");
    assert!(order(&shell, b) < order(&shell, a));
    run_on_workspace(&mut shell.workspace, Command::with_params("node.update", json!({ "nodes": [b], "set": { "visible": false } }))).expect("node.update");
    assert!(!object(&shell, b).pickable);
    assert!(object(&shell, a).pickable);
    invoke(&mut shell.workspace, "layer.show.all").expect("show all");
    assert!(object(&shell, b).pickable);
}

#[test]
fn menu_invoke_error_shapes_leave_the_live_document_unchanged() {
    let _home = test_home::enter("menu_invoke_error_shapes_leave_the_live_document_unchanged");
    let (mut shell, _) = shell_with(&["a"]);
    let before = generation(&shell);
    assert_error(invoke(&mut shell.workspace, "menu.not_an_item"), "NOT_FOUND");
    assert_error(run_on_workspace(&mut shell.workspace, Command::with_params("ui.menu.invoke", json!({ "persona": "vector" }))), "INVALID_PARAMS");
    let tree = menu(&mut shell.workspace);
    let planned = planned_leaf(&tree).expect("a planned leaf in the vector menu");
    assert_error(invoke(&mut shell.workspace, &planned), "NOT_IMPLEMENTED");
    assert_eq!(generation(&shell), before);
    run_on_workspace(&mut shell.workspace, Command::with_params("selection.set", json!({ "ids": [] }))).unwrap();
    let cleared = generation(&shell);
    assert_error(invoke(&mut shell.workspace, "layer.lock"), "CANNOT_EXECUTE");
    assert_eq!(generation(&shell), cleared);
}

#[test]
fn menu_invoke_without_an_open_document_is_no_document() {
    let _home = test_home::enter("menu_invoke_without_an_open_document_is_no_document");
    ready();
    let mut shell = AppShell::new();
    while let Some(id) = shell.workspace.active() {
        shell.workspace.close_with(id, dtool_core::workspace::CloseAnswer::No).unwrap();
    }
    assert_error(invoke(&mut shell.workspace, "layer.lock"), "NO_DOCUMENT");
}

#[test]
fn ui_host_commands_on_the_live_workspace_are_no_window_and_do_not_touch_it() {
    let _home = test_home::enter("ui_host_commands_on_the_live_workspace_are_no_window_and_do_not_touch_it");
    let (mut shell, ids) = shell_with(&["a"]);
    let before = generation(&shell);
    for name in UI_HOST_COMMANDS {
        assert_error(run_on_workspace(&mut shell.workspace, Command::with_params(name, json!({ "viewport": "root" }))), "NO_WINDOW");
    }
    assert_eq!(generation(&shell), before);
    assert_eq!(shell.workspace.session().unwrap().selection.ids, vec![ObjectId(ids[0])]);
    assert!(menu(&mut shell.workspace).is_array());
}
#[test]
fn mcp_menu_rows_match_gui_row_state() {
    let _home = test_home::enter("mcp_menu_rows_match_gui_row_state");
    let (mut shell, ids) = shell_with(&["a"]);
    let node = ids[0];
    let before = generation(&shell);
    assert_error(invoke(&mut shell.workspace, "layer.unlock"), "CANNOT_EXECUTE");
    assert_eq!(generation(&shell), before);
    let tree = menu(&mut shell.workspace);
    assert_eq!(find(&tree, "layer.unlock").unwrap()["enabled"], json!(false));
    assert_eq!(find(&tree, "layer.hide.hide").unwrap()["enabled"], json!(true));
    invoke(&mut shell.workspace, "layer.lock").expect("layer.lock");
    let tree = menu(&mut shell.workspace);
    assert_eq!(find(&tree, "layer.lock").unwrap()["enabled"], json!(false));
    assert_error(invoke(&mut shell.workspace, "layer.lock"), "CANNOT_EXECUTE");
    invoke(&mut shell.workspace, "layer.unlock").expect("layer.unlock");
    invoke(&mut shell.workspace, "layer.hide.hide").expect("layer.hide.hide");
    assert!(!object(&shell, node).pickable);
}
