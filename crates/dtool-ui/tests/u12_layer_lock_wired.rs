#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::menu_tree::bridge_action_enabled;
use dtool_ui::menus::layer_lock::{for_bridge, is_enabled, LAYER_LOCK, LAYER_SHOW_ALL, LAYER_UNLOCK, LAYER_UNLOCK_ALL};
use dtool_ui::shell::AppShell;
use serde_json::json;

#[test]
fn bridge_names_map_to_layer_lock_rows() {
    let _home = test_home::enter("bridge_names_map_to_layer_lock_rows");
    assert_eq!(for_bridge("lock"), Some(LAYER_LOCK));
    assert_eq!(for_bridge("unlock"), Some(LAYER_UNLOCK));
    assert_eq!(for_bridge("unlock_all"), Some(LAYER_UNLOCK_ALL));
    assert_eq!(for_bridge("show_all"), Some(LAYER_SHOW_ALL));
    assert_eq!(for_bridge("group"), None);
}

#[test]
fn lock_rows_follow_object_lock_state() {
    let _home = test_home::enter("lock_rows_follow_object_lock_state");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("lock");
    shell.workspace.activate(id).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    let node = session.document.next_object_id;
    session.run_named("object.create", json!({ "name": "lock", "pos": [0.0, 0.0], "size": [10.0, 10.0] })).expect("object.create");
    assert!(session.document.objects().iter().any(|object| object.id.0 == node));
    session.run_named("selection.set", json!({ "ids": [node] })).unwrap();
    assert_eq!(bridge_action_enabled("lock", session), is_enabled(session, LAYER_LOCK));
    assert!(bridge_action_enabled("lock", session));
    assert!(!bridge_action_enabled("unlock", session));
    session.run_named("node.lock_state", json!({ "nodes": [node], "state": "locked" })).unwrap();
    assert!(!bridge_action_enabled("lock", session));
    assert!(bridge_action_enabled("unlock", session));
    assert!(bridge_action_enabled("unlock_all", session));
}
