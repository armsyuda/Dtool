use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::workspace::Workspace;
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1))
}

#[test]
fn artboard_kind_survives_delete_undo_and_native_round_trip() {
    let mut s = session();
    let id = s.document.create_object(Object { name: "Board".into(), pos: [10.0, 20.0], size: [300.0, 200.0], kind: ObjectKind::Artboard, ..Object::default() });
    s.run(&Command::with_params("object.delete", json!({"id": id.0}))).unwrap();
    assert!(s.document.get_object(id).is_none());
    s.run(&Command::new("edit.undo")).unwrap();
    let restored = s.document.get_object(id).unwrap();
    assert!(matches!(restored.kind, ObjectKind::Artboard));
    assert_eq!(restored.size, [300.0, 200.0]);
    let body = serde_json::to_string(&s.document).unwrap();
    let reopened = Document::from_native_json(&body).unwrap();
    assert!(matches!(reopened.get_object(id).unwrap().kind, ObjectKind::Artboard));
}

#[test]
fn imported_artboard_groups_are_retagged_on_open_idempotently() {
    let mut doc = Document::new([400.0, 400.0], Units::Px, 72.0, 1);
    let board = doc.create_object(Object { name: "Artboard 1".into(), kind: ObjectKind::Group, ..Object::default() });
    let other = doc.create_object(Object { name: "Artboard one".into(), kind: ObjectKind::Group, ..Object::default() });
    let body = serde_json::to_string(&doc).unwrap();
    let first = Document::from_native_json(&body).unwrap();
    assert!(matches!(first.get_object(board).unwrap().kind, ObjectKind::Artboard));
    assert!(matches!(first.get_object(other).unwrap().kind, ObjectKind::Group));
    let second = Document::from_native_json(&serde_json::to_string(&first).unwrap()).unwrap();
    assert!(matches!(second.get_object(board).unwrap().kind, ObjectKind::Artboard));
    assert_eq!(second.objects.len(), 2);
}

#[test]
fn duplicate_cell_ids_are_rejected_on_open_and_workspace_unchanged() {
    let mut s = session();
    let table = s.run(&Command::with_params("table.create", json!({"rows": 1, "cols": 2, "pos": [0.0, 0.0]}))).unwrap()["object_id"].as_u64().unwrap();
    if let ObjectKind::Table(node) = &mut s.document.find_object_mut(table).unwrap().kind {
        let first = node.cells[0].id;
        node.cells[1].id = first;
    }
    let dir = std::env::temp_dir().join(format!("k14_dup_cells_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dup.dtool");
    std::fs::write(&path, serde_json::to_string(&s.document).unwrap()).unwrap();
    assert!(Document::from_native_json(&std::fs::read_to_string(&path).unwrap()).is_err());
    let mut ws = Workspace::new(std::sync::Arc::new(dtool_core::workspace::SharedServices::default()));
    ws.new_document("keep");
    let before = ws.contexts().len();
    assert!(ws.open_path(&path).is_err());
    assert_eq!(ws.contexts().len(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn full_query_embeds_tables_inside_groups_without_version_bump() {
    let mut s = session();
    let group = s.document.create_object(Object { kind: ObjectKind::Group, ..Object::default() });
    let table = s.run(&Command::with_params("table.create", json!({"rows": 2, "cols": 2, "pos": [0.0, 0.0]}))).unwrap()["object_id"].as_u64().unwrap();
    s.document.find_object_mut(table).unwrap().parent_id = Some(group);
    let version = s.document.version;
    let generation = s.generation;
    let reply = s.query(None).unwrap();
    let tables = reply["tables"].as_array().unwrap();
    let entry = tables.iter().find(|entry| entry["id"] == table).unwrap();
    assert_eq!(entry["table"]["cells"].as_array().unwrap().len(), 4);
    assert_eq!(s.document.version, version);
    assert_eq!(s.generation, generation);
    let _ = ObjectId(table);
}
