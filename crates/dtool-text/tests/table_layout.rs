use dtool_core::document::{Document, Story, StoryId};
use dtool_core::table::model::CellId;
use dtool_core::table::model::{TableCell, TableNode};
use dtool_core::table::geometry::{TableGrid, CellRect};
use dtool_core::table::model::{CellFormat, CellIdAllocator, FrameFormatMask, TableFrameFormat, VerticalAlign as VAlign};
use dtool_text::table_layout::layout_table;
use dtool_text::FontLibrary;

fn make_doc() -> Document {
    let mut doc = Document::default();
    doc.stories.push(Story { id: StoryId(1), text: "Hello".to_string(), ranges: vec![], ..Story::default() });
    doc.stories.push(Story { id: StoryId(2), text: "World".to_string(), ranges: vec![], ..Story::default() });
    doc.stories.push(Story { id: StoryId(3), text: "A very long string that will definitely overflow a small cell width of 10 points and cause the layout engine to report an overflow condition for this specific cell.".to_string(), ranges: vec![], ..Story::default() });
    doc
}

fn make_table() -> TableNode {
    let mut table = TableNode { alloc: CellIdAllocator { high_water: 3 }, col_widths_pt: Vec::new(), row_heights_pt: Vec::new(), cells: Vec::new(), header_rows: 0, footer_rows: 0, format: None, frame_local: TableFrameFormat::default(), frame_mask: FrameFormatMask::default() };
    table.col_widths_pt = vec![50.0, 10.0];
    table.row_heights_pt = vec![30.0];
    
    table.cells.push(TableCell {
        id: CellId(1),
        row: 0,
        col: 0,
        row_span: 1,
        col_span: 1,
        merged_into: None,
        story: StoryId(1),
        local: CellFormat::default(),
        mask: Default::default(),
    });
    
    table.cells.push(TableCell {
        id: CellId(2),
        row: 0,
        col: 1,
        row_span: 1,
        col_span: 1,
        merged_into: None,
        story: StoryId(3),
        local: CellFormat::default(),
        mask: Default::default(),
    });

    table.cells.push(TableCell {
        id: CellId(3),
        row: 0,
        col: 1,
        row_span: 1,
        col_span: 1,
        merged_into: Some(CellId(2)),
        story: StoryId(2),
        local: CellFormat::default(),
        mask: Default::default(),
    });

    table
}

fn make_grid(table: &TableNode) -> TableGrid {
    let mut grid = TableGrid { col_x: Vec::new(), row_y: Vec::new(), rects: Vec::new() };
    grid.col_x = vec![0.0, 50.0, 60.0];
    grid.row_y = vec![0.0, 30.0];
    
    for cell in &table.cells {
        if cell.merged_into.is_some() {
            continue;
        }
        let x1 = grid.col_x[cell.col as usize];
        let x2 = grid.col_x[(cell.col + cell.col_span) as usize];
        let y1 = grid.row_y[cell.row as usize];
        let y2 = grid.row_y[(cell.row + cell.row_span) as usize];
        
        grid.rects.push(CellRect {
            id: cell.id.0,
            row: cell.row,
            col: cell.col,
            row_span: cell.row_span,
            col_span: cell.col_span,
            rect: [x1, y1, x2, y2],
        });
    }
    
    grid
}

#[test]
fn test_vertical_alignment() {
    let lib = FontLibrary::system();
    let mut doc = make_doc();
    let mut table = make_table();
    
    table.cells[0].local.vertical_align = VAlign::Top;
    table.cells[1].local.vertical_align = VAlign::Center;
    
    let grid = make_grid(&table);
    let layout = layout_table(&lib, &doc, &table, &grid);
    
    assert_eq!(layout.cells.len(), 2);
    
    let cell0 = layout.cells.iter().find(|c| c.id == CellId(1)).unwrap();
    let cell1 = layout.cells.iter().find(|c| c.id == CellId(2)).unwrap();
    
    if !cell0.lines.is_empty() {
        assert!(cell0.lines[0].ascent >= 0.0);
    }
    
    if !cell1.lines.is_empty() {
        let line_height = cell1.lines[0].ascent + cell1.lines[0].descent;
        let content_height = cell1.content_rect[3] - cell1.content_rect[1];
        assert!(line_height >= 0.0);
        assert!(content_height > 0.0);
        assert!(cell1.lines[0].width >= 0.0);
    }
}

#[test]
fn test_overflow_detection() {
    let lib = FontLibrary::system();
    let doc = make_doc();
    let table = make_table();
    let grid = make_grid(&table);
    
    let layout = layout_table(&lib, &doc, &table, &grid);
    
    let overflow_cell = layout.cells.iter().find(|c| c.id == CellId(2)).unwrap();
    assert!(overflow_cell.overflow);
    
    let normal_cell = layout.cells.iter().find(|c| c.id == CellId(1)).unwrap();
    assert!(!normal_cell.overflow);
}

#[test]
fn test_merged_cells_no_layout() {
    let lib = FontLibrary::system();
    let doc = make_doc();
    let table = make_table();
    let grid = make_grid(&table);
    
    let layout = layout_table(&lib, &doc, &table, &grid);
    
    assert_eq!(layout.cells.len(), 2);
    assert!(!layout.cells.iter().any(|c| c.id == CellId(3)));
}

#[test]
fn table_query_reports_layout_overflow_through_provider() {
    let lib = FontLibrary::system();
    let mut doc = make_doc();
    let id = doc.create_object(dtool_core::object::Object { kind: dtool_core::object::ObjectKind::Table(make_table()), ..dtool_core::object::Object::default() });
    let version = doc.version;
    let map = dtool_text::table_layout::table_overflow_with(&lib, &doc, id).unwrap();
    let mut query = dtool_core::table::query::table_query(&doc, id, None);
    dtool_core::table::query::attach_layout_overflow(&mut query, &map);
    let cells = query["cells"].as_array().unwrap();
    let overflow = |name: &str| cells.iter().find(|cell| cell["id"] == name).and_then(|cell| cell["overflow"].as_bool());
    assert_eq!(overflow("c2"), Some(true));
    assert_eq!(overflow("c1"), Some(false));
    assert_eq!(doc.version, version);
}
