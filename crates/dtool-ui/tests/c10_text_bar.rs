use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session, Story, StoryId};
use serde_json::{json, Value};
use dtool_ui::context::text_bar::{target_of, TextBarEdit, TextBarState, TextTarget};

fn session() -> Session {
    let mut doc = Document::new([400.0, 300.0], Units::Px, 72.0, 1);
    let mut story = Story { id: StoryId(4), text: "Hello world".into(), ..Default::default() };
    story.ranges = vec![
        json!({ "start": 0, "end": 5, "family": "Inter", "size_pt": 12.0, "weight": 400 }),
        json!({ "start": 5, "end": 11, "family": "Pretendard", "size_pt": 18.0, "weight": 700, "italic": true, "tracking": 50.0 }),
    ];
    doc.stories.push(story);
    let mut s = Session::new(doc);
    s.text_edit = Some(dtool_core::command::TextEditState { story: StoryId(4), object: None, caret: 0, anchor: 11, before: None });
    s
}

fn field(s: &Session, index: usize, key: &str) -> Value {
    s.document.stories[0].ranges[index].get(key).cloned().unwrap_or(Value::Null)
}

#[test]
fn c10_text_bar_reads_mixed_selection() {
    let mut s = session();
    assert_eq!(target_of(&s), Some(TextTarget { story: 4, start: 0, end: 11 }));
    let state = TextBarState::load(&mut s).unwrap();
    assert_eq!((state.family.clone(), state.size_pt, state.weight, state.italic, state.tracking), (None, None, None, None, None));
    s.text_edit.as_mut().unwrap().caret = 2;
    s.text_edit.as_mut().unwrap().anchor = 4;
    let state = TextBarState::load(&mut s).unwrap();
    assert_eq!(state.family.as_deref(), Some("Inter"));
    assert_eq!((state.size_pt, state.bold(), state.italic, state.tracking), (Some(12.0), Some(false), Some(false), Some(0.0)));
}

#[test]
fn c10_size_change_on_mixed_selection_is_one_undo_and_keeps_families() {
    let mut s = session();
    let before = s.document.stories[0].clone();
    let mut state = TextBarState::load(&mut s).unwrap();
    let steps = s.undo.applied_steps();
    let command = state.command(&TextBarEdit::Size(24.0)).unwrap();
    assert_eq!(command.name, "text.style");
    assert_eq!(command.params["patch"], json!({ "size_pt": 24.0 }));
    assert!(state.apply(&mut s, &TextBarEdit::Size(24.0)).unwrap());
    assert_eq!(s.undo.applied_steps(), steps + 1);
    assert_eq!(state.size_pt, Some(24.0));
    assert_eq!(state.family, None);
    assert_eq!((field(&s, 0, "family"), field(&s, 1, "family")), (json!("Inter"), json!("Pretendard")));
    assert_eq!((field(&s, 0, "weight"), field(&s, 1, "weight")), (json!(400), json!(700)));
    assert_eq!(field(&s, 1, "italic"), json!(true));
    assert_eq!(field(&s, 1, "tracking"), json!(50.0));
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.document.stories[0], before);
}

#[test]
fn c10_family_and_bold_change_one_undo_each() {
    let mut s = session();
    s.text_edit.as_mut().unwrap().caret = 5;
    s.text_edit.as_mut().unwrap().anchor = 0;
    let mut state = TextBarState::load(&mut s).unwrap();
    assert_eq!(state.target, Some(TextTarget { story: 4, start: 0, end: 5 }));
    let steps = s.undo.applied_steps();
    assert!(state.apply(&mut s, &TextBarEdit::Family("Pretendard".into())).unwrap());
    assert_eq!(s.undo.applied_steps(), steps + 1);
    assert_eq!(state.family.as_deref(), Some("Pretendard"));
    assert_eq!(field(&s, 0, "size_pt"), json!(12.0));
    assert!(state.apply(&mut s, &TextBarEdit::Bold(true)).unwrap());
    assert_eq!(state.bold(), Some(true));
    assert_eq!(s.undo.applied_steps(), steps + 2);
    assert!(!state.apply(&mut s, &TextBarEdit::Size(-3.0)).unwrap());
    assert!(!state.apply(&mut s, &TextBarEdit::Family("  ".into())).unwrap());
    assert_eq!(s.undo.applied_steps(), steps + 2);
    s.run(&Command::new("edit.undo")).unwrap();
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(field(&s, 0, "family"), json!("Inter"));
}

#[test]
fn c10_text_bar_without_text_target_is_inert() {
    let mut s = session();
    s.text_edit = None;
    let state = TextBarState::load(&mut s).unwrap();
    assert_eq!(state, TextBarState::default());
    assert!(state.command(&TextBarEdit::Size(10.0)).is_none());
}