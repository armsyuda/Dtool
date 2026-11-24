use dtool_core::document::{Document, Story, StoryId};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::table::model::{CellFormat, CellId, CellIdAllocator, FrameFormatMask, TableCell, TableFrameFormat, TableNode};
use dtool_core::table::query::{cell_lines_value, register_table_lines, table_lines, table_query, table_query_with_lines, CellLine, CellLines, TableLines};
use dtool_core::Session;
use serde_json::{json, Value};
use std::collections::HashMap;

const GENERATION: u64 = 41;

fn cell(id: u64, col: u32, col_span: u32, merged_into: Option<u64>) -> TableCell {
    TableCell { id: CellId(id), row: 0, col, row_span: 1, col_span, merged_into: merged_into.map(CellId), story: StoryId(id), local: CellFormat::default(), mask: Default::default() }
}

fn document() -> (Document, ObjectId) {
    let mut doc = Document::default();
    for id in 1..=3 {
        doc.stories.push(Story { id: StoryId(id), text: format!("cell {id}"), ..Story::default() });
    }
    let table = TableNode {
        alloc: CellIdAllocator { high_water: 3 },
        col_widths_pt: vec![40.0, 20.0, 30.0],
        row_heights_pt: vec![25.0],
        cells: vec![cell(1, 0, 1, None), cell(2, 1, 2, None), cell(3, 2, 1, Some(2))],
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
    };
    let id = doc.create_object(Object { kind: ObjectKind::Table(table), size: [90.0, 25.0], ..Object::default() });
    (doc, id)
}

fn layout(overflow: bool, lines: usize) -> CellLines {
    CellLines {
        content_rect: [1.0, 2.0, 39.0, 23.0],
        rotation_deg: 0.0,
        overflow,
        lines: (0..lines).map(|i| CellLine { range: [i * 3, i * 3 + 3], baseline: 10.0 + i as f32 * 12.0, width: 15.0, ascent: 8.0, descent: 2.0, glyphs: 3 }).collect(),
    }
}

fn fake_lines(doc: &Document, id: ObjectId) -> Option<TableLines> {
    doc.get_object(id).filter(|o| matches!(o.kind, ObjectKind::Table(_)))?;
    Some(TableLines { font_generation: GENERATION, cells: HashMap::from([(CellId(1), layout(false, 1)), (CellId(2), layout(true, 2))]) })
}

fn cell_json<'a>(query: &'a Value, id: &str) -> &'a Value {
    query["cells"].as_array().unwrap().iter().find(|c| c["id"] == json!(id)).unwrap()
}

#[test]
fn k_repair4_lines_attach_owner_layout_and_font_generation() {
    let (doc, id) = document();
    let lines = fake_lines(&doc, id).unwrap();
    let query = table_query_with_lines(&doc, id, None, Some(&lines));
    assert_eq!(query["font_generation"], json!(GENERATION));
    assert_eq!(cell_json(&query, "c1")["layout"], cell_lines_value(&layout(false, 1)));
    assert_eq!(cell_json(&query, "c2")["layout"]["lines"].as_array().unwrap().len(), 2);
    assert_eq!(cell_json(&query, "c2")["layout"]["lines"][1], json!({ "range": [3, 6], "baseline": 22.0, "width": 15.0, "ascent": 8.0, "descent": 2.0, "glyphs": 3 }));
    assert_eq!(cell_json(&query, "c3")["layout"], Value::Null);
    assert_eq!(cell_json(&query, "c3")["overflow"], json!(true));
    assert_eq!(cell_json(&query, "c2")["overflow"], json!(true));
    assert_eq!(cell_json(&query, "c1")["overflow"], json!(false));
    for c in query["cells"].as_array().unwrap() {
        if !c["layout"].is_null() {
            assert_eq!(c["layout"]["overflow"], c["overflow"]);
        }
    }
}

#[test]
fn k_repair4_no_provider_data_keeps_query_unchanged() {
    let (doc, id) = document();
    let bare = table_query(&doc, id, None);
    assert_eq!(table_query_with_lines(&doc, id, None, None), bare);
    assert!(bare.get("font_generation").is_none());
    assert!(cell_json(&bare, "c1").get("layout").is_none());
    let version = doc.version;
    let _ = table_query_with_lines(&doc, id, None, fake_lines(&doc, id).as_ref());
    assert_eq!(doc.version, version);
}

#[test]
fn k_repair4_cell_filter_and_missing_owner_layout() {
    let (doc, id) = document();
    let mut lines = fake_lines(&doc, id).unwrap();
    lines.cells.remove(&CellId(1));
    let one = table_query_with_lines(&doc, id, Some(CellId(3)), Some(&lines));
    assert_eq!(one["cells"].as_array().unwrap().len(), 1);
    assert_eq!(one["font_generation"], json!(GENERATION));
    assert_eq!(cell_json(&one, "c3")["overflow"], json!(true));
    let all = table_query_with_lines(&doc, id, None, Some(&lines));
    assert_eq!(cell_json(&all, "c1")["layout"], Value::Null);
    assert_eq!(cell_json(&all, "c1")["overflow"], json!(false));
}

#[test]
fn k_repair4_registered_provider_reaches_table_query_command() {
    assert!(register_table_lines(fake_lines));
    assert!(register_table_lines(fake_lines));
    let (doc, id) = document();
    assert_eq!(table_lines(&doc, id).unwrap().font_generation, GENERATION);
    assert!(table_lines(&doc, ObjectId(9999)).is_none());
    let live = dtool_core::query::table_query_live(&doc, id, None);
    assert_eq!(live["font_generation"], json!(GENERATION));
    assert_eq!(cell_json(&live, "c3")["layout"], Value::Null);
    let mut session = Session::new(doc);
    session.has_document = true;
    let version = session.document.version;
    let out = session.run_named("table.query", json!({ "id": id.0 })).expect("table.query");
    assert_eq!(out["font_generation"], json!(GENERATION));
    assert_eq!(cell_json(&out, "c2")["layout"]["overflow"], json!(true));
    assert_eq!(session.document.version, version);
}