use dtool_core::document::{Document, Units};
use dtool_core::{ObjectId, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1))
}

fn create(s: &mut Session, pos: [f32; 2]) -> ObjectId {
    let id = s.document.next_object_id;
    s.run_named("object.create", json!({"name": "o", "pos": pos, "size": [10.0, 10.0]})).unwrap();
    ObjectId(id)
}

fn pos(s: &Session, id: ObjectId) -> [f32; 2] {
    s.document.get_object(id).unwrap().pos
}

#[test]
fn key_defaults_to_first_selected_and_follows_deselect() {
    let mut s = session();
    let a = create(&mut s, [10.0, 10.0]);
    let b = create(&mut s, [40.0, 20.0]);
    let c = create(&mut s, [80.0, 30.0]);
    assert_eq!(s.run_named("selection.key.query", json!({})).unwrap()["key"], json!(null));
    s.run_named("selection.set", json!({"ids": [b.0]})).unwrap();
    s.run_named("selection.set", json!({"ids": [a.0, c.0], "op": "Add"})).unwrap();
    assert_eq!(s.selection.key(), Some(b));
    assert_eq!(s.run_named("selection.query", json!({})).unwrap()["key"], json!(b.0));
    s.run_named("selection.set", json!({"ids": [b.0], "op": "Subtract"})).unwrap();
    assert_eq!(s.selection.key(), Some(a));
    s.run_named("selection.set", json!({"ids": [], "op": "Replace"})).unwrap();
    assert_eq!(s.selection.key(), None);
}

#[test]
fn key_set_reorders_with_one_undo_and_rejects_unselected() {
    let mut s = session();
    let a = create(&mut s, [10.0, 10.0]);
    let b = create(&mut s, [40.0, 20.0]);
    let c = create(&mut s, [80.0, 30.0]);
    s.run_named("selection.set", json!({"ids": [a.0, b.0]})).unwrap();
    assert!(s.run_named("selection.key.set", json!({"id": c.0})).is_err());
    assert!(s.run_named("selection.key.set", json!({"id": b.0, "x": 1})).is_err());
    let out = s.run_named("selection.key.set", json!({"id": b.0})).unwrap();
    assert_eq!(out["key"], json!(b.0));
    assert_eq!(out["ids"], json!([b.0, a.0]));
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.selection.ids, vec![a, b]);
    assert_eq!(s.selection.key(), Some(a));
}

#[test]
fn align_relative_to_key_uses_selection_key() {
    let mut s = session();
    let a = create(&mut s, [10.0, 10.0]);
    let b = create(&mut s, [40.0, 20.0]);
    let c = create(&mut s, [80.0, 30.0]);
    s.run_named("selection.set", json!({"ids": [a.0, b.0, c.0]})).unwrap();
    s.run_named("selection.key.set", json!({"id": c.0})).unwrap();
    s.run_named("layer.arrange.align.left", json!({"relative_to": "key"})).unwrap();
    assert_eq!(pos(&s, a), [80.0, 10.0]);
    assert_eq!(pos(&s, b), [80.0, 20.0]);
    assert_eq!(pos(&s, c), [80.0, 30.0]);
    s.run_named("edit.undo", json!({})).unwrap();
    s.run_named("layer.arrange.align.top", json!({"relative_to": "key", "ids": [a.0, c.0]})).unwrap();
    assert_eq!(pos(&s, a), [10.0, 30.0]);
    s.run_named("layer.arrange.align.top", json!({"relative_to": "key", "ids": [a.0, b.0], "key_id": b.0})).unwrap();
    assert_eq!(pos(&s, a), [10.0, 20.0]);
    s.run_named("layer.arrange.align.bottom", json!({"relative_to": "key", "ids": [a.0, b.0]})).unwrap();
    assert_eq!(pos(&s, b), [40.0, 20.0]);
}
