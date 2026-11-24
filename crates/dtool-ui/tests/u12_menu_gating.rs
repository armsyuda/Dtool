#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::menus::{register_menu_row_gate, row_gate_ready};
use dtool_core::{ObjectId, Session};
use dtool_protocol::CommandResult;
use dtool_ui::menu_tree::{bridge_action, bridge_action_enabled, install, missing_b11_keys, BRIDGE_MENU_ACTIONS};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn gui_row(id: &str, session: &Session) -> Option<bool> {
    bridge_action(id).map(|action| bridge_action_enabled(action, session))
}

fn shell_with(names: &[&str]) -> (AppShell, Vec<u64>) {
    assert!(install(), "menu provider not installed, missing keys {:?}", missing_b11_keys());
    assert!(register_menu_row_gate(gui_row));
    assert!(row_gate_ready());
    let mut shell = AppShell::new();
    let doc = shell.workspace.new_document("u12 gating");
    shell.workspace.activate(doc).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    let mut ids = Vec::new();
    for (index, name) in names.iter().enumerate() {
        ids.push(session.document.next_object_id);
        session.run_named("object.create", json!({ "name": name, "pos": [index as f64 * 20.0, 0.0], "size": [10.0, 10.0] })).expect("object.create");
    }
    session.run_named("selection.set", json!({ "ids": ids })).expect("selection.set");
    (shell, ids)
}

fn session(shell: &mut AppShell) -> &mut Session {
    shell.workspace.session_mut().unwrap()
}

fn invoke(shell: &mut AppShell, id: &str) -> CommandResult {
    session(shell).run_named("ui.menu.invoke", json!({ "id": id, "persona": "vector" }))
}

fn tree(shell: &mut AppShell) -> Value {
    session(shell).run_named("ui.menu.query", json!({ "persona": "vector" })).expect("ui.menu.query")
}

fn find<'a>(items: &'a Value, id: &str) -> Option<&'a Value> {
    for item in items.as_array()? {
        if item.get("id").and_then(Value::as_str) == Some(id) { return Some(item); }
        if let Some(found) = item.get("children").and_then(|children| find(children, id)) { return Some(found); }
    }
    None
}

fn row_enabled(shell: &mut AppShell, id: &str) -> bool {
    let tree = tree(shell);
    let row = find(&tree, id).unwrap_or_else(|| panic!("row {id}"));
    assert_eq!(row["planned"], json!(false), "{id}");
    row["enabled"].as_bool().unwrap()
}

fn stamp(shell: &mut AppShell) -> (u64, u64) {
    let session = session(shell);
    (session.generation, session.document.version)
}

fn assert_refused(shell: &mut AppShell, id: &str) {
    let before = stamp(shell);
    let error = invoke(shell, id).expect_err(id);
    assert_eq!(error.code, "CANNOT_EXECUTE", "{id}");
    assert_eq!(stamp(shell), before, "{id}");
}

fn locked(shell: &mut AppShell, id: u64) -> bool {
    session(shell).document.get_object(ObjectId(id)).expect("object").locked
}

fn assert_parity(shell: &mut AppShell, label: &str) {
    let tree = tree(shell);
    let session = session(shell);
    for (menu, action) in BRIDGE_MENU_ACTIONS {
        let row = find(&tree, menu).unwrap_or_else(|| panic!("row {menu}"));
        assert_eq!(row["enabled"].as_bool(), Some(bridge_action_enabled(action, session)), "{label}: {menu}");
    }
}

#[test]
fn unlock_with_nothing_locked_is_refused_without_version_bump_or_undo_step() {
    let _home = test_home::enter("unlock_with_nothing_locked_is_refused_without_version_bump_or_undo_step");
    let (mut shell, ids) = shell_with(&["a"]);
    assert!(!row_enabled(&mut shell, "layer.unlock"));
    let history = format!("{:?}", session(&mut shell).undo);
    assert_refused(&mut shell, "layer.unlock");
    assert!(!locked(&mut shell, ids[0]));
    assert_eq!(format!("{:?}", session(&mut shell).undo), history, "refused unlock must not push an undo step");
}

#[test]
fn lock_row_is_disabled_on_an_already_locked_selection() {
    let _home = test_home::enter("lock_row_is_disabled_on_an_already_locked_selection");
    let (mut shell, ids) = shell_with(&["a"]);
    assert!(row_enabled(&mut shell, "layer.lock"));
    invoke(&mut shell, "layer.lock").expect("layer.lock");
    assert!(locked(&mut shell, ids[0]));
    assert!(!row_enabled(&mut shell, "layer.lock"));
    assert!(row_enabled(&mut shell, "layer.unlock"));
    assert_refused(&mut shell, "layer.lock");
    invoke(&mut shell, "layer.unlock").expect("layer.unlock");
    assert!(!locked(&mut shell, ids[0]));
    assert!(!row_enabled(&mut shell, "layer.unlock"));
}

#[test]
fn hide_row_matches_gui_and_hides_the_selection() {
    let _home = test_home::enter("hide_row_matches_gui_and_hides_the_selection");
    let (mut shell, ids) = shell_with(&["a"]);
    assert!(row_enabled(&mut shell, "layer.hide.hide"));
    assert!(bridge_action_enabled("hide", session(&mut shell)));
    let before = stamp(&mut shell);
    invoke(&mut shell, "layer.hide.hide").expect("layer.hide.hide");
    assert!(stamp(&mut shell).1 > before.1);
    assert!(!session(&mut shell).document.get_object(ObjectId(ids[0])).expect("object").pickable);
    session(&mut shell).run_named("selection.set", json!({ "ids": [] })).expect("selection.set");
    assert!(!row_enabled(&mut shell, "layer.hide.hide"));
    assert_refused(&mut shell, "layer.hide.hide");
}

#[test]
fn every_bridge_row_matches_gui_enabled_state_across_selection_states() {
    let _home = test_home::enter("every_bridge_row_matches_gui_enabled_state_across_selection_states");
    let (mut shell, ids) = shell_with(&["a", "b"]);
    assert_parity(&mut shell, "two unlocked selected");
    invoke(&mut shell, "layer.lock").expect("layer.lock");
    assert_parity(&mut shell, "two locked selected");
    invoke(&mut shell, "layer.unlock").expect("layer.unlock");
    session(&mut shell).run_named("selection.set", json!({ "ids": [ids[0]] })).expect("selection.set");
    assert_parity(&mut shell, "one selected");
    invoke(&mut shell, "layer.hide.hide").expect("layer.hide.hide");
    assert_parity(&mut shell, "hidden");
    session(&mut shell).run_named("selection.set", json!({ "ids": [] })).expect("selection.set");
    assert_parity(&mut shell, "empty selection");
}