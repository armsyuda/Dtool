use dtool_ui::panels::character::{self, read_selection, selection_at, Field, BATCH_COMMAND, STYLE_COMMAND};
use dtool_ui::font_combo;
use dtool_core::command::{Session, TextEditState};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectKind};
use dtool_protocol::Command;
use serde_json::{json, Value};

const ID: u64 = 70;

fn session(text: &str, ranges: Vec<Value>) -> Session {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let story = StoryId(ID);
    session.document.stories.push(Story { id: story, text: text.to_string(), ranges, specials: Vec::new(), paras: Vec::new() });
    let object = session.document.create_object(Object { kind: ObjectKind::FrameText { story }, size: [200.0, 100.0], ..Object::default() });
    session.selection.ids = vec![object];
    session
}

fn ranges(session: &Session) -> Vec<Value> {
    session.document.stories.iter().find(|s| s.id.0 == ID).unwrap().ranges.clone()
}

fn undo(session: &mut Session) {
    session.run(&Command::new("edit.undo")).unwrap();
}

fn key_at(session: &Session, at: usize, key: &str) -> Value {
    let found = ranges(session);
    found.iter().find(|r| r["start"].as_u64().unwrap() as usize <= at && r["end"].as_u64().unwrap() as usize > at).and_then(|r| r.get(key).cloned()).unwrap_or(Value::Null)
}

#[test]
fn tracking_on_range_emits_text_style_and_is_one_undo() {
    let mut s = session("abcdef", vec![json!({"start": 0, "end": 6, "size_pt": 12.0})]);
    let original = ranges(&s);
    let selection = selection_at(&s, ID, 1, 4).unwrap();
    let command = selection.set("tracking", json!(50.0)).unwrap();
    assert_eq!(command.name, STYLE_COMMAND);
    assert_eq!(command.params["patch"], json!({"tracking": 50.0}));
    assert_eq!(command.params["range"], json!({"start": 1, "end": 4}));
    character::apply(&mut s, &command).unwrap();
    assert_eq!(key_at(&s, 0, "tracking"), Value::Null);
    assert_eq!(key_at(&s, 2, "tracking"), json!(50.0));
    assert_eq!(key_at(&s, 2, "size_pt"), json!(12.0));
    assert_eq!(selection_at(&s, ID, 1, 4).unwrap().number("tracking", 0.0), Some(50.0));
    undo(&mut s);
    assert_eq!(ranges(&s), original);
}

#[test]
fn mixed_selection_leaves_unprovided_attributes_unchanged() {
    let underline = json!({"on": false, "colour": "#ff0000", "weight": 2.0});
    let mut s = session("abcdef", vec![
        json!({"start": 0, "end": 3, "family": "A", "tracking": 10.0, "underline": underline}),
        json!({"start": 3, "end": 6, "family": "B", "size_pt": 20.0, "tracking": -5.0}),
    ]);
    let original = ranges(&s);
    let selection = selection_at(&s, ID, 0, 6).unwrap();
    assert_eq!(selection.field("family"), Field::Mixed);
    assert_eq!(selection.number("tracking", 0.0), None);
    assert_eq!(selection.text_value("family"), None);
    assert_eq!(selection.decoration_on("underline"), Some(false));

    character::apply(&mut s, &selection.set("tracking", json!(0.0)).unwrap()).unwrap();
    assert_eq!((key_at(&s, 1, "family"), key_at(&s, 4, "family")), (json!("A"), json!("B")));
    assert_eq!(key_at(&s, 4, "size_pt"), json!(20.0));
    assert_eq!(key_at(&s, 1, "underline"), underline);
    assert_eq!((key_at(&s, 1, "tracking"), key_at(&s, 4, "tracking")), (json!(0.0), json!(0.0)));
    undo(&mut s);
    assert_eq!(ranges(&s), original);

    let command = selection.decoration("underline", true).unwrap();
    assert_eq!(command.name, BATCH_COMMAND);
    character::apply(&mut s, &command).unwrap();
    assert_eq!(key_at(&s, 1, "underline"), json!({"on": true, "colour": "#ff0000", "weight": 2.0}));
    assert_eq!(key_at(&s, 4, "underline"), json!({"on": true}));
    assert_eq!(key_at(&s, 4, "size_pt"), json!(20.0));
    assert_eq!(selection_at(&s, ID, 0, 6).unwrap().decoration_on("underline"), Some(true));
    undo(&mut s);
    assert_eq!(ranges(&s), original);

    let command = selection.nudge("tracking", dtool_tools::dtool_text::char_style::tracking_tighten).unwrap();
    character::apply(&mut s, &command).unwrap();
    assert_eq!((key_at(&s, 1, "tracking"), key_at(&s, 4, "tracking")), (json!(-10.0), json!(-25.0)));
    assert_eq!((key_at(&s, 1, "family"), key_at(&s, 4, "family")), (json!("A"), json!("B")));
    undo(&mut s);
    assert_eq!(ranges(&s), original);
}

#[test]
fn caret_selection_reads_live_range_and_position_toggles() {
    let mut s = session("abcdef", vec![json!({"start": 2, "end": 5, "position": "Superscript"})]);
    s.text_edit = Some(TextEditState { story: StoryId(ID), object: None, caret: 5, anchor: 2, before: None });
    let selection = read_selection(&s).unwrap();
    assert_eq!((selection.start, selection.end), (2, 5));
    assert_eq!(selection.field("position"), Field::Same(json!("Superscript")));
    let command = selection.toggle_position("Superscript").unwrap();
    assert_eq!(command.params["patch"], json!({"position": "Normal"}));
    character::apply(&mut s, &command).unwrap();
    assert_eq!(key_at(&s, 3, "position"), json!("Normal"));
    let sub = read_selection(&s).unwrap().toggle_position("Subscript").unwrap();
    character::apply(&mut s, &sub).unwrap();
    assert_eq!(key_at(&s, 3, "position"), json!("Subscript"));

    s.text_edit = Some(TextEditState { story: StoryId(ID), object: None, caret: 3, anchor: 3, before: None });
    let collapsed = read_selection(&s).unwrap();
    assert_eq!(collapsed.field("position"), Field::Same(json!("Subscript")));
    assert!(collapsed.set("tracking", json!(10.0)).is_none());
    assert!(collapsed.nudge("tracking", dtool_tools::dtool_text::char_style::tracking_loosen).is_none());
}

#[test]
fn whole_story_when_not_editing_and_noop_nudge_emits_nothing() {
    let s = session("abc", Vec::new());
    let selection = read_selection(&s).unwrap();
    assert_eq!((selection.start, selection.end), (0, 3));
    assert_eq!(selection.field("tracking"), Field::Same(Value::Null));
    assert!(selection.per_segment("tracking", |current| current.cloned()).is_none());
}
