use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::table::geometry::{CellRect, TableGrid};
use dtool_core::table::model::{CellFormat, CellFormatMask, CellId, CellIdAllocator, FrameFormatMask, TableCell, TableFrameFormat, TableNode};
use dtool_core::{Command, ObjectId, Session};
use dtool_text::metrics::{family_metrics, MetricsSource};
use dtool_text::table_layout::layout_table;
use dtool_text::FontLibrary;

fn actual_family(lib: &FontLibrary) -> Option<String> {
    let actual: Vec<&str> = lib
        .families()
        .into_iter()
        .filter(|family| family_metrics(lib, family, "", 12.0).metrics.source == MetricsSource::Actual)
        .collect();
    actual
        .iter()
        .find(|family| (family_metrics(lib, family, "", 12.0).metrics.ascent_pt - 9.6).abs() > 0.05)
        .or(actual.first())
        .map(|family| family.to_string())
}

fn cell(id: u64, col: u32, story: u64, merged_into: Option<u64>) -> TableCell {
    TableCell { id: CellId(id), row: 0, col, row_span: 1, col_span: 1, merged_into: merged_into.map(CellId), story: StoryId(story), local: CellFormat::default(), mask: CellFormatMask::default() }
}

fn grid(table: &TableNode, col_x: Vec<f32>, height: f32) -> TableGrid {
    let mut grid = TableGrid { col_x, row_y: vec![0.0, height], rects: Vec::new() };
    for c in table.cells.iter().filter(|c| c.merged_into.is_none()) {
        let rect = [grid.col_x[c.col as usize], 0.0, grid.col_x[(c.col + c.col_span) as usize], height];
        grid.rects.push(CellRect { id: c.id.0, row: c.row, col: c.col, row_span: c.row_span, col_span: c.col_span, rect });
    }
    grid
}

#[test]
fn t8_first_baseline_covered_and_overflow_use_real_metrics() {
    let lib = FontLibrary::system();
    let family = actual_family(&lib);
    let mut doc = Document::new([800.0, 600.0], Units::Px, 96.0, 1);
    let styled = |id: u64, text: String| {
        let mut story = Story { id: StoryId(id), text, ..Story::default() };
        story.ranges.push(Default::default());
        story.ranges[0]["start"] = 0u64.into();
        story.ranges[0]["end"] = (story.text.chars().count() as u64).into();
        story.ranges[0]["size_pt"] = 12.0f64.into();
        if let Some(family) = &family {
            story.ranges[0]["family"] = family.as_str().into();
        }
        story
    };
    doc.stories.push(styled(1, "Hello".to_string()));
    doc.stories.push(styled(2, "word ".repeat(60)));
    doc.stories.push(Story { id: StoryId(3), text: "covered".into(), ..Story::default() });
    let table = TableNode {
        alloc: CellIdAllocator { high_water: 3 },
        col_widths_pt: vec![80.0, 20.0],
        row_heights_pt: vec![40.0],
        cells: vec![cell(1, 0, 1, None), cell(2, 1, 2, None), cell(3, 1, 3, Some(2))],
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
    };
    let layout = layout_table(&lib, &doc, &table, &grid(&table, vec![0.0, 80.0, 100.0], 40.0));
    assert_eq!(layout.cells.len(), 2);
    assert!(!layout.cells.iter().any(|c| c.id == CellId(3)));
    let first = layout.cells.iter().find(|c| c.id == CellId(1)).unwrap();
    assert_eq!(first.lines.len(), 1);
    assert_eq!(first.baselines.len(), 1);
    let size_px = 12.0 * 96.0 / 72.0;
    let expected = match &family {
        Some(family) => family_metrics(&lib, family, "", size_px).metrics.ascent_pt,
        None => size_px * 0.8,
    };
    let baseline = first.baselines[0] - first.content_rect[1];
    eprintln!("t8 family={:?} first_baseline={baseline} ascent={expected}", family);
    assert!((baseline - expected).abs() < 0.01);
    assert!((first.lines[0].ascent - expected).abs() < 0.01);
    assert!(!first.overflow);
    let long = layout.cells.iter().find(|c| c.id == CellId(2)).unwrap();
    assert!(long.lines.len() > 1);
    assert!(long.baselines.windows(2).all(|pair| pair[1] > pair[0]));
    assert!(long.overflow);
}

#[test]
fn t4_overset_hook_reports_frame_range_and_art_null() {
    dtool_text::canvas_text::install();
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";
    let mut add = Command::new("text.frame.add");
    add.params["rect"] = vec![0.0f64, 0.0, 100.0, 30.0].into();
    add.params["text"] = text.into();
    let small = ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
    let mut add_big = Command::new("text.frame.add");
    add_big.params["rect"] = vec![0.0f64, 100.0, 300.0, 200.0].into();
    add_big.params["text"] = "short".into();
    let big = ObjectId(session.run(&add_big).unwrap()["id"].as_u64().unwrap());
    let mut art = Command::new("text.art.add");
    art.params["pos"] = vec![10.0f64, 350.0].into();
    art.params["text"] = "art".into();
    let art = ObjectId(session.run(&art).unwrap()["id"].as_u64().unwrap());
    let total = text.chars().count();
    match dtool_core::text_lines::overset(&session.document, small) {
        Some(Some((start, end))) => {
            assert!(start > 0 && start < total);
            assert_eq!(end, total);
        }
        other => panic!("expected overset, got {other:?}"),
    }
    assert_eq!(dtool_core::text_lines::overset(&session.document, big), Some(None));
    assert_eq!(dtool_core::text_lines::overset(&session.document, art), Some(None));
    let reply = dtool_core::query::query_overset(&session.document, &small.0.to_string()).unwrap().unwrap();
    assert_eq!(reply["overset_known"].as_bool(), Some(true));
    assert_eq!(reply["overset"]["end"].as_u64(), Some(total as u64));
    let reply = dtool_core::query::query_overset(&session.document, &big.0.to_string()).unwrap().unwrap();
    assert!(reply["overset"].is_null());
    assert_eq!(reply["overset_known"].as_bool(), Some(true));
}
