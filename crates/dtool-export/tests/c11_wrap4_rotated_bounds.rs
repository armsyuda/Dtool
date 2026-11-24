use dtool_core::ObjectId;
use dtool_core::document::{Document, Story as CoreStory, Units};
use dtool_core::object::Object;
use dtool_core::{FrameProps, StoryId};
use dtool_protocol::Command;
use dtool_text::frame::{layout_chain_wrapped, LineSpans};
use dtool_text::{story_from_core, FontLibrary, Line};
use serde_json::json;
use dtool_core::text_wrap::{line_spans, WrapTable, CLEAR, SET};
use dtool_core::commands::text_wrap_cmd::{edit, frame_wrappers};

const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";

fn doc(rotation: f32) -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::new([600.0, 600.0], Units::Px, 72.0, 1);
    let frame = doc.create_object(Object { pos: [100.0, 100.0], size: [220.0, 300.0], ..Object::default() });
    let neighbour = doc.create_object(Object { pos: [100.0, 100.0], size: [120.0, 40.0], rotation_deg: rotation, ..Object::default() });
    (doc, frame, neighbour)
}

fn story() -> dtool_text::Story {
    story_from_core(&CoreStory { id: StoryId(1), text: TEXT.to_string(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() })
}

fn left_x(line: &Line) -> f32 {
    line.glyphs.first().map(|g| g.offset[0]).unwrap_or(0.0)
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

fn set_right(doc: &Document, table: &mut WrapTable, neighbour: ObjectId) -> Option<WrapTable> {
    edit(doc, table, &Command::with_params(SET, json!({"id": neighbour.0, "mode": "square", "sides": "right", "distance": 5.0}))).unwrap()
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

#[test]
fn rotated_neighbour_excludes_its_rotated_world_box() {
    let (doc, frame, neighbour) = doc(90.0);
    let mut table = WrapTable::default();
    set_right(&doc, &mut table, neighbour);
    let wrappers = frame_wrappers(&table, &doc, frame);
    assert_eq!(wrappers.len(), 1);
    assert!(near(wrappers[0].1, [40.0, -40.0, 40.0, 120.0]), "{:?}", wrappers[0].1);
    let lines = layout(&table, &doc, frame);
    let top: Vec<&Line> = lines.iter().filter(|l| l.baseline < 40.0).collect();
    assert!(!top.is_empty());
    assert!(top.iter().all(|l| left_x(l) >= 85.0 - 0.5 && left_x(l) < 120.0));
    let only_rotated: Vec<&Line> = lines.iter().filter(|l| l.baseline > 55.0 && l.baseline < 80.0).collect();
    assert!(!only_rotated.is_empty());
    assert!(only_rotated.iter().all(|l| left_x(l) >= 85.0 - 0.5));
    assert!(lines.iter().filter(|l| l.baseline > 105.0).all(|l| left_x(l) < 20.0));
}

#[test]
fn unrotated_neighbour_keeps_the_axis_aligned_box() {
    let (doc, frame, neighbour) = doc(0.0);
    let mut table = WrapTable::default();
    set_right(&doc, &mut table, neighbour);
    let wrappers = frame_wrappers(&table, &doc, frame);
    assert_eq!(wrappers[0].1, [0.0, 0.0, 120.0, 40.0]);
    let lines = layout(&table, &doc, frame);
    assert!(lines.iter().filter(|l| l.baseline < 40.0).all(|l| left_x(l) >= 125.0 - 0.5));
    assert!(lines.iter().filter(|l| l.baseline > 60.0).all(|l| left_x(l) < 20.0));
    let mut flipped = doc.clone();
    flipped.objects[1].size = [-120.0, 40.0];
    assert_eq!(frame_wrappers(&table, &flipped, frame)[0].1, [0.0, 0.0, 120.0, 40.0]);
}

#[test]
fn through_and_none_still_do_not_wrap_a_rotated_neighbour() {
    let (doc, frame, neighbour) = doc(90.0);
    let mut table = WrapTable::default();
    edit(&doc, &mut table, &Command::with_params(SET, json!({"id": neighbour.0, "mode": "through"}))).unwrap();
    let plain = shape(&layout(&WrapTable::default(), &doc, frame));
    assert_eq!(shape(&layout(&table, &doc, frame)), plain);
    edit(&doc, &mut table, &Command::with_params(SET, json!({"id": neighbour.0, "mode": "none"}))).unwrap();
    assert_eq!(shape(&layout(&table, &doc, frame)), plain);
}

#[test]
fn wrap_set_and_undo_restore_the_rotated_layout() {
    let (doc, frame, neighbour) = doc(90.0);
    let mut table = WrapTable::default();
    let plain = shape(&layout(&table, &doc, frame));
    let before = set_right(&doc, &mut table, neighbour).expect("one change");
    let wrapped = shape(&layout(&table, &doc, frame));
    assert_ne!(wrapped, plain);
    let set = table.clone();
    table = before;
    assert_eq!(table, WrapTable::default());
    assert_eq!(shape(&layout(&table, &doc, frame)), plain);
    table = set;
    assert_eq!(shape(&layout(&table, &doc, frame)), wrapped);
    let cleared = edit(&doc, &mut table, &Command::with_params(CLEAR, json!({"id": neighbour.0}))).unwrap();
    assert!(cleared.is_some());
    assert_eq!(shape(&layout(&table, &doc, frame)), plain);
}