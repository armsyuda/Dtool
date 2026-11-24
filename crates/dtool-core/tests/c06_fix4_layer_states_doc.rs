use dtool_core::command::Session;
use dtool_core::commands::layer_state;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_protocol::Command;
use serde_json::json;

fn session() -> (Session, ObjectId, ObjectId) {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let a = session.document.create_object(Object { name: "A".into(), pos: [0.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    let b = session.document.create_object(Object { name: "B".into(), pos: [20.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    (session, a, b)
}

fn names(session: &mut Session) -> Vec<String> {
    session.run(&Command::new("layer.state.list")).unwrap()["states"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn c06_fix4_layer_states_live_on_the_document_not_the_thread() {
    let (mut first, _, b) = session();
    let (mut second, _, _) = session();
    first.run(&Command::with_params("node.update", json!({ "nodes": [b.0], "set": { "visible": false } }))).unwrap();
    first.run(&Command::with_params("layer.state.save", json!({ "name": "Hidden B" }))).unwrap();
    assert_eq!(names(&mut first), vec!["Hidden B".to_string()]);
    assert!(names(&mut second).is_empty());
    assert_eq!(first.document.layer_states.len(), 1);
    assert_eq!(layer_state::states(&first), first.document.layer_states);
    assert!(!second.run(&Command::with_params("layer.state.restore", json!({ "name": "Hidden B" }))).is_ok());
}

#[test]
fn c06_fix4_layer_states_round_trip_with_the_document() {
    let (mut s, a, b) = session();
    s.run(&Command::with_params("node.lock_state", json!({ "nodes": [a.0], "state": "locked" }))).unwrap();
    s.run(&Command::with_params("layer.state.save", json!({ "name": "Locked A" }))).unwrap();
    let saved = serde_json::to_value(&s.document).unwrap();
    let loaded: Document = serde_json::from_value(saved).unwrap();
    assert_eq!(loaded.layer_states, s.document.layer_states);
    let flags = &loaded.layer_states[0].flags;
    assert!(flags.iter().any(|(id, _, locked)| *id == a && *locked));
    assert!(flags.iter().any(|(id, visible, locked)| *id == b && *visible && !*locked));
    let mut reopened = Session::new(loaded);
    reopened.run(&Command::with_params("node.unlock_all", json!({}))).unwrap();
    reopened.run(&Command::with_params("layer.state.restore", json!({ "name": "Locked A" }))).unwrap();
    assert!(reopened.document.get_object(a).unwrap().locked);
    reopened.run(&Command::new("edit.undo")).unwrap();
    assert!(!reopened.document.get_object(a).unwrap().locked);
}

#[test]
fn c06_fix4_save_rename_delete_are_each_one_undo() {
    let (mut s, _, _) = session();
    s.run(&Command::with_params("layer.state.save", json!({ "name": "One" }))).unwrap();
    s.run(&Command::with_params("layer.state.rename", json!({ "name": "One", "new_name": "Two" }))).unwrap();
    s.run(&Command::with_params("layer.state.delete", json!({ "name": "Two" }))).unwrap();
    assert!(names(&mut s).is_empty());
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(names(&mut s), vec!["Two".to_string()]);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(names(&mut s), vec!["One".to_string()]);
    s.run(&Command::new("edit.undo")).unwrap();
    assert!(names(&mut s).is_empty());
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(names(&mut s), vec!["One".to_string()]);
}