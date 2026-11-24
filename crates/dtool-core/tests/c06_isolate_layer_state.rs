use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::scene::{effective_pickable, is_drawn_in_view};
use dtool_protocol::Command;
use serde_json::json;

fn session_with_group() -> (Session, ObjectId, ObjectId, ObjectId, ObjectId) {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let a = session.document.create_object(Object { name: "A".into(), pos: [0.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    let b = session.document.create_object(Object { name: "B".into(), pos: [20.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    let c = session.document.create_object(Object { name: "C".into(), pos: [40.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    let group = ObjectId(session.run(&Command::with_params("node.group", json!({ "nodes": [a.0, b.0], "name": "G" }))).unwrap()["group"].as_u64().unwrap());
    (session, group, a, b, c)
}

fn flags(session: &Session) -> Vec<(u64, bool, bool)> {
    session.document.objects.iter().map(|object| (object.id.0, object.pickable, object.locked)).collect()
}

fn listed(session: &mut Session) -> Vec<u64> {
    session
        .run(&Command::new("canvas.display_list"))
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["object_id"].as_u64().unwrap())
        .collect()
}

#[test]
fn isolate_is_side_check_and_does_not_change_pickable_flags() {
    let (mut session, group, a, b, c) = session_with_group();
    let before_flags = flags(&session);
    let before_version = session.document.version;
    session.run(&Command::with_params("node.isolate", json!({ "nodes": [group.0] }))).unwrap();
    assert_eq!(flags(&session), before_flags);
    assert_eq!(session.document.version, before_version);
    assert_eq!(session.view.isolate_target(), Some(group));
    assert!(effective_pickable(&session.document, c));
    assert!(!is_drawn_in_view(&session.document, &session.view, c));
    assert!(is_drawn_in_view(&session.document, &session.view, group));
    assert!(is_drawn_in_view(&session.document, &session.view, a));
    assert!(is_drawn_in_view(&session.document, &session.view, b));
    let items = listed(&mut session);
    assert!(items.contains(&group.0) && items.contains(&a.0) && items.contains(&b.0));
    assert!(!items.contains(&c.0));
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.view.isolate_target(), None);
    assert_eq!(flags(&session), before_flags);
    assert!(is_drawn_in_view(&session.document, &session.view, c));
    assert!(listed(&mut session).contains(&c.0));
}

#[test]
fn isolate_end_undo_reenters_without_touching_pickable() {
    let (mut session, group, _, _, c) = session_with_group();
    let before_flags = flags(&session);
    session.run(&Command::with_params("node.isolate", json!({ "nodes": [group.0] }))).unwrap();
    session.run(&Command::new("node.isolate.end")).unwrap();
    assert_eq!(session.view.isolate_target(), None);
    assert!(is_drawn_in_view(&session.document, &session.view, c));
    assert_eq!(flags(&session), before_flags);
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.view.isolate_target(), Some(group));
    assert!(!is_drawn_in_view(&session.document, &session.view, c));
    assert_eq!(flags(&session), before_flags);
}

#[test]
fn layer_state_restore_is_one_undo_and_saves_visible_locked() {
    let (mut session, group, a, _, c) = session_with_group();
    session.run(&Command::with_params("node.update", json!({ "nodes": [c.0], "set": { "visible": false } }))).unwrap();
    session.run(&Command::with_params("node.lock_state", json!({ "nodes": [a.0], "state": "locked" }))).unwrap();
    session.run(&Command::with_params("layer.state.save", json!({ "name": "Setup" }))).unwrap();
    session.run(&Command::with_params("node.show_all", json!({}))).unwrap();
    session.run(&Command::with_params("node.unlock_all", json!({}))).unwrap();
    assert!(session.document.get_object(c).unwrap().pickable);
    assert!(!session.document.get_object(a).unwrap().locked);
    session.run(&Command::with_params("layer.state.restore", json!({ "name": "Setup" }))).unwrap();
    assert!(!session.document.get_object(c).unwrap().pickable);
    assert!(session.document.get_object(a).unwrap().locked);
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(session.document.get_object(c).unwrap().pickable);
    assert!(!session.document.get_object(a).unwrap().locked);
    let listed = session.run(&Command::new("layer.state.list")).unwrap();
    assert_eq!(listed["states"][0]["name"], "Setup");
    session.run(&Command::with_params("layer.state.rename", json!({ "name": "Setup", "new_name": "Final" }))).unwrap();
    session.run(&Command::with_params("layer.state.delete", json!({ "name": "Final" }))).unwrap();
    let empty = session.run(&Command::new("layer.state.list")).unwrap();
    assert_eq!(empty["states"].as_array().unwrap().len(), 0);
    let _ = group;
}
