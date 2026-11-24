use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::commands::arrange::{NODE_GROUP, NODE_REPARENT, NODE_SELECT_INSIDE, NODE_TREE_QUERY, NODE_UPDATE, would_cycle};
use dtool_core::scene::{effective_pickable, is_drawn};
use dtool_protocol::Command;
use serde_json::json;

fn session_with_shapes() -> (Session, Vec<ObjectId>) {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let mut ids = Vec::new();
    for name in ["A", "B", "C"] {
        let id = session.document.create_object(Object {
            name: name.into(),
            pos: [10.0, 10.0],
            size: [20.0, 20.0],
            ..Object::default()
        });
        ids.push(id);
    }
    (session, ids)
}

#[test]
fn tree_query_keeps_hidden_rows_and_maps_visible_to_pickable() {
    let (mut session, ids) = session_with_shapes();
    session.run(&Command::with_params(NODE_UPDATE, json!({ "nodes": [ids[1].0], "set": { "visible": false } }))).unwrap();
    session.run(&Command::with_params(NODE_UPDATE, json!({ "nodes": [ids[0].0], "set": { "locked": true } }))).unwrap();
    let tree = session.run(&Command::new(NODE_TREE_QUERY)).unwrap();
    let nodes = tree["tree"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    let b = nodes.iter().find(|row| row["id"] == ids[1].0).unwrap();
    assert_eq!(b["flags"]["visible"], json!(false));
    assert_eq!(b["flags"]["locked"], json!(false));
    let hidden_only = session.run(&Command::with_params(NODE_TREE_QUERY, json!({ "include_hidden": false }))).unwrap();
    let visible_ids: Vec<u64> = hidden_only["nodes"].as_array().unwrap().iter().map(|row| row["id"].as_u64().unwrap()).collect();
    assert!(!visible_ids.contains(&ids[1].0));
    assert!(visible_ids.contains(&ids[0].0));
}

#[test]
fn hide_does_not_overwrite_descendant_pickable() {
    let (mut session, ids) = session_with_shapes();
    let group = session.run(&Command::with_params(NODE_GROUP, json!({ "nodes": [ids[0].0, ids[1].0], "name": "G" }))).unwrap()["group"].as_u64().unwrap();
    session.run(&Command::with_params(NODE_UPDATE, json!({ "nodes": [group], "set": { "visible": false } }))).unwrap();
    let child = session.document.get_object(ids[0]).unwrap();
    assert!(child.pickable, "child pickable must stay true when only the group is hidden");
    assert!(!session.document.get_object(ObjectId(group)).unwrap().pickable);
    assert!(session.document.get_object(ids[1]).unwrap().pickable);
    assert!(effective_pickable(&session.document, ids[2]));
    assert!(!effective_pickable(&session.document, ObjectId(group)));
    assert!(!effective_pickable(&session.document, ids[0]));
    assert!(!effective_pickable(&session.document, ids[1]));
    let tree = session.run(&Command::new(NODE_TREE_QUERY)).unwrap();
    let rows = tree["tree"].as_array().unwrap();
    assert!(rows.iter().any(|row| row["id"] == ids[0].0 && row["flags"]["visible"] == json!(true)));
    assert!(rows.iter().any(|row| row["id"] == group && row["flags"]["visible"] == json!(false)));
    assert!(!is_drawn(&session.document, ids[0]));
    assert!(!is_drawn(&session.document, ObjectId(group)));
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(session.document.get_object(ObjectId(group)).unwrap().pickable);
    assert!(session.document.get_object(ids[0]).unwrap().pickable);
    assert!(effective_pickable(&session.document, ids[0]));
    assert!(is_drawn(&session.document, ids[0]));
}

#[test]
fn empty_group_is_allowed_with_flag() {
    let (mut session, _) = session_with_shapes();
    let before = session.document.next_object_id;
    let reply = session.run(&Command::with_params(NODE_GROUP, json!({ "empty": true, "name": "Empty" }))).unwrap();
    let group = ObjectId(reply["group"].as_u64().unwrap());
    assert!(matches!(session.document.get_object(group).unwrap().kind, ObjectKind::Group));
    assert_eq!(session.document.get_object(group).unwrap().name, "Empty");
    assert!(session.document.next_object_id > before);
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(session.document.get_object(group).is_none());
    assert!(session.document.next_object_id > before);
}

#[test]
fn reparent_rejects_cycles_and_inserts_after_parent() {
    let (mut session, ids) = session_with_shapes();
    let group = ObjectId(session.run(&Command::with_params(NODE_GROUP, json!({ "nodes": [ids[0].0, ids[1].0], "name": "G" }))).unwrap()["group"].as_u64().unwrap());
    assert!(would_cycle(&session, group, ids[0]));
    let err = session.run(&Command::with_params(NODE_REPARENT, json!({ "nodes": [group], "parent": ids[0].0 }))).unwrap_err();
    assert_eq!(err.code, "INVALID_PARAMS");
    session.run(&Command::with_params(NODE_REPARENT, json!({ "nodes": [ids[2].0], "parent": group.0 }))).unwrap();
    assert_eq!(session.document.get_object(ids[2]).unwrap().parent_id, Some(group));
    let order: Vec<ObjectId> = session.document.objects.iter().map(|obj| obj.id).collect();
    let parent_at = order.iter().position(|id| *id == group).unwrap();
    let child_at = order.iter().position(|id| *id == ids[2]).unwrap();
    assert!(child_at > parent_at);
    let missing = session.run(&Command::with_params(NODE_REPARENT, json!({ "nodes": [ids[2].0], "parent": 999 }))).unwrap_err();
    assert_eq!(missing.code, "NOT_FOUND");
}

#[test]
fn select_inside_picks_unlocked_pickable_children() {
    let (mut session, ids) = session_with_shapes();
    let group = ObjectId(session.run(&Command::with_params(NODE_GROUP, json!({ "nodes": [ids[0].0, ids[1].0], "name": "G" }))).unwrap()["group"].as_u64().unwrap());
    session.document.objects.iter_mut().find(|obj| obj.id == ids[1]).unwrap().locked = true;
    session.selection.ids = vec![group];
    let reply = session.run(&Command::new(NODE_SELECT_INSIDE)).unwrap();
    let selected: Vec<u64> = reply["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(selected, vec![ids[0].0]);
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.selection.ids, vec![group]);
}
