use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathFlow, PathTextData};
use dtool_core::table::geometry::{CellRect, TableGrid};
use dtool_core::table::model::{CellFormat, CellFormatMask, CellId, CellIdAllocator, CellRotation, FrameFormatMask, TableCell, TableFrameFormat, TableNode, VerticalAlign};
use dtool_core::table::resolve::{CELL_ROTATION, CELL_VERTICAL_ALIGN};
use dtool_text::table_layout::{aligned_baselines, layout_table, rotation_degrees};
use dtool_text::FontLibrary;

fn table_with(cells: Vec<(u64, u32, VerticalAlign, CellRotation)>) -> TableNode {
    TableNode {
        alloc: CellIdAllocator { high_water: cells.len() as u64 },
        col_widths_pt: vec![80.0; cells.len()],
        row_heights_pt: vec![100.0],
        cells: cells
            .into_iter()
            .map(|(id, col, align, rotation)| TableCell {
                id: CellId(id),
                row: 0,
                col,
                row_span: 1,
                col_span: 1,
                merged_into: None,
                story: StoryId(id),
                local: CellFormat { vertical_align: align, rotation, ..CellFormat::default() },
                mask: CellFormatMask(CELL_VERTICAL_ALIGN | CELL_ROTATION),
            })
            .collect(),
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
        text_defaults: None,
    }
}

fn grid(table: &TableNode) -> TableGrid {
    let col_x: Vec<f32> = (0..=table.cells.len()).map(|i| i as f32 * 80.0).collect();
    let rects = table.cells.iter().map(|c| CellRect { id: c.id.0, row: 0, col: c.col, row_span: 1, col_span: 1, rect: [col_x[c.col as usize], 0.0, col_x[c.col as usize + 1], 100.0] }).collect();
    TableGrid { col_x, row_y: vec![0.0, 100.0], rects }
}

fn sized(story: &mut Story) {
    story.ranges.push(Default::default());
    story.ranges[0]["start"] = 0u64.into();
    story.ranges[0]["end"] = (story.text.chars().count() as u64).into();
    story.ranges[0]["size_pt"] = 12.0f64.into();
}

fn doc_with(texts: &[(u64, &str)]) -> Document {
    let mut doc = Document::default();
    for (id, text) in texts {
        let mut story = Story { id: StoryId(*id), text: text.to_string(), ..Story::default() };
        sized(&mut story);
        doc.stories.push(story);
    }
    doc
}

#[test]
fn k12_cell_vertical_align_moves_first_baseline() {
    let lib = FontLibrary::system();
    let table = table_with(vec![(1, 0, VerticalAlign::Top, CellRotation::R0), (2, 1, VerticalAlign::Center, CellRotation::R0), (3, 2, VerticalAlign::Bottom, CellRotation::R0)]);
    let doc = doc_with(&[(1, "Hi"), (2, "Hi"), (3, "Hi")]);
    let layout = layout_table(&lib, &doc, &table, &grid(&table));
    let base = |id: u64| {
        let cell = layout.cells.iter().find(|c| c.id == CellId(id)).unwrap();
        (cell.baselines[0] - cell.content_rect[1], cell.content_rect[3] - cell.content_rect[1])
    };
    let (top, height) = base(1);
    let (middle, _) = base(2);
    let (bottom, _) = base(3);
    assert!(top > 0.0 && top < height * 0.5, "{top}");
    assert!(middle > top && bottom > middle, "{top} {middle} {bottom}");
    let free = bottom - top;
    assert!((middle - top - free * 0.5).abs() < 0.01, "{top} {middle} {bottom}");
    assert!(bottom < height);
    assert!(layout.cells.iter().all(|c| !c.overflow));
}

#[test]
fn k12_align_helper_justify_and_overflow() {
    assert_eq!(aligned_baselines(&[10.0, 24.0], 30.0, 50.0, VerticalAlign::Justify), vec![10.0, 44.0]);
    assert_eq!(aligned_baselines(&[10.0], 30.0, 50.0, VerticalAlign::Justify), vec![10.0]);
    assert_eq!(aligned_baselines(&[10.0, 24.0], 80.0, 50.0, VerticalAlign::Bottom), vec![10.0, 24.0]);
    assert_eq!(aligned_baselines(&[], 0.0, 50.0, VerticalAlign::Center), Vec::<f32>::new());
}

#[test]
fn k12_rotated_cell_lays_out_along_height() {
    let lib = FontLibrary::system();
    let text = "word ".repeat(12);
    let table = table_with(vec![(1, 0, VerticalAlign::Top, CellRotation::R0), (2, 1, VerticalAlign::Top, CellRotation::R90)]);
    let doc = doc_with(&[(1, text.as_str()), (2, text.as_str())]);
    let layout = layout_table(&lib, &doc, &table, &grid(&table));
    let flat = layout.cells.iter().find(|c| c.id == CellId(1)).unwrap();
    let turned = layout.cells.iter().find(|c| c.id == CellId(2)).unwrap();
    assert_eq!(turned.rotation, CellRotation::R90);
    assert_eq!(rotation_degrees(turned.rotation), 90.0);
    assert!(turned.lines.len() <= flat.lines.len(), "{} {}", turned.lines.len(), flat.lines.len());
}

fn path_text_doc(path_size: [f32; 2], text: &str) -> (Document, ObjectId) {
    let mut doc = Document::new([400.0, 400.0], Units::Pt, 72.0, 1);
    doc.objects.push(Object { id: ObjectId(1), name: "path".into(), size: path_size, ..Object::default() });
    doc.objects.push(Object {
        id: ObjectId(2),
        name: "text".into(),
        kind: ObjectKind::PathText(PathTextData { path: ObjectId(1), story: StoryId(9), flows: vec![PathFlow::default()], ..PathTextData::default() }),
        ..Object::default()
    });
    let mut story = Story { id: StoryId(9), text: text.to_string(), ..Story::default() };
    sized(&mut story);
    doc.stories.push(story);
    doc.next_object_id = 3;
    (doc, ObjectId(2))
}

#[test]
fn k12_path_text_overset_reported_through_hook() {
    dtool_text::canvas_text::install();
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda";
    let total = text.chars().count();
    let (doc, id) = path_text_doc([30.0, 4.0], text);
    match dtool_core::text_lines::overset(&doc, id) {
        Some(Some((start, end))) => {
            assert!(start < total, "{start}");
            assert_eq!(end, total);
        }
        other => panic!("expected overset, got {other:?}"),
    }
    let reply = dtool_core::query::query_overset(&doc, &id.0.to_string()).unwrap().unwrap();
    assert_eq!(reply["overset_known"].as_bool(), Some(true));
    assert!(!reply["overset"].is_null());
    assert_eq!(reply["overset"]["end"].as_u64(), Some(total as u64));
    let (roomy, roomy_id) = path_text_doc([3000.0, 4.0], "hi");
    let reply = dtool_core::query::query_overset(&roomy, &roomy_id.0.to_string()).unwrap().unwrap();
    assert_eq!(reply["overset_known"].as_bool(), Some(true));
    assert!(reply["overset"].is_null());
}