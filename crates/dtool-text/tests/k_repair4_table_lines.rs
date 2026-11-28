use dtool_core::document::{Document, Story, StoryId};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::table::geometry::table_grid;
use dtool_core::table::model::{CellFormat, CellId, CellIdAllocator, FrameFormatMask, TableCell, TableFrameFormat, TableNode};
use dtool_core::table::query::table_query_with_lines;
use dtool_text::fonts::{font_generation, note_font_change};
use dtool_text::table_layout::{layout_table, table_lines_with, table_overflow_with};
use dtool_text::FontLibrary;

const LONG: &str = "A very long string that will definitely overflow a small cell and make the layout report overflow for it.";

fn cell(id: u64, col: u32, col_span: u32, merged_into: Option<u64>) -> TableCell {
    TableCell { id: CellId(id), row: 0, col, row_span: 1, col_span, merged_into: merged_into.map(CellId), story: StoryId(id), local: CellFormat::default(), mask: Default::default() }
}

fn document() -> (Document, ObjectId) {
    let mut doc = Document::default();
    doc.stories.push(Story { id: StoryId(1), text: "Hello world".into(), ..Story::default() });
    doc.stories.push(Story { id: StoryId(2), text: LONG.into(), ..Story::default() });
    doc.stories.push(Story { id: StoryId(3), text: "hidden".into(), ..Story::default() });
    doc.stories.push(Story { id: StoryId(4), text: String::new(), ..Story::default() });
    let table = TableNode {
        alloc: CellIdAllocator { high_water: 4 },
        col_widths_pt: vec![80.0, 12.0, 12.0, 40.0],
        row_heights_pt: vec![30.0],
        cells: vec![cell(1, 0, 1, None), cell(2, 1, 2, None), cell(3, 2, 1, Some(2)), cell(4, 3, 1, None)],
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
        text_defaults: None,
    };
    let id = doc.create_object(Object { kind: ObjectKind::Table(table), size: [144.0, 30.0], ..Object::default() });
    (doc, id)
}

fn table(doc: &Document, id: ObjectId) -> &TableNode {
    match &doc.get_object(id).unwrap().kind {
        ObjectKind::Table(t) => t,
        _ => unreachable!(),
    }
}

macro_rules! cell_json {
    ($query:expr, $id:expr) => {
        $query["cells"].as_array().unwrap().iter().find(|c| c["id"].as_str() == Some($id)).unwrap()
    };
}

#[test]
fn k_repair4_lines_match_layout_table() {
    let lib = FontLibrary::system();
    let (doc, id) = document();
    let t = table(&doc, id);
    let layout = layout_table(&lib, &doc, t, &table_grid(t));
    let lines = table_lines_with(&lib, &doc, id).expect("lines");
    assert_eq!(lines.cells.len(), layout.cells.len());
    assert!(!lines.cells.contains_key(&CellId(3)));
    for cell in &layout.cells {
        let got = &lines.cells[&cell.id];
        assert_eq!(got.content_rect, cell.content_rect);
        assert_eq!(got.overflow, cell.overflow);
        assert_eq!(got.lines.len(), cell.lines.len());
        for (i, line) in got.lines.iter().enumerate() {
            assert_eq!(line.baseline, cell.baselines[i]);
            assert_eq!(line.width, cell.lines[i].width);
            assert_eq!(line.glyphs, cell.lines[i].glyphs.len());
            assert_eq!(line.range, cell.ranges[i]);
            assert_eq!((line.ascent, line.descent), (cell.lines[i].ascent, cell.lines[i].descent));
        }
    }
    assert!(lines.cells[&CellId(1)].lines.len() >= 1);
    assert!(lines.cells[&CellId(4)].lines.is_empty());
    let first = &lines.cells[&CellId(1)].lines[0];
    assert_eq!(first.range[0], 0);
    assert!(lines.cells[&CellId(2)].lines.windows(2).all(|w| w[0].range[1] <= w[1].range[0]));
}

#[test]
fn k_repair4_overflow_consistent_and_covered_cell_follows_owner() {
    let lib = FontLibrary::system();
    let (doc, id) = document();
    let lines = table_lines_with(&lib, &doc, id).expect("lines");
    let overflow = table_overflow_with(&lib, &doc, id).expect("overflow");
    for (cell, layout) in &lines.cells {
        assert_eq!(overflow[cell], layout.overflow);
    }
    assert!(lines.cells[&CellId(2)].overflow);
    assert!(!lines.cells[&CellId(1)].overflow);
    let query = table_query_with_lines(&doc, id, None, Some(&lines));
    assert_eq!(cell_json!(query, "c3")["layout"].is_null(), true);
    assert_eq!(cell_json!(query, "c3")["overflow"].as_bool(), Some(true));
    for c in query["cells"].as_array().unwrap() {
        if !c["layout"].is_null() {
            assert_eq!(c["layout"]["overflow"], c["overflow"]);
            assert_eq!(c["layout"]["lines"].as_array().unwrap().len(), lines.cells[&CellId(c["id"].as_str().unwrap()[1..].parse().unwrap())].lines.len());
        }
    }
}

#[test]
fn k_repair4_font_generation_follows_font_change() {
    let lib = FontLibrary::system();
    let (doc, id) = document();
    let before = table_lines_with(&lib, &doc, id).expect("lines").font_generation;
    assert!(before <= font_generation());
    let bumped = note_font_change(1, 2);
    assert!(bumped > before);
    let after = table_lines_with(&lib, &doc, id).expect("lines").font_generation;
    assert!(after >= bumped);
    assert!(dtool_core::text_lines::font_generation() >= bumped);
    let same = note_font_change(3, 3);
    assert!(same >= bumped);
}

#[test]
fn k_repair4_installed_provider_reaches_live_query() {
    dtool_text::canvas_text::install();
    let (doc, id) = document();
    let floor = font_generation();
    let live = dtool_core::query::table_query_live(&doc, id, None);
    assert!(live["font_generation"].as_u64().expect("font_generation") >= floor);
    assert!(cell_json!(live, "c1")["layout"]["lines"].as_array().expect("lines").len() >= 1);
    assert_eq!(cell_json!(live, "c3")["layout"].is_null(), true);
    assert_eq!(cell_json!(live, "c3")["overflow"], cell_json!(live, "c2")["overflow"]);
    assert!(table_lines_with(&FontLibrary::shared(), &doc, ObjectId(9999)).is_none());
}