use dtool_core::document::{Document, Units};
use dtool_core::{ObjectKind, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1))
}

#[test]
fn create_inserts_selected_table_object() {
    let mut s = session();
    let out = s.run_named("table.create", json!({"rows": 3, "cols": 2, "bounds": [120.0, 60.0], "pos": [10.0, 20.0], "header_rows": 1})).unwrap();
    let id = out["object_id"].as_u64().unwrap();
    assert_eq!(out["id"], json!(id));
    assert_eq!(out["ids"], json!([id]));
    assert_eq!(out["table"]["cells"].as_array().unwrap().len(), 6);
    assert_eq!(s.document.objects.len(), 1);
    let obj = &s.document.objects[0];
    assert_eq!(obj.id.0, id);
    assert_eq!(obj.pos, [10.0, 20.0]);
    assert_eq!(obj.size, [120.0, 60.0]);
    assert!(matches!(&obj.kind, ObjectKind::Table(t) if t.header_rows == 1 && t.cells.len() == 6));
    assert_eq!(s.selection.ids.iter().map(|i| i.0).collect::<Vec<_>>(), vec![id]);
}

#[test]
fn create_is_one_undo_step_and_redo_restores() {
    let mut s = session();
    let stories = s.document.stories.len();
    s.run_named("table.create", json!({"rows": 2, "cols": 2, "insert": true})).unwrap();
    assert_eq!(s.document.stories.len(), stories + 4);
    assert!(s.can_execute_named("edit.undo"));
    s.run_named("edit.undo", json!({})).unwrap();
    assert!(s.document.objects.is_empty());
    assert_eq!(s.document.stories.len(), stories);
    assert!(!s.can_execute_named("edit.undo"));
    s.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(s.document.objects.len(), 1);
    assert_eq!(s.document.stories.len(), stories + 4);
}

#[test]
fn create_rejects_bad_params_without_side_effects() {
    let mut s = session();
    assert!(s.run_named("table.create", json!({"rows": 0, "cols": 2})).is_err());
    assert!(s.run_named("table.create", json!({"rows": 2, "cols": 2, "pos": [1.0]})).is_err());
    assert!(s.run_named("table.create", json!({"rows": 2, "cols": 2, "bounds": [0.0, 10.0]})).is_err());
    assert!(s.run_named("table.create", json!({"rows": 2, "cols": 2, "insert": "yes"})).is_err());
    assert!(s.document.objects.is_empty());
    assert!(!s.can_execute_named("edit.undo"));
    assert!(s.can_execute_named("table.create"));
    assert!(!Session::default().can_execute_named("table.create"));
}

#[test]
fn query_by_id_and_by_selection() {
    let mut s = session();
    let id = s.run_named("table.create", json!({"rows": 2, "cols": 3, "bounds": [90.0, 40.0], "footer_rows": 1, "insert": true})).unwrap()["id"].as_u64().unwrap();
    let q = s.run_named("table.query", json!({"id": id})).unwrap();
    assert_eq!(q["id"], json!(id));
    assert_eq!(q["rows"], json!(2));
    assert_eq!(q["cols"], json!(3));
    assert_eq!(q["col_widths_pt"], json!([30.0, 30.0, 30.0]));
    assert_eq!(q["footer_rows"], json!([1]));
    assert_eq!(q["cells"].as_array().unwrap().len(), 6);
    assert_eq!(q["cells"][4]["address"], json!("B2"));
    let by_sel = s.run_named("table.query", json!({})).unwrap();
    assert_eq!(by_sel, q);
    let one = s.run_named("table.query", json!({"id": id, "cell": "c2"})).unwrap();
    assert_eq!(one["cell"], json!("c2"));
    assert_eq!(one["cells"].as_array().unwrap().len(), 1);
    assert_eq!(one["cells"][0]["address"], json!("B1"));
}

#[test]
fn query_rejects_non_tables_and_unknown_cells() {
    let mut s = session();
    assert!(!s.can_execute_named("table.query"));
    assert!(s.run_named("table.query", json!({})).is_err());
    s.run_named("object.create", json!({"name": "o", "pos": [0.0, 0.0], "size": [5.0, 5.0]})).unwrap();
    let shape = s.document.objects[0].id.0;
    assert!(s.run_named("table.query", json!({"id": shape})).is_err());
    let id = s.run_named("table.create", json!({"rows": 1, "cols": 1, "insert": true})).unwrap()["id"].as_u64().unwrap();
    assert!(s.run_named("table.query", json!({"id": id, "cell": "c99"})).is_err());
    assert!(s.run_named("table.query", json!({"id": id, "bogus": 1})).is_err());
    assert!(s.can_execute_named("table.query"));
}

#[test]
fn node_only_create_leaves_document_objects_and_undo_alone() {
    let mut s = session();
    let out = s.run_named("table.create", json!({"rows": 2, "cols": 2})).unwrap();
    assert!(out.get("object_id").is_none());
    assert!(s.document.objects.is_empty());
    assert!(!s.can_execute_named("edit.undo"));
}
