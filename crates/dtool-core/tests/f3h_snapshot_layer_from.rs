use dtool_core::document::{Document, Units};
use dtool_core::{ObjectId, ObjectKind, Session};
use serde_json::json;
use std::collections::HashSet;

fn session() -> Session { Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1)) }

fn create(session: &mut Session, name: &str) {
    session.run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
}

#[test]
fn layer_from_snapshot_adds_group_with_copies_and_undoes() {
    let mut s = session();
    create(&mut s, "a");
    create(&mut s, "b");
    let id = s.run_named("history.snapshot.create", json!({"name": "snap"})).unwrap()["id"].as_u64().unwrap();
    create(&mut s, "c");
    let out = s.run_named("history.snapshot.layer_from", json!({"id": id})).unwrap();
    assert_eq!(out["object_count"], json!(2));
    let group = ObjectId(out["group_id"].as_u64().unwrap());
    assert_eq!(s.document.objects.len(), 6);
    let g = s.document.objects.iter().find(|o| o.id == group).unwrap();
    assert!(matches!(g.kind, ObjectKind::Group));
    assert_eq!(g.name, "snap");
    let copies: Vec<_> = s.document.objects.iter().filter(|o| o.parent_id == Some(group)).collect();
    assert_eq!(copies.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    let ids: HashSet<_> = s.document.objects.iter().map(|o| o.id).collect();
    assert_eq!(ids.len(), 6);
    assert!(s.document.objects.iter().all(|o| o.id.0 < s.document.next_object_id));
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.document.objects.len(), 3);
    s.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(s.document.objects.len(), 6);
}

#[test]
fn layer_from_snapshot_errors() {
    let mut s = session();
    let empty = s.run_named("history.snapshot.create", json!({})).unwrap()["id"].as_u64().unwrap();
    assert!(s.run_named("history.snapshot.layer_from", json!({"id": empty})).is_err());
    assert!(s.run_named("history.snapshot.layer_from", json!({"id": empty + 9})).is_err());
    assert!(s.run_named("history.snapshot.layer_from", json!({})).is_err());
    assert!(s.document.objects.is_empty());
}

#[test]
fn layer_from_is_in_catalog() {
    assert!(dtool_protocol::catalog::CommandCatalog::new().all().iter().any(|c| c.name == "history.snapshot.layer_from" && c.mutating && c.undoable));
}
