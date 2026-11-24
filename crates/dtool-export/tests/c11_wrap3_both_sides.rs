use dtool_core::ObjectId;
use dtool_core::document::{Document, Story as CoreStory, Units};
use dtool_core::object::Object;
use dtool_core::{FrameProps, StoryId};
use dtool_protocol::Command;
use dtool_text::frame::{layout_chain_wrapped, LineSpans};
use dtool_text::{story_from_core, FontLibrary, Line};
use serde_json::json;
use dtool_core::text_wrap::{line_spans, TextWrap, WrapMode, WrapSides, WrapTable, SET};
use dtool_core::commands::text_wrap_cmd::{edit, frame_wrappers};

const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";

fn doc() -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::new([600.0, 600.0], Units::Px, 72.0, 1);
    let frame = doc.create_object(Object { pos: [100.0, 100.0], size: [220.0, 300.0], ..Object::default() });
    let neighbour = doc.create_object(Object { pos: [175.0, 100.0], size: [70.0, 60.0], ..Object::default() });
    (doc, frame, neighbour)
}

fn story() -> dtool_text::Story {
    story_from_core(&CoreStory { id: StoryId(1), text: TEXT.to_string(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() })
}

fn left_x(line: &Line) -> f32 {
    line.glyphs.first().map(|g| g.offset[0]).unwrap_or(0.0)
}

fn right_x(line: &Line) -> f32 {
    line.glyphs.iter().map(|g| g.offset[0] + g.advance).fold(0.0, f32::max)
}

fn shape(lines: &[Line]) -> Vec<(usize, usize, i32, i32)> {
    lines.iter().map(|l| (l.range.start, l.range.end, left_x(l).round() as i32, l.baseline.round() as i32)).collect()
}

fn layout(table: &WrapTable, doc: &Document, frame: ObjectId) -> Vec<Line> {
    let wrappers = frame_wrappers(table, doc, frame);
    let lib = FontLibrary::shared();
    let props = FrameProps::from_rect([220.0, 300.0]);
    let spans = |span: [f32; 2], band: [f32; 2]| line_spans(span, band, &wrappers);
    let hooks: Vec<Option<LineSpans>> = vec![Some(&spans)];
    layout_chain_wrapped(&lib, &story(), &[props], &hooks).per_frame.remove(0)
}

fn set(doc: &Document, table: &mut WrapTable, sides: &str) -> Option<WrapTable> {
    edit(doc, table, &Command::with_params(SET, json!({"id": doc.objects[1].id.0, "mode": "square", "sides": sides, "distance": 5.0}))).unwrap()
}

#[test]
fn both_sides_flow_text_left_and_right_on_the_same_band() {
    let (doc, frame, _) = doc();
    let mut table = WrapTable::default();
    set(&doc, &mut table, "both");
    let lines = layout(&table, &doc, frame);
    let beside: Vec<&Line> = lines.iter().filter(|l| l.baseline < 60.0).collect();
    let lefts: Vec<&&Line> = beside.iter().filter(|l| left_x(l) < 20.0).collect();
    let rights: Vec<&&Line> = beside.iter().filter(|l| left_x(l) >= 150.0 - 0.5).collect();
    assert!(!lefts.is_empty() && !rights.is_empty());
    assert_eq!(lefts.len() + rights.len(), beside.len());
    assert!(lefts.iter().all(|l| right_x(l) <= 70.0 + 0.5));
    for left in &lefts {
        assert!(rights.iter().any(|r| (r.baseline - left.baseline).abs() < 0.01 && r.range.start >= left.range.end));
    }
    for pair in lines.windows(2) {
        assert!(pair[1].range.start >= pair[0].range.end);
    }
    assert!(lines.iter().filter(|l| l.baseline > 85.0).all(|l| left_x(l) < 20.0));
}

#[test]
fn left_side_still_uses_only_the_left_span() {
    let (doc, frame, _) = doc();
    let mut table = WrapTable::default();
    set(&doc, &mut table, "left");
    let lines = layout(&table, &doc, frame);
    let beside: Vec<&Line> = lines.iter().filter(|l| l.baseline < 60.0).collect();
    assert!(!beside.is_empty());
    assert!(beside.iter().all(|l| left_x(l) < 20.0 && right_x(l) <= 70.0 + 0.5));
    let mut baselines: Vec<i32> = beside.iter().map(|l| l.baseline.round() as i32).collect();
    let count = baselines.len();
    baselines.dedup();
    assert_eq!(baselines.len(), count);
}

#[test]
fn non_both_sides_with_two_obstacles_keep_one_span_per_band() {
    let wrap = |sides| TextWrap { mode: WrapMode::Square, sides, distance: [0.0; 4] };
    let a = (wrap(WrapSides::Left), [150.0, 0.0, 20.0, 50.0]);
    let b = (wrap(WrapSides::Both), [40.0, 0.0, 20.0, 50.0]);
    assert_eq!(line_spans([0.0, 200.0], [10.0, 20.0], &[a, b]), vec![[60.0, 150.0]]);
    let both = (wrap(WrapSides::Both), [80.0, 0.0, 20.0, 50.0]);
    assert_eq!(line_spans([0.0, 200.0], [10.0, 20.0], &[both]), vec![[0.0, 80.0], [100.0, 200.0]]);
}

#[test]
fn undo_of_wrap_set_restores_previous_wrap_and_layout() {
    let (doc, frame, neighbour) = doc();
    let mut table = WrapTable::default();
    set(&doc, &mut table, "left");
    let left_table = table.clone();
    let left_layout = shape(&layout(&table, &doc, frame));
    let before = set(&doc, &mut table, "both").expect("one change");
    assert_ne!(shape(&layout(&table, &doc, frame)), left_layout);
    table = before;
    assert_eq!(table, left_table);
    assert_eq!(table.get(neighbour).unwrap().wrap.unwrap().sides, WrapSides::Left);
    assert_eq!(shape(&layout(&table, &doc, frame)), left_layout);
    assert!(set(&doc, &mut table, "left").is_none());
}