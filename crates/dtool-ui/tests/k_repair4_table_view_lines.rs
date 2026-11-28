use dtool_core::document::{Document, Story, StoryId};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::table::model::{CellFormat, CellId, CellIdAllocator, FrameFormatMask, TableCell, TableFrameFormat, TableNode};
use dtool_core::{Command, Scope, Workspace};
use dtool_tools::dtool_text::FontLibrary;
use dtool_ui::table::snapshot::{table_query_with_layout, TableSnapshots};
use dtool_ui::table::view::{TableView, ERROR_KEY};
use serde_json::json;

fn cell(id: u64, col: u32, col_span: u32, merged_into: Option<u64>) -> TableCell {
    TableCell { id: CellId(id), row: 0, col, row_span: 1, col_span, merged_into: merged_into.map(CellId), story: StoryId(id), local: CellFormat::default(), mask: Default::default() }
}

fn document() -> (Document, dtool_core::ObjectId) {
    let mut doc = Document::default();
    doc.stories.push(Story { id: StoryId(1), text: "Hello world".into(), ..Story::default() });
    doc.stories.push(Story { id: StoryId(2), text: "A long cell text that wraps over several lines in a narrow merged cell".into(), ..Story::default() });
    doc.stories.push(Story { id: StoryId(3), text: String::new(), ..Story::default() });
    let table = TableNode {
        alloc: CellIdAllocator { high_water: 3 },
        col_widths_pt: vec![80.0, 15.0, 15.0],
        row_heights_pt: vec![30.0],
        cells: vec![cell(1, 0, 1, None), cell(2, 1, 2, None), cell(3, 2, 1, Some(2))],
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
        text_defaults: None,
    };
    let id = doc.create_object(Object { kind: ObjectKind::Table(table), size: [110.0, 30.0], ..Object::default() });
    (doc, id)
}

#[test]
fn k_repair4_view_parses_layout_lines_from_query() {
    let lib = FontLibrary::system();
    let (doc, id) = document();
    let mut query = table_query_with_layout(&lib, &doc, id, None);
    query["generation"] = json!(1);
    query["document_version"] = json!(doc.version);
    let view = TableView::from_query(&query).expect("view");
    assert!(view.font_generation.is_some());
    assert_eq!(view.layouts.len(), view.rects.len());
    assert_eq!(view.layout_of(3), view.layouts.get(&2));
    assert!(view.layouts[&1].lines.len() >= 1);
    assert!(view.layouts[&2].lines.len() > 1);
    assert_eq!(view.layouts[&2].overflow, query["cells"][1]["overflow"].as_bool().unwrap());
    let mut broken = query.clone();
    broken["cells"][2]["layout"] = broken["cells"][1]["layout"].clone();
    assert_eq!(TableView::from_query(&broken).unwrap_err(), ERROR_KEY);
    let mut stale = query.clone();
    stale.as_object_mut().unwrap().remove("font_generation");
    assert_eq!(TableView::from_query(&stale).unwrap_err(), ERROR_KEY);
}

#[test]
fn k_repair4_snapshot_views_carry_lines() {
    let mut workspace = Workspace::default();
    let context = workspace.new_document("lines");
    let (_, created) = workspace.run(Scope::active(), Command::with_params("table.create", json!({ "rows": 2, "cols": 2, "insert": true })));
    let created = created.expect("table.create");
    let table = created.get("id").or_else(|| created.get("object_id")).and_then(|v| v.as_u64()).expect("id");
    let mut snapshots = TableSnapshots::new();
    assert!(snapshots.refresh(&workspace));
    let view = snapshots.views(context).expect("views").views.get(&table).expect("table").as_ref().expect("view ok");
    assert!(view.font_generation.is_some());
    assert_eq!(view.layouts.len(), 4);
    for rect in &view.rects {
        let layout = view.layout_of(rect.id).expect("layout");
        assert!(layout.content_rect.iter().all(|v| v.is_finite()));
    }
}