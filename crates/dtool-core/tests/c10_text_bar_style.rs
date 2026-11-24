use dtool_core::command::Session;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_protocol::Command;
use serde_json::{json, Value};

fn session_with_story(text: &str, ranges: Vec<Value>, paras: Vec<Value>) -> (Session, u64) {
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    session.document.stories.push(Story { id: StoryId(4), text: text.into(), ranges, specials: Vec::new(), paras });
    (session, 4)
}

fn mixed_story() -> (Session, u64) {
    session_with_story(
        "Hello world",
        vec![
            json!({ "start": 0, "end": 5, "family": "Inter", "style": "Regular", "size_pt": 12.0, "weight": 400, "fill": "black" }),
            json!({ "start": 5, "end": 11, "family": "Pretendard", "style": "Bold", "size_pt": 18.0, "weight": 700, "italic": true, "tracking": 50.0, "fill": "red" }),
        ],
        vec![
            json!({ "start": 0, "end": 6, "align": "Left", "leading": 14.0 }),
            json!({ "start": 6, "end": 11, "align": "Right", "leading": 22.0 }),
        ],
    )
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap()
}

fn query(session: &mut Session, story: u64, start: usize, end: usize) -> Value {
    run(session, "text.style.query", json!({ "story": story, "range": { "start": start, "end": end } }))
}

fn field(session: &Session, story: u64, index: usize, key: &str) -> Value {
    session.document.stories.iter().find(|s| s.id.0 == story).unwrap().ranges[index].get(key).cloned().unwrap_or(Value::Null)
}

#[test]
fn c10_style_query_reports_mixed_and_agreed_runs() {
    let (mut s, id) = mixed_story();
    let all = query(&mut s, id, 0, 11);
    assert_eq!(all["family"], json!("Mixed"));
    assert_eq!(all["style"], json!("Mixed"));
    assert_eq!(all["size_pt"], json!("Mixed"));
    assert_eq!(all["weight"], json!("Mixed"));
    assert_eq!(all["italic"], json!("Mixed"));
    assert_eq!(all["tracking"], json!("Mixed"));
    assert_eq!(all["fill"], json!("Mixed"));
    assert_eq!(all["align"], json!("Mixed"));
    assert_eq!(all["leading"], json!("Mixed"));

    let first = query(&mut s, id, 0, 5);
    assert_eq!(first["family"], json!("Inter"));
    assert_eq!(first["style"], json!("Regular"));
    assert_eq!(first["size_pt"], json!(12.0));
    assert_eq!(first["weight"], json!(400));
    assert_eq!(first["italic"], json!(false));
    assert_eq!(first["tracking"], json!(0.0));
    assert_ne!(first["fill"], json!("Mixed"));
    assert_eq!(first["align"], json!("Left"));
    assert_eq!(first["leading"], json!(14.0));

    let second = query(&mut s, id, 5, 11);
    assert_eq!(second["family"], json!("Pretendard"));
    assert_eq!(second["style"], json!("Bold"));
    assert_eq!(second["size_pt"], json!(18.0));
    assert_eq!(second["weight"], json!(700));
    assert_eq!(second["italic"], json!(true));
    assert_eq!(second["tracking"], json!(50.0));
    assert_eq!(second["align"], json!("Mixed"));
    assert_eq!(second["leading"], json!("Mixed"));
}

#[test]
fn c10_style_query_keeps_mixed_across_three_runs() {
    let (mut s, id) = session_with_story(
        "abcdefghij",
        vec![
            json!({ "start": 0, "end": 3, "family": "Inter", "size_pt": 12.0, "weight": 400 }),
            json!({ "start": 3, "end": 7, "family": "Pretendard", "size_pt": 18.0, "weight": 700 }),
            json!({ "start": 7, "end": 10, "family": "Inter", "size_pt": 12.0, "weight": 400 }),
        ],
        Vec::new(),
    );
    let all = query(&mut s, id, 0, 10);
    assert_eq!((all["family"].clone(), all["size_pt"].clone(), all["weight"].clone()), (json!("Mixed"), json!("Mixed"), json!("Mixed")));
}

#[test]
fn c10_style_query_skips_frame_ranges() {
    let (mut s, id) = session_with_story(
        "Hello",
        vec![
            json!({ "start": 0, "end": 5, "frame": 1, "family": "IgnoreMe", "size_pt": 99.0 }),
            json!({ "start": 0, "end": 5, "family": "Inter", "size_pt": 12.0 }),
        ],
        Vec::new(),
    );
    let reply = query(&mut s, id, 0, 5);
    assert_eq!(reply["family"], json!("Inter"));
    assert_eq!(reply["size_pt"], json!(12.0));
}

#[test]
fn c10_size_change_on_mixed_selection_is_one_undo_and_keeps_families() {
    let (mut s, id) = mixed_story();
    let before = s.document.stories[0].clone();
    let steps = s.undo.applied_steps();
    run(&mut s, "text.style", json!({ "story": id, "range": { "start": 0, "end": 11 }, "patch": { "size_pt": 24.0 } }));
    assert_eq!(s.undo.applied_steps(), steps + 1);
    let all = query(&mut s, id, 0, 11);
    assert_eq!(all["size_pt"], json!(24.0));
    assert_eq!(all["family"], json!("Mixed"));
    assert_eq!(all["italic"], json!("Mixed"));
    assert_eq!(all["tracking"], json!("Mixed"));
    assert_eq!((field(&s, id, 0, "family"), field(&s, id, 1, "family")), (json!("Inter"), json!("Pretendard")));
    assert_eq!((field(&s, id, 0, "weight"), field(&s, id, 1, "weight")), (json!(400), json!(700)));
    assert_eq!(field(&s, id, 1, "italic"), json!(true));
    assert_eq!(field(&s, id, 1, "tracking"), json!(50.0));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document.stories[0], before);
}

#[test]
fn c10_family_bold_italic_tracking_fill_each_one_undo() {
    let (mut s, id) = mixed_story();
    let steps = s.undo.applied_steps();
    run(&mut s, "text.style", json!({ "story": id, "range": { "start": 0, "end": 5 }, "patch": { "family": "Pretendard" } }));
    assert_eq!(query(&mut s, id, 0, 5)["family"], json!("Pretendard"));
    assert_eq!(field(&s, id, 0, "size_pt"), json!(12.0));
    run(&mut s, "text.style", json!({ "story": id, "range": { "start": 0, "end": 5 }, "patch": { "weight": 700 } }));
    assert_eq!(query(&mut s, id, 0, 5)["weight"], json!(700));
    run(&mut s, "text.style", json!({ "story": id, "range": { "start": 0, "end": 5 }, "patch": { "italic": true } }));
    assert_eq!(query(&mut s, id, 0, 5)["italic"], json!(true));
    run(&mut s, "text.style", json!({ "story": id, "range": { "start": 0, "end": 5 }, "patch": { "tracking": 25.0 } }));
    assert_eq!(query(&mut s, id, 0, 5)["tracking"], json!(25.0));
    run(&mut s, "text.style", json!({ "story": id, "range": { "start": 0, "end": 5 }, "patch": { "fill": "blue" } }));
    assert_ne!(query(&mut s, id, 0, 5)["fill"], json!("Mixed"));
    assert_eq!(s.undo.applied_steps(), steps + 5);
    for _ in 0..5 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(field(&s, id, 0, "family"), json!("Inter"));
    assert_eq!(query(&mut s, id, 0, 5)["italic"], json!(false));
    assert_eq!(s.undo.applied_steps(), steps);
}

#[test]
fn c10_leading_and_align_set_via_para_and_show_in_style_query() {
    let (mut s, id) = mixed_story();
    let steps = s.undo.applied_steps();
    run(&mut s, "text.para.set", json!({ "story": id, "range": [0, 11], "patch": { "align": "Center", "leading": 20.0 } }));
    assert_eq!(s.undo.applied_steps(), steps + 1);
    let all = query(&mut s, id, 0, 11);
    assert_eq!(all["align"], json!("Center"));
    assert_eq!(all["leading"], json!(20.0));
    assert_eq!(all["family"], json!("Mixed"));
    run(&mut s, "edit.undo", json!({}));
    let restored = query(&mut s, id, 0, 11);
    assert_eq!(restored["align"], json!("Mixed"));
    assert_eq!(restored["leading"], json!("Mixed"));
    assert_eq!(query(&mut s, id, 0, 5)["align"], json!("Left"));
}

#[test]
fn c10_live_art_text_style_query_and_undo() {
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    let added = run(&mut session, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "hello" }));
    let story = added["story"].as_u64().expect("story");
    let steps = session.undo.applied_steps();
    run(&mut session, "text.style", json!({ "story": story, "range": { "start": 0, "end": 5 }, "patch": { "family": "SourceSans", "size_pt": 30.0, "weight": 700, "italic": true, "tracking": 10.0 } }));
    assert_eq!(session.undo.applied_steps(), steps + 1);
    let reply = query(&mut session, story, 0, 5);
    assert_eq!(reply["family"], json!("SourceSans"));
    assert_eq!(reply["size_pt"], json!(30.0));
    assert_eq!(reply["weight"], json!(700));
    assert_eq!(reply["italic"], json!(true));
    assert_eq!(reply["tracking"], json!(10.0));
    run(&mut session, "edit.undo", json!({}));
    let undone = query(&mut session, story, 0, 5);
    assert_ne!(undone["family"], json!("SourceSans"));
    assert_ne!(undone["size_pt"], json!(30.0));
}
