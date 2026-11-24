use dtool_core::ObjectId;
use dtool_core::command::Session;
use dtool_core::document::{Document, Story as CoreStory, Units};
use dtool_core::object::Object;
use dtool_core::{FrameProps, StoryId};
use dtool_protocol::Command;
use dtool_text::frame::{layout_chain_wrapped, LineSpans};
use dtool_text::{layout_chain, story_from_core, FontLibrary, Line};
use serde_json::json;
use dtool_core::text_wrap::{line_spans, WrapTable, CLEAR, IGNORE, QUERY, SET};
use dtool_core::commands::text_wrap_cmd::{edit, frame_wrappers, run_with};

const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";

fn doc() -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::new([600.0, 600.0], Units::Px, 72.0, 1);
    let frame = doc.create_object(Object { pos: [100.0, 100.0], size: [220.0, 300.0], ..Object::default() });
    let neighbour = doc.create_object(Object { pos: [100.0, 100.0], size: [90.0, 60.0], ..Object::default() });
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

#[test]
fn square_wrap_on_neighbour_insets_the_frame_on_that_side() {
    let (doc, frame, neighbour) = doc();
    let mut table = WrapTable::default();
    edit(&doc, &mut table, &Command::with_params(SET, json!({"id": neighbour.0, "mode": "square", "sides": "right", "distance": 5.0}))).unwrap().unwrap();
    let wrappers = frame_wrappers(&table, &doc, frame);
    assert_eq!(wrappers.len(), 1);
    assert_eq!(wrappers[0].1, [0.0, 0.0, 90.0, 60.0]);
    let lib = FontLibrary::shared();
    let props = FrameProps::from_rect([220.0, 300.0]);
    let spans = |span: [f32; 2], band: [f32; 2]| line_spans(span, band, &wrappers);
    let hooks: Vec<Option<LineSpans>> = vec![Some(&spans)];
    let wrapped = layout_chain_wrapped(&lib, &story(), &[props.clone()], &hooks);
    let plain = layout_chain(&lib, &story(), &[props]);
    let lines = &wrapped.per_frame[0];
    assert!(lines.len() > 4);
    let (beside, below): (Vec<&Line>, Vec<&Line>) = lines.iter().partition(|l| l.baseline < 60.0);
    assert!(!beside.is_empty() && !below.is_empty());
    assert!(beside.iter().all(|l| left_x(l) >= 95.0 - 0.5));
    assert!(below.iter().filter(|l| l.baseline > 85.0).all(|l| left_x(l) < 20.0));
    assert!(plain.per_frame[0].iter().all(|l| left_x(l) < 20.0));
    assert_ne!(shape(lines), shape(&plain.per_frame[0]));
}

#[test]
fn ignore_wrap_frame_layout_is_unchanged() {
    let (doc, frame, neighbour) = doc();
    let mut table = WrapTable::default();
    edit(&doc, &mut table, &Command::with_params(SET, json!({"id": neighbour.0, "mode": "square"}))).unwrap();
    edit(&doc, &mut table, &Command::with_params(IGNORE, json!({"id": frame.0, "on": true}))).unwrap();
    assert!(frame_wrappers(&table, &doc, frame).is_empty());
    let lib = FontLibrary::shared();
    let props = FrameProps::from_rect([220.0, 300.0]);
    let hooks: Vec<Option<LineSpans>> = vec![None];
    let unchanged = layout_chain_wrapped(&lib, &story(), &[props.clone()], &hooks);
    let plain = layout_chain(&lib, &story(), &[props]);
    assert_eq!(shape(&unchanged.per_frame[0]), shape(&plain.per_frame[0]));
    assert_eq!(unchanged.overset, plain.overset);
}

#[test]
fn set_and_clear_are_one_step_each_and_unknown_object_is_not_found() {
    let (doc, _frame, neighbour) = doc();
    let mut session = Session::new(doc);
    let mut table = WrapTable::default();
    let empty = table.clone();
    let reply = run_with(&mut session, &mut table, &Command::with_params(SET, json!({"id": neighbour.0, "mode": "square", "distance": 4.0}))).unwrap().unwrap();
    assert_eq!(reply["changed"], true);
    let set = table.clone();
    let reply = run_with(&mut session, &mut table, &Command::with_params(SET, json!({"id": neighbour.0, "mode": "square", "distance": 4.0}))).unwrap().unwrap();
    assert_eq!(reply["changed"], false);
    let before = edit(&session.document, &mut table, &Command::with_params(CLEAR, json!({"id": neighbour.0}))).unwrap();
    assert_eq!(before, Some(set.clone()));
    assert_eq!(table, empty);
    let query = run_with(&mut session, &mut table, &Command::with_params(QUERY, json!({"id": neighbour.0}))).unwrap().unwrap();
    assert!(query["wrap"].is_null());
    let error = run_with(&mut session, &mut table, &Command::with_params(SET, json!({"id": 999, "mode": "square"}))).unwrap().unwrap_err();
    assert_eq!(error.code, "NOT_FOUND");
    assert_eq!(table, empty);
    assert!(run_with(&mut session, &mut table, &Command::new("object.other")).is_none());
}