use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::text_wrap::WrapMode;
use dtool_core::{Command, Session};
use serde_json::json;

fn session() -> (Session, ObjectId, ObjectId) {
    let mut session = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    let a = session.document.create_object(Object { size: [10.0, 10.0], ..Object::default() });
    let b = session.document.create_object(Object { pos: [20.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    (session, a, b)
}

fn mode(session: &Session, id: ObjectId) -> Option<WrapMode> {
    session.document.text_wraps.get(id).and_then(|entry| entry.wrap).map(|wrap| wrap.mode)
}

#[test]
fn edit_batch_of_wrap_sets_is_one_undo_step() {
    let (mut s, a, b) = session();
    let batch = json!({ "commands": [
        { "name": "object.text_wrap.set", "params": { "id": a.0, "mode": "tight" } },
        { "name": "object.text_wrap.set", "params": { "id": b.0, "mode": "tight", "distance": 4.0 } },
        { "name": "object.text_wrap.ignore", "params": { "id": b.0, "on": true } }
    ] });
    s.run(&Command::with_params("edit.batch", batch)).expect("batch");
    assert_eq!((mode(&s, a), mode(&s, b)), (Some(WrapMode::Tight), Some(WrapMode::Tight)));
    assert!(s.document.text_wraps.get(b).unwrap().ignore);
    s.run(&Command::new("edit.undo")).expect("undo");
    assert_eq!((mode(&s, a), mode(&s, b)), (None, None));
    assert!(s.document.text_wraps.get(b).is_none());
    s.run(&Command::new("edit.redo")).expect("redo");
    assert_eq!((mode(&s, a), mode(&s, b)), (Some(WrapMode::Tight), Some(WrapMode::Tight)));
}

#[test]
fn a_failing_wrap_command_rolls_the_whole_batch_back() {
    let (mut s, a, _) = session();
    let batch = json!({ "commands": [
        { "name": "object.text_wrap.set", "params": { "id": a.0, "mode": "square" } },
        { "name": "object.text_wrap.set", "params": { "id": a.0, "distance": -1.0 } }
    ] });
    assert!(s.run(&Command::with_params("edit.batch", batch)).is_err());
    assert_eq!(mode(&s, a), None);
    assert!(!s.undo.can_undo());
}