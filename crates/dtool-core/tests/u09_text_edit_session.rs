use dtool_core::document::{Document, Units};
use dtool_core::Session;
use dtool_protocol::Command;
use serde_json::json;

fn editing(text: &str, caret: usize) -> (Session, u64) {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    s.run_named("text.art.add", json!({ "pos": [0.0, 0.0], "text": text })).unwrap();
    let id = s.document.objects.last().unwrap().id;
    let story = s.document.get_object(id).unwrap().story_id().unwrap().0;
    s.run_named("text.select", json!({ "story": story, "id": id.0, "range": { "start": caret, "end": caret } })).unwrap();
    (s, story)
}

fn story_text(s: &Session, story: u64) -> String {
    s.document.stories.iter().find(|st| st.id.0 == story).unwrap().text.clone()
}

fn caret(s: &Session) -> (usize, usize) {
    let edit = s.text_edit.as_ref().unwrap();
    (edit.caret, edit.anchor)
}

#[test]
fn type_hangul_at_caret_one_undo_step() {
    let (mut s, story) = editing("ab", 1);
    let steps = s.undo.applied_steps();
    s.run_named("text.edit.type", json!({ "text": "한" })).unwrap();
    s.run_named("text.edit.type", json!({ "text": "글" })).unwrap();
    assert_eq!(story_text(&s, story), "a한글b");
    assert_eq!(caret(&s), (3, 3));
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "ab");
    assert_eq!(s.undo.applied_steps(), steps);
}

#[test]
fn compose_does_not_change_document_version() {
    let (mut s, story) = editing("ab", 2);
    let version = s.document.version;
    let generation = s.generation;
    s.run_named("text.edit.compose", json!({ "text": "ㅎ" })).unwrap();
    s.run_named("text.edit.compose", json!({ "text": "하" })).unwrap();
    assert_eq!(s.document.version, version);
    assert_eq!(s.generation, generation);
    assert_eq!(story_text(&s, story), "ab");
    assert_eq!(s.text_composing.as_deref(), Some("하"));
}

#[test]
fn compose_commit_inserts_and_moves_caret() {
    let (mut s, story) = editing("ab", 1);
    s.run_named("text.edit.compose", json!({ "text": "하" })).unwrap();
    s.run_named("text.edit.compose_commit", json!({ "text": "한" })).unwrap();
    assert_eq!(story_text(&s, story), "a한b");
    assert_eq!(caret(&s), (2, 2));
    assert_eq!(s.text_composing, None);
    s.run_named("text.edit.compose", json!({ "text": "글" })).unwrap();
    s.run_named("text.edit.compose_commit", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "a한글b");
}

#[test]
fn compose_cancel_leaves_document() {
    let (mut s, story) = editing("ab", 1);
    let version = s.document.version;
    s.run_named("text.edit.compose", json!({ "text": "ㅎ" })).unwrap();
    s.run_named("text.edit.compose_cancel", json!({})).unwrap();
    assert_eq!(s.text_composing, None);
    assert_eq!(s.document.version, version);
    s.run_named("text.edit.end", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "ab");
}

#[test]
fn type_replaces_selection() {
    let (mut s, story) = editing("hello", 0);
    s.run_named("text.edit.caret", json!({ "to": 4, "extend": true })).unwrap();
    assert_eq!(caret(&s), (4, 0));
    s.run_named("text.edit.type", json!({ "text": "J" })).unwrap();
    assert_eq!(story_text(&s, story), "Jo");
    assert_eq!(caret(&s), (1, 1));
}

#[test]
fn backspace_merges_with_typing_step() {
    let (mut s, story) = editing("ab", 2);
    let steps = s.undo.applied_steps();
    s.run_named("text.edit.type", json!({ "text": "cd" })).unwrap();
    s.run_named("text.edit.backspace", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "abc");
    s.run_named("text.edit.caret", json!({ "to": 0 })).unwrap();
    s.run_named("text.edit.delete_forward", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "bc");
    s.run_named("text.edit.end", json!({})).unwrap();
    assert_eq!(s.undo.applied_steps(), steps + 2);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "abc");
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "ab");
}

#[test]
fn edit_end_while_composing_commits_preedit() {
    let (mut s, story) = editing("ab", 2);
    s.run_named("text.edit.compose", json!({ "text": "한" })).unwrap();
    s.run_named("text.edit.end", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "ab한");
    assert_eq!(s.text_composing, None);
    assert!(s.text_edit.is_none());
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "ab");
}

#[test]
fn edit_end_cancel_drops_preedit() {
    let (mut s, story) = editing("ab", 2);
    s.run_named("text.edit.compose", json!({ "text": "한" })).unwrap();
    s.run_named("text.edit.end", json!({ "cancel": true })).unwrap();
    assert_eq!(story_text(&s, story), "ab");
    assert_eq!(s.text_composing, None);
}

#[test]
fn caret_move_closes_typing_step() {
    let (mut s, story) = editing("", 0);
    let steps = s.undo.applied_steps();
    s.run_named("text.edit.type", json!({ "text": "x" })).unwrap();
    s.run_named("text.edit.caret", json!({ "by": -1 })).unwrap();
    s.run_named("text.edit.type", json!({ "text": "y" })).unwrap();
    s.run_named("text.edit.end", json!({})).unwrap();
    assert_eq!(story_text(&s, story), "yx");
    assert_eq!(s.undo.applied_steps(), steps + 2);
}

#[test]
fn api_insert_before_caret_shifts_caret() {
    let (mut s, story) = editing("ab", 2);
    s.run_named("text.insert", json!({ "story": story, "at": 0, "text": "zz" })).unwrap();
    assert_eq!(caret(&s), (4, 4));
    s.run_named("text.edit.type", json!({ "text": "!" })).unwrap();
    assert_eq!(story_text(&s, story), "zzab!");
}

#[test]
fn edit_commands_need_text_edit() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    assert!(s.run_named("text.edit.type", json!({ "text": "a" })).is_err());
    assert!(s.run_named("text.edit.compose", json!({ "text": "a" })).is_err());
}

fn ui_type(s: &mut Session, story: u64, text: &str) {
    let (caret, anchor) = caret(s);
    let at = caret.min(anchor);
    s.run_named("text.insert", json!({ "story": story, "at": at, "text": text })).unwrap();
    let to = at + text.chars().count();
    s.run_named("text.select", json!({ "story": story, "range": { "start": to, "end": to } })).unwrap();
}

fn ui_backspace(s: &mut Session, story: u64) {
    let (caret, _) = caret(s);
    s.run_named("text.delete", json!({ "story": story, "range": { "start": caret - 1, "end": caret } })).unwrap();
    s.run_named("text.select", json!({ "story": story, "range": { "start": caret - 1, "end": caret - 1 } })).unwrap();
}

#[test]
fn caret_move_typing_ab_cd_undoes_in_two_steps() {
    let (mut s, story) = editing("xy", 2);
    let steps = s.undo.applied_steps();
    s.run_named("text.edit.type", json!({ "text": "a" })).unwrap();
    s.run_named("text.edit.type", json!({ "text": "b" })).unwrap();
    s.run_named("text.edit.caret", json!({ "to": 0 })).unwrap();
    s.run_named("text.edit.type", json!({ "text": "c" })).unwrap();
    s.run_named("text.edit.type", json!({ "text": "d" })).unwrap();
    assert_eq!(story_text(&s, story), "cdxyab");
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "xyab");
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "xy");
    assert_eq!(s.undo.applied_steps(), steps);
}

#[test]
fn ui_select_caret_move_closes_typing_step() {
    let (mut s, story) = editing("xy", 2);
    let steps = s.undo.applied_steps();
    ui_type(&mut s, story, "a");
    ui_type(&mut s, story, "b");
    s.run_named("text.select", json!({ "story": story, "range": { "start": 0, "end": 0 } })).unwrap();
    ui_type(&mut s, story, "c");
    ui_type(&mut s, story, "d");
    assert_eq!(story_text(&s, story), "cdxyab");
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "xyab");
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "xy");
    assert_eq!(s.undo.applied_steps(), steps);
}

#[test]
fn ui_typing_and_backspace_without_caret_move_stay_one_step() {
    let (mut s, story) = editing("xy", 2);
    let steps = s.undo.applied_steps();
    ui_type(&mut s, story, "a");
    ui_type(&mut s, story, "b");
    ui_backspace(&mut s, story);
    ui_type(&mut s, story, "c");
    assert_eq!(story_text(&s, story), "xyac");
    assert_eq!(caret(&s), (4, 4));
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&s, story), "xy");
    assert_eq!(s.undo.applied_steps(), steps);
}

#[test]
fn text_delete_moves_caret_with_text() {
    let (mut s, story) = editing("abcdef", 5);
    s.run_named("text.delete", json!({ "story": story, "range": { "start": 1, "end": 3 } })).unwrap();
    assert_eq!(caret(&s), (3, 3));
    s.run_named("text.select", json!({ "story": story, "range": { "start": 2, "end": 2 } })).unwrap();
    s.run_named("text.delete", json!({ "story": story, "range": { "start": 1, "end": 3 } })).unwrap();
    assert_eq!(caret(&s), (1, 1));
}
