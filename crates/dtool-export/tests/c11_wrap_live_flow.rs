use dtool_core::document::{Document, Units};
use dtool_core::{Command, ObjectId, Session};
use dtool_text::frame::layout_doc_flow;
use dtool_text::FontLibrary;
use serde_json::{json, Value};

const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";

struct Live {
    session: Session,
    frame: ObjectId,
    neighbour: ObjectId,
}

fn live() -> Live {
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [100, 100, 220, 300], "text": TEXT}))).unwrap()["id"].as_u64().unwrap();
    let neighbour = session.document.next_object_id;
    session.run(&Command::with_params("object.create", json!({"name": "n", "pos": [100.0, 100.0], "size": [90.0, 60.0]}))).unwrap();
    Live { session, frame: ObjectId(frame), neighbour: ObjectId(neighbour) }
}

fn story(live: &Live) -> dtool_core::StoryId {
    dtool_core::frame::frame_story(&live.session.document, live.frame).unwrap()
}

fn line_starts(live: &Live) -> Vec<(f32, f32)> {
    let lib = FontLibrary::system();
    let flow = layout_doc_flow(&lib, &live.session.document, story(live)).expect("flow");
    flow.layout.per_frame[0]
        .iter()
        .filter_map(|line| line.glyphs.first().map(|glyph| (line.baseline, glyph.offset[0])))
        .collect()
}

fn run(live: &mut Live, name: &str, params: Value) -> Value {
    live.session.run(&Command::with_params(name, params)).unwrap()
}

#[test]
fn live_wrap_set_pushes_lines_beside_the_neighbour_and_undo_restores() {
    let mut live = live();
    let plain = line_starts(&live);
    assert!(plain.len() > 3);
    assert!(plain[0].1 < 20.0, "{plain:?}");
    let version = live.session.document.version;
    let neighbour = live.neighbour.0;
    run(&mut live, "object.text_wrap.set", json!({"id": neighbour, "mode": "square", "distance": 4.0}));
    assert!(live.session.document.version > version);
    let wrapped = line_starts(&live);
    assert!(wrapped[0].1 >= 90.0, "{wrapped:?}");
    let below = wrapped.iter().find(|(baseline, _)| *baseline > 90.0).expect("line below the neighbour");
    assert!(below.1 < 20.0, "{wrapped:?}");
    run(&mut live, "edit.undo", json!({}));
    assert_eq!(line_starts(&live), plain);
}

#[test]
fn live_frame_ignoring_wraps_keeps_full_width_lines() {
    let mut live = live();
    let plain = line_starts(&live);
    let (neighbour, frame) = (live.neighbour.0, live.frame.0);
    run(&mut live, "object.text_wrap.set", json!({"id": neighbour, "mode": "square"}));
    assert_ne!(line_starts(&live), plain);
    run(&mut live, "object.text_wrap.ignore", json!({"id": frame, "on": true}));
    assert_eq!(line_starts(&live), plain);
    run(&mut live, "object.text_wrap.ignore", json!({"id": frame, "on": false}));
    run(&mut live, "object.text_wrap.clear", json!({"id": neighbour}));
    assert_eq!(line_starts(&live), plain);
}

#[test]
fn live_wrap_query_reports_without_touching_the_document() {
    let mut live = live();
    let neighbour = live.neighbour.0;
    run(&mut live, "object.text_wrap.set", json!({"id": neighbour, "mode": "square", "distance": 2.0}));
    let version = live.session.document.version;
    let query = run(&mut live, "object.text_wrap.query", json!({"id": neighbour}));
    assert_eq!(query["wrap"]["mode"], "square");
    assert_eq!(live.session.document.version, version);
}