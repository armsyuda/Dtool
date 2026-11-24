use dtool_core::command::TextEditState;
use dtool_core::document::{Document, Units};
use dtool_core::{ObjectId, Session};
use serde_json::json;

fn session_with_text(text: &str) -> (Session, ObjectId, ObjectId) {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    s.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": text})).unwrap();
    let text_id = s.document.objects.last().unwrap().id;
    let rect = s.document.next_object_id;
    s.run_named("object.create", json!({"name": "r", "pos": [50.0, 50.0], "size": [5.0, 5.0]})).unwrap();
    (s, text_id, ObjectId(rect))
}

#[test]
fn active_story_follows_text_edit_then_selection() {
    let (mut s, text_id, rect) = session_with_text("one\ntwo");
    let story = s.document.get_object(text_id).unwrap().story_id().unwrap();
    s.selection.ids = vec![rect];
    assert_eq!(s.active_story_id(), None);
    assert_eq!(s.active_para_range(), None);
    s.selection.ids = vec![rect, text_id];
    assert_eq!(s.active_story_id(), Some(story));
    assert_eq!(s.active_para_range(), Some((story, [0, 7])));
    s.selection.ids.clear();
    s.text_edit = Some(TextEditState { story, object: Some(text_id), caret: 5, anchor: 5, before: None });
    assert_eq!(s.active_story_id(), Some(story));
    assert_eq!(s.active_para_range(), Some((story, [4, 7])));
    s.text_edit = Some(TextEditState { story, object: Some(text_id), caret: 1, anchor: 2, before: None });
    assert_eq!(s.active_para_range(), Some((story, [0, 4])));
}

#[test]
fn text_align_writes_layout_align_key() {
    let (mut s, text_id, _) = session_with_text("one\ntwo");
    s.selection.ids = vec![text_id];
    s.run_named("text.align", json!({"align": "justify_all"})).unwrap();
    let story = s.active_story_id().unwrap();
    let style = s.run_named("text.para.query", json!({"story": story.0, "range": "all"})).unwrap()["style"].clone();
    assert_eq!(style["align"], "JustifyAll");
    assert!(style.get("alignment").is_none());
}
