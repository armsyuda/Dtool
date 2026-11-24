use dtool_core::document::{Document, Units};
use dtool_core::selection::PixelSelection;
use dtool_core::{Command, Session};
use serde_json::json;

fn text_session() -> (Session, u64) {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    s.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "abc"})).unwrap();
    let story = s.document.stories.last().unwrap().id.0;
    (s, story)
}

fn story_text(s: &Session, story: u64) -> String {
    s.document.stories.iter().find(|item| item.id.0 == story).unwrap().text.clone()
}

fn undo_depth(s: &Session) -> usize {
    format!("{:?}", s.undo).matches("Change {").count()
}

#[test]
fn k04_text_session_commits_one_undo_step() {
    let (mut s, story) = text_session();
    let depth = undo_depth(&s);
    s.run_named("text.select", json!({"story": story, "range": {"start": 3, "end": 3}})).unwrap();
    s.run_named("text.insert", json!({"story": story, "at": 3, "text": "d"})).unwrap();
    s.run_named("text.insert", json!({"story": story, "at": 4, "text": "e"})).unwrap();
    s.run_named("text.delete", json!({"story": story, "range": {"start": 0, "end": 1}})).unwrap();
    assert_eq!(undo_depth(&s), depth);
    s.run_named("text.edit.end", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "bcde");
    assert_eq!(undo_depth(&s), depth + 1);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "abc");
    s.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "bcde");
}

#[test]
fn k04_text_session_cancel_restores_and_does_not_grow_history() {
    let (mut s, story) = text_session();
    let depth = undo_depth(&s);
    let could_undo = s.undo.can_undo();
    s.run_named("text.select", json!({"story": story, "range": {"start": 0, "end": 0}})).unwrap();
    s.run_named("text.insert", json!({"story": story, "at": 0, "text": "xy"})).unwrap();
    s.run_named("text.edit.end", json!({"cancel": true})).unwrap();
    assert_eq!(story_text(&s, story), "abc");
    assert_eq!(undo_depth(&s), depth);
    assert_eq!(s.undo.can_undo(), could_undo);
    assert!(s.text_edit.is_none());
}

#[test]
fn k04_undo_during_text_session_undoes_the_typing_first() {
    let (mut s, story) = text_session();
    s.run_named("text.select", json!({"story": story, "range": {"start": 3, "end": 3}})).unwrap();
    s.run_named("text.insert", json!({"story": story, "at": 3, "text": "z"})).unwrap();
    assert!(s.can_execute(&Command::new("edit.undo")));
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "abc");
}

#[test]
fn k04_insert_without_session_keeps_one_step_per_insert() {
    let (mut s, story) = text_session();
    let depth = undo_depth(&s);
    s.run_named("text.insert", json!({"story": story, "at": 0, "text": "1"})).unwrap();
    s.run_named("text.insert", json!({"story": story, "at": 0, "text": "2"})).unwrap();
    assert_eq!(undo_depth(&s), depth + 2);
}

#[test]
fn k04_pixel_selection_round_trips_through_replacement_undo() {
    let mut s = Session::new(Document::new([20.0, 20.0], Units::Px, 96.0, 1));
    let mut mask = PixelSelection::empty(20, 20);
    mask.coverage[5] = 255;
    s.document.pixel_selection = Some(mask.clone());
    let value = serde_json::to_value(&s.document).unwrap();
    let back: Document = serde_json::from_value(value).unwrap();
    assert_eq!(back.pixel_selection, Some(mask.clone()));
    let empty = serde_json::to_value(Document::new([20.0, 20.0], Units::Px, 96.0, 1)).unwrap();
    assert!(empty.get("pixel_selection").is_none());
    let high = s.document.next_object_id;
    s.run_named("edit.batch", json!({"commands": [{"name": "object.create", "params": {"name": "a", "pos": [0.0, 0.0], "size": [5.0, 5.0]}}]})).unwrap();
    s.document.pixel_selection = None;
    let generation = s.generation;
    s.run_named("edit.undo", json!({})).unwrap();
    assert!(s.generation > generation);
    assert_eq!(s.document.pixel_selection, Some(mask));
    assert!(s.document.objects.is_empty());
    assert!(s.document.next_object_id > high);
}
