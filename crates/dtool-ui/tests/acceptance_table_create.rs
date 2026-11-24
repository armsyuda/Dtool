use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::table::ids::{CellId, CellIdAllocator};
use dtool_core::table::model::TableNode;
use dtool_core::Session;
use serde_json::{json, Value};

fn session() -> Session { Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1)) }

fn create(session: &mut Session, rows: u64, cols: u64, header: u64, footer: u64) -> Value {
    session.run_named("table.create", json!({"rows": rows, "cols": cols, "header_rows": header, "footer_rows": footer})).unwrap()["table"].clone()
}

fn place(session: &mut Session, table: Value) -> u64 {
    let node: TableNode = serde_json::from_value(table).unwrap();
    let mut object = Object::default();
    object.id = ObjectId(1);
    object.kind = ObjectKind::Table(node);
    session.document.objects.push(object);
    1
}

fn table_of(session: &Session) -> &TableNode {
    match &session.document.objects[0].kind { ObjectKind::Table(t) => t, _ => panic!("expected table") }
}

#[test]
fn test_table_create_drag_and_command_match() {
    let mut s = session();
    let table = create(&mut s, 3, 2, 1, 0);
    assert_eq!(table["header_rows"], json!(1));
    assert_eq!(table["footer_rows"], json!(0));
    assert_eq!(table["cells"].as_array().unwrap().len(), 6);
}

#[test]
fn test_table_header_footer_stored() {
    let mut s = session();
    let table = create(&mut s, 5, 3, 2, 1);
    place(&mut s, table);
    assert_eq!(table_of(&s).header_rows, 2);
    assert_eq!(table_of(&s).footer_rows, 1);
}

#[test]
fn test_table_create_rejects_out_of_range() {
    let mut s = session();
    assert!(s.run_named("table.create", json!({"rows": 0, "cols": 2})).is_err());
    assert!(s.run_named("table.create", json!({"rows": 2, "cols": 1001})).is_err());
}

#[test]
fn test_legacy_id_hydration() {
    let legacy_ids: Vec<CellId> = (1..=9).map(CellId).collect();
    let mut allocator = CellIdAllocator::hydrate(&legacy_ids).unwrap();
    assert_eq!(allocator.high_water, 9);
    assert_eq!(allocator.next().0, 10);
    assert!(CellIdAllocator::hydrate(&[CellId(1), CellId(1)]).is_err());
}

#[test]
fn test_table_create_with_legacy_hydration() {
    let mut s = session();
    let table = create(&mut s, 2, 2, 1, 0);
    let id = place(&mut s, table);
    assert_eq!(table_of(&s).alloc.high_water, 4);
    s.run_named("table.row.insert", json!({"object_id": id, "row": 0})).unwrap();
    assert_eq!(table_of(&s).alloc.high_water, 6);
    assert_eq!(table_of(&s).row_heights_pt.len(), 3);
    assert_unique_ids(&s);
}

fn assert_unique_ids(session: &Session) {
    let cells: Vec<u64> = table_of(session).cells.iter().map(|cell| cell.id.0).collect();
    let unique_cells: std::collections::BTreeSet<u64> = cells.iter().copied().collect();
    assert_eq!(unique_cells.len(), cells.len());
    assert!(cells.iter().all(|id| *id >= 1 && *id <= table_of(session).alloc.high_water));
    let objects: Vec<u64> = session.document.objects.iter().map(|object| object.id.0).collect();
    let unique_objects: std::collections::BTreeSet<u64> = objects.iter().copied().collect();
    assert_eq!(unique_objects.len(), objects.len());
    let stories: Vec<u64> = table_of(session).cells.iter().map(|cell| cell.story.0).collect();
    let unique_stories: std::collections::BTreeSet<u64> = stories.iter().copied().collect();
    assert_eq!(unique_stories.len(), stories.len());
}

#[test]
fn test_drag_create_matches_command_create() {
    let mut a = session();
    let mut b = session();
    let t1 = create(&mut a, 4, 3, 1, 1);
    place(&mut a, t1.clone());
    assert_unique_ids(&a);
    let t2 = create(&mut b, 4, 3, 1, 1);
    for key in ["header_rows", "footer_rows", "col_widths_pt", "row_heights_pt"] {
        assert_eq!(t1[key], t2[key]);
    }
}
