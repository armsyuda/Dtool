#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::menus::{row_gate, row_gate_ready};
use dtool_ui::menu_tree::{bridge_action_enabled, install, missing_b11_keys, BRIDGE_MENU_ACTIONS};
use dtool_ui::shell::AppShell;
use serde_json::json;

#[test]
fn install_registers_the_gui_row_gate_for_every_bridge_row() {
    let _home = test_home::enter("install_registers_the_gui_row_gate_for_every_bridge_row");
    let installed = install();
    assert_eq!(installed, missing_b11_keys().is_empty());
    assert!(row_gate_ready());
    let mut shell = AppShell::new();
    let doc = shell.workspace.new_document("u12 gate install");
    shell.workspace.activate(doc).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    session.run_named("object.create", json!({ "name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0] })).expect("object.create");
    let id = session.document.next_object_id - 1;
    for ids in [json!([]), json!([id])] {
        session.run_named("selection.set", json!({ "ids": ids })).expect("selection.set");
        for (menu, action) in BRIDGE_MENU_ACTIONS {
            assert_eq!(row_gate(menu, session), Some(bridge_action_enabled(action, session)), "{menu}");
        }
    }
    assert_eq!(row_gate("file.no_such_row", session), None);
}