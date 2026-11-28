use dtool_core::document::{Document, Story, StoryId};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::table::model::{CellFormat, CellId, CellIdAllocator, FrameFormatMask, TableCell, TableFrameFormat, TableNode};
use dtool_core::workspace::{SharedServices, Workspace};
use dtool_protocol::Command;
use dtool_tools::dtool_text::FontLibrary;
use dtool_ui::table::snapshot::{layout_overflows, table_query_with_layout, TableSnapshots};
use serde_json::json;
use std::sync::Arc;

const LONG: &str = "A very long string that will definitely overflow a small cell width of 10 points and cause the layout engine to report an overflow condition for this specific cell.";

fn cell(id: u64, col: u32, story: u64, merged_into: Option<u64>) -> TableCell {
    TableCell { id: CellId(id), row: 0, col, row_span: 1, col_span: 1, merged_into: merged_into.map(CellId), story: StoryId(story), local: CellFormat::default(), mask: Default::default() }
}

fn document() -> (Document, dtool_core::ObjectId) {
    let mut doc = Document::default();
    doc.stories.push(Story { id: StoryId(1), text: "Hi".into(), ..Story::default() });
    doc.stories.push(Story { id: StoryId(2), text: String::new(), ..Story::default() });
    doc.stories.push(Story { id: StoryId(3), text: LONG.into(), ..Story::default() });
    let table = TableNode {
        alloc: CellIdAllocator { high_water: 3 },
        col_widths_pt: vec![50.0, 10.0],
        row_heights_pt: vec![30.0],
        cells: vec![cell(1, 0, 1, None), cell(2, 1, 3, None), cell(3, 1, 2, Some(2))],
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
        text_defaults: None,
    };
    let id = doc.create_object(Object { kind: ObjectKind::Table(table), size: [60.0, 30.0], ..Object::default() });
    (doc, id)
}

fn overflow_of(query: &serde_json::Value, id: &str) -> bool {
    query["cells"].as_array().unwrap().iter().find(|c| c["id"] == json!(id)).unwrap()["overflow"].as_bool().unwrap()
}

#[test]
fn layout_overflow_reaches_query_and_covered_cell_follows_owner() {
    let lib = FontLibrary::system();
    let (doc, id) = document();
    let version = doc.version;
    let before = serde_json::to_value(&doc.objects).unwrap();
    let map = layout_overflows(&lib, &doc, id);
    assert_eq!(map.get(&CellId(2)), Some(&true));
    assert_eq!(map.get(&CellId(1)), Some(&false));
    let query = table_query_with_layout(&lib, &doc, id, None);
    assert!(overflow_of(&query, "c2"));
    assert!(overflow_of(&query, "c3"));
    assert!(!overflow_of(&query, "c1"));
    let bare = dtool_core::table::query::table_query(&doc, id, None);
    assert!(!overflow_of(&bare, "c2"));
    assert_eq!(doc.version, version);
    assert_eq!(serde_json::to_value(&doc.objects).unwrap(), before);
}

#[test]
fn snapshot_refresh_records_overflow_without_bumping_versions() {
    let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
    workspace.new_document("A");
    let (_, result) = workspace.run(dtool_core::Scope::active(), Command::with_params("table.create", json!({ "rows": 1, "cols": 2, "bounds": [60.0, 20.0], "insert": true })));
    let table = result.expect("table.create")["object_id"].as_u64().expect("table id");
    let session = workspace.session_mut().expect("session");
    let first = match &session.document.objects.iter().find(|o| o.id.0 == table).unwrap().kind {
        ObjectKind::Table(t) => (t.cells[0].id.0, t.cells[0].story),
        _ => unreachable!(),
    };
    session.document.stories.iter_mut().find(|s| s.id == first.1).unwrap().text = LONG.into();
    let generation = session.generation;
    let version = session.document.version;
    let mut snapshots = TableSnapshots::new();
    assert!(snapshots.refresh(&workspace));
    let id = workspace.active().unwrap();
    let views = snapshots.views(id).unwrap();
    assert_eq!(views.overflows.get(&table), Some(&vec![first.0]));
    let session = workspace.session_mut().unwrap();
    assert_eq!(session.generation, generation);
    assert_eq!(session.document.version, version);
}
#[test]
fn table_query_command_reports_layout_overflow_once_text_providers_are_installed() {
    assert!(dtool_tools::frame_text::install_canvas_text());
    let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
    workspace.new_document("A");
    let (_, result) = workspace.run(dtool_core::Scope::active(), Command::with_params("table.create", json!({ "rows": 1, "cols": 2, "bounds": [60.0, 20.0], "insert": true })));
    let table = result.expect("table.create")["object_id"].as_u64().expect("table id");
    let session = workspace.session_mut().expect("session");
    let (cell, story) = match &session.document.objects.iter().find(|o| o.id.0 == table).unwrap().kind {
        ObjectKind::Table(t) => (t.cells[0].id.0, t.cells[0].story),
        _ => unreachable!(),
    };
    session.document.stories.iter_mut().find(|s| s.id == story).unwrap().text = LONG.into();
    let version = session.document.version;
    let (_, result) = workspace.run(dtool_core::Scope::active(), Command::with_params("table.query", json!({ "object_id": table })));
    let query = result.expect("table.query");
    assert!(overflow_of(&query, &format!("c{cell}")));
    assert_eq!(workspace.session_mut().unwrap().document.version, version);
}