use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_protocol::Command;
use serde_json::json;

fn session_with_shapes() -> (Session, Vec<ObjectId>) {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let mut ids = Vec::new();
    for (name, pos) in [("A", [10.0, 10.0]), ("B", [20.0, 20.0]), ("C", [60.0, 60.0])] {
        ids.push(session.document.create_object(Object {
            name: name.into(),
            pos,
            size: [30.0, 30.0],
            ..Object::default()
        }));
    }
    (session, ids)
}

#[test]
fn display_list_omits_hidden_and_uses_pos_size_bounds() {
    let (mut session, ids) = session_with_shapes();
    session.run(&Command::with_params("node.update", json!({ "nodes": [ids[1].0], "set": { "visible": false } }))).unwrap();
    let reply = session.run(&Command::new("canvas.display_list")).unwrap();
    let items = reply["items"].as_array().unwrap();
    let listed: Vec<u64> = items.iter().map(|item| item["object_id"].as_u64().unwrap()).collect();
    assert!(listed.contains(&ids[0].0));
    assert!(!listed.contains(&ids[1].0));
    assert!(listed.contains(&ids[2].0));
    let bounds = items.iter().find(|item| item["object_id"] == ids[0].0).unwrap()["bounds"].as_array().unwrap();
    assert_eq!(bounds.len(), 4);
    assert_eq!(bounds[0], 10.0);
    assert_eq!(bounds[2], 30.0);
}

#[test]
fn ui_action_group_and_locked_delete_confirm() {
    let (mut session, ids) = session_with_shapes();
    let generation = session.generation;
    let reply = session.run(&Command::with_params("ui.action.invoke", json!({ "action": "group", "nodes": [ids[0].0, ids[1].0] }))).unwrap();
    let group = ObjectId(reply["group"].as_u64().unwrap());
    assert_eq!(session.generation, generation + 1);
    assert_eq!(session.document.get_object(ids[0]).unwrap().parent_id, Some(group));
    assert!(matches!(session.document.get_object(group).unwrap().kind, ObjectKind::Group));

    session.run(&Command::with_params("node.lock_state", json!({ "nodes": [ids[2].0], "state": "locked" }))).unwrap();
    let locked_gen = session.generation;
    let err = session.run(&Command::with_params("ui.action.invoke", json!({ "action": "delete", "nodes": [ids[2].0] }))).unwrap_err();
    assert_eq!(err.code, "LOCKED_NEEDS_CONFIRM");
    assert_eq!(session.generation, locked_gen);
    assert!(session.document.get_object(ids[2]).is_some());
    session.run(&Command::with_params("ui.action.invoke", json!({ "action": "delete_confirm_no" }))).unwrap();
    assert_eq!(session.generation, locked_gen);
    assert!(session.document.get_object(ids[2]).is_some());
    session.run(&Command::with_params("ui.action.invoke", json!({ "action": "delete", "nodes": [ids[2].0] }))).unwrap_err();
    session.run(&Command::with_params("ui.action.invoke", json!({ "action": "delete_confirm_yes" }))).unwrap();
    assert_eq!(session.generation, locked_gen + 1);
    assert!(session.document.get_object(ids[2]).is_none());
}

#[test]
fn layer_menu_query_fallback_is_read_only_and_ordered() {
    let (mut session, _) = session_with_shapes();
    let generation = session.generation;
    let before = session.document.version;
    let reply = session.run(&Command::with_params("ui.menu.query", json!({ "menu": "layer" }))).unwrap();
    let mut keys = Vec::new();
    fn collect(value: &serde_json::Value, keys: &mut Vec<String>) {
        match value {
            serde_json::Value::String(text) => keys.push(text.clone()),
            serde_json::Value::Array(items) => { for item in items { collect(item, keys); } }
            serde_json::Value::Object(fields) => { for item in fields.values() { collect(item, keys); } }
            _ => {}
        }
    }
    collect(&reply, &mut keys);
    let ordered = ["menu.layer.group", "menu.layer.ungroup", "menu.layer.ungroup_all", "menu.layer.new_group", "menu.layer.lock"];
    let positions: Vec<_> = ordered.iter().map(|key| keys.iter().position(|value| value == key).expect("missing layer key")).collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    for key in ["menu.layer.unlock", "menu.layer.unlock_all", "menu.layer.hide", "menu.layer.show", "menu.layer.show_all"] {
        assert!(keys.iter().any(|value| value == key));
    }
    assert_eq!(session.generation, generation);
    assert_eq!(session.document.version, before);
}
