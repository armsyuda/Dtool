use dtool_core::document::{Document, Units};
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::Session;
use serde_json::json;

fn session() -> Session { Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1)) }

#[test]
fn table_create_with_pos_inserts_object() {
    let mut s = session();
    let out = s.run_named("table.create", json!({"rows": 2, "cols": 3, "bounds": [300.0, 80.0], "pos": [50.0, 60.0]})).unwrap();
    let id = out["object_id"].as_u64().unwrap();
    let object = s.document.objects.iter().find(|o| o.id == ObjectId(id)).unwrap();
    assert_eq!(object.pos, [50.0, 60.0]);
    assert_eq!(object.size, [300.0, 80.0]);
    match &object.kind { ObjectKind::Table(t) => assert_eq!(t.cells.len(), 6), _ => panic!("expected table") }
    s.run_named("table.row.insert", json!({"object_id": id, "row": 0})).unwrap();
    let object = s.document.objects.iter().find(|o| o.id == ObjectId(id)).unwrap();
    match &object.kind { ObjectKind::Table(t) => assert_eq!(t.row_heights_pt.len(), 3), _ => panic!("expected table") }
}

#[test]
fn table_create_insert_flag_inserts_at_origin() {
    let mut s = session();
    let out = s.run_named("table.create", json!({"rows": 1, "cols": 1, "insert": true})).unwrap();
    assert!(out["object_id"].as_u64().is_some());
    assert_eq!(s.document.objects.len(), 1);
    assert_eq!(s.document.objects[0].pos, [0.0, 0.0]);
}

#[test]
fn table_create_without_pos_keeps_node_only() {
    let mut s = session();
    let out = s.run_named("table.create", json!({"rows": 2, "cols": 2})).unwrap();
    assert!(out.get("object_id").is_none());
    assert!(s.document.objects.is_empty());
}

#[test]
fn two_inserted_tables_get_distinct_ids() {
    let mut s = session();
    let a = s.run_named("table.create", json!({"rows": 1, "cols": 1, "pos": [0.0, 0.0]})).unwrap()["object_id"].as_u64().unwrap();
    let b = s.run_named("table.create", json!({"rows": 1, "cols": 1, "pos": [10.0, 10.0]})).unwrap()["object_id"].as_u64().unwrap();
    assert_ne!(a, b);
}
