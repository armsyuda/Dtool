use dtool_core::document::{Document, Story};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::table::ids::{CellId, CellIdAllocator};
use dtool_core::table::model::{CellFormat, FrameFormatMask, StoryId, TableCell, TableFrameFormat, TableNode};
use dtool_core::table::query::{attach_layout_overflow, table_query, table_query_with_overflows};
use dtool_protocol::CommandError;
use std::collections::HashMap;

fn story(id: u64, text: &str) -> Story {
    Story { id: dtool_core::document::StoryId(id), text: text.to_string(), ..Story::default() }
}

fn cell(id: u64, row: u32, col: u32, merged_into: Option<u64>, story: u64) -> TableCell {
    TableCell {
        id: CellId(id),
        row,
        col,
        row_span: 1,
        col_span: if merged_into.is_none() && id == 1 { 2 } else { 1 },
        merged_into: merged_into.map(CellId),
        story: StoryId(story),
        local: CellFormat::default(),
        mask: Default::default(),
    }
}

fn table_doc() -> (Document, ObjectId) {
    let mut table = TableNode {
        alloc: CellIdAllocator { high_water: 2 },
        col_widths_pt: vec![40.0, 40.0],
        row_heights_pt: vec![20.0],
        cells: vec![
            cell(1, 0, 0, None, 10),
            cell(2, 0, 1, Some(1), 99),
        ],
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
        text_defaults: None,
    };
    table.hydrate_cell_ids().unwrap();
    let mut doc = Document::default();
    doc.stories.push(story(10, "owner-text"));
    doc.stories.push(story(99, "covered-text"));
    let id = ObjectId(7);
    doc.objects.push(Object {
        id,
        kind: ObjectKind::Table(table),
        ..Object::default()
    });
    (doc, id)
}

#[test]
fn hydrate_rejects_duplicate_on_model_allocator() {
    let err = CellIdAllocator::hydrate(&[CellId(1), CellId(1)]).unwrap_err();
    assert_eq!(err.code, "TableIdConflict");
}

#[test]
fn hydrate_keeps_saved_high_water_when_larger() {
    let alloc = CellIdAllocator::hydrate_with_saved(&[CellId(2)], 9).unwrap();
    assert_eq!(alloc.high_water, 9);
}

#[test]
fn try_next_out_of_range_at_max() {
    let mut alloc = CellIdAllocator { high_water: u64::MAX };
    let err = alloc.try_next().unwrap_err();
    assert_eq!(err.code, CommandError::out_of_range().code);
}

#[test]
fn table_node_deserialize_rejects_duplicate_cell_ids() {
    let json = r#"{"alloc":{"high_water":2},"col_widths_pt":[10.0],"row_heights_pt":[10.0],"cells":[{"id":1,"row":0,"col":0,"row_span":1,"col_span":1,"merged_into":null,"story":1},{"id":1,"row":0,"col":0,"row_span":1,"col_span":1,"merged_into":null,"story":1}]}"#;
    let result = serde_json::from_str::<TableNode>(json);
    assert!(result.is_err());
}

#[test]
fn covered_cell_rect_follows_owner() {
    let (doc, id) = table_doc();
    let query = table_query(&doc, id, None);
    let cells = query["cells"].as_array().unwrap();
    let owner = cells.iter().find(|c| c["id"] == "c1").unwrap();
    let covered = cells.iter().find(|c| c["id"] == "c2").unwrap();
    assert_eq!(covered["rect"], owner["rect"]);
    assert_eq!(covered["merged_into"], "c1");
    assert_ne!(covered["id"], covered["address"]);
}

#[test]
fn story_id_is_not_a_vec_index() {
    let (doc, id) = table_doc();
    let query = table_query(&doc, id, Some(CellId(2)));
    let cell = &query["cells"][0];
    assert_eq!(cell["text"], "covered-text");
    assert_eq!(cell["story_id"], 99);
}

#[test]
fn overflow_comes_from_layout_map() {
    let (doc, id) = table_doc();
    let mut overflows = HashMap::new();
    overflows.insert(CellId(1), true);
    let query = table_query_with_overflows(&doc, id, None, Some(&overflows));
    let cells = query["cells"].as_array().unwrap();
    assert_eq!(cells.iter().find(|c| c["id"] == "c1").unwrap()["overflow"], true);
    assert_eq!(cells.iter().find(|c| c["id"] == "c2").unwrap()["overflow"], true);
    let mut bare = table_query(&doc, id, None);
    assert_eq!(bare["cells"][0]["overflow"], false);
    attach_layout_overflow(&mut bare, &overflows);
    assert_eq!(bare["cells"].as_array().unwrap().iter().find(|c| c["id"] == "c1").unwrap()["overflow"], true);
}
