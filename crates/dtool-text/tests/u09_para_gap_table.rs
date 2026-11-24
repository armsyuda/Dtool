use dtool_core::document::{Document, Story, StoryId};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::table::geometry::table_grid;
use dtool_core::table::model::{CellFormat, CellId, CellIdAllocator, FrameFormatMask, TableCell, TableFrameFormat, TableNode};
use dtool_text::table_layout::layout_table;
use dtool_text::{layout_story, story_from_core, FontLibrary};

const TEXT: &str = "ab\ncd\n\nef";

fn document(width: f32) -> (Document, ObjectId) {
    let mut doc = Document::new([width, 200.0], dtool_core::document::Units::Pt, 72.0, 1);
    doc.stories.push(Story { id: StoryId(1), text: TEXT.into(), ..Story::default() });
    let table = TableNode {
        alloc: CellIdAllocator { high_water: 1 },
        col_widths_pt: vec![width],
        row_heights_pt: vec![200.0],
        cells: vec![TableCell { id: CellId(1), row: 0, col: 0, row_span: 1, col_span: 1, merged_into: None, story: StoryId(1), local: CellFormat::default(), mask: Default::default() }],
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
    };
    let id = doc.create_object(Object { kind: ObjectKind::Table(table), size: [width, 200.0], ..Object::default() });
    (doc, id)
}

fn cell_baselines(width: f32) -> (Vec<[usize; 2]>, Vec<f32>) {
    let (doc, id) = document(width);
    let ObjectKind::Table(table) = &doc.get_object(id).unwrap().kind else { unreachable!() };
    let layout = layout_table(&FontLibrary::shared(), &doc, table, &table_grid(table));
    let cell = layout.cells.iter().find(|c| c.id == CellId(1)).expect("cell");
    (cell.ranges.clone(), cell.baselines.clone())
}

fn steps(baselines: &[f32]) -> Vec<f32> {
    baselines.windows(2).map(|pair| pair[1] - pair[0]).collect()
}

#[test]
fn table_cell_paragraph_step_matches_unwrapped_line_height() {
    let core = Story { id: StoryId(1), text: TEXT.into(), ..Story::default() };
    let unwrapped = layout_story(&FontLibrary::shared(), &story_from_core(&core), None);
    let unwrapped_steps = steps(&unwrapped.iter().map(|line| line.baseline).collect::<Vec<_>>());
    for width in [200.0, 60.0] {
        let (ranges, baselines) = cell_baselines(width);
        assert_eq!(ranges, vec![[0, 2], [3, 5], [6, 6], [7, 9]], "width {width}");
        let cell_steps = steps(&baselines);
        assert_eq!(cell_steps.len(), unwrapped_steps.len());
        for (cell, plain) in cell_steps.iter().zip(&unwrapped_steps) {
            assert!((cell - plain).abs() <= 0.01, "width {width}: {cell_steps:?} {unwrapped_steps:?}");
            assert!((cell - 12.0).abs() <= 0.01, "width {width}: {cell_steps:?}");
        }
    }
}
