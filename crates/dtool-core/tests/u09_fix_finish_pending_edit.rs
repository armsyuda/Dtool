use dtool_core::document::{Document, Units};
use dtool_core::table::model::StoryId;
use dtool_core::workspace::{ContextId, Workspace};
use serde_json::json;

fn editing(ws: &mut Workspace, text: &str, caret: usize) -> (ContextId, ContextId, u64) {
    let a = ws.insert_context("a", None, Document::new([200.0, 100.0], Units::Px, 96.0, 1), 0);
    let b = ws.insert_context("b", None, Document::new([200.0, 100.0], Units::Px, 96.0, 1), 0);
    ws.activate(a).unwrap();
    let s = &mut ws.get_mut(a).unwrap().session;
    s.run_named("text.art.add", json!({ "pos": [0.0, 0.0], "text": text })).unwrap();
    let id = s.document.objects.last().unwrap().id;
    let story = s.document.get_object(id).unwrap().story_id().unwrap().0;
    s.run_named("text.select", json!({ "story": story, "id": id.0, "range": { "start": caret, "end": caret } })).unwrap();
    (a, b, story)
}

fn story_text(ws: &Workspace, id: ContextId, story: u64) -> String {
    ws.get(id).unwrap().session.document.stories.iter().find(|st| st.id.0 == story).unwrap().text.clone()
}

#[test]
fn finish_with_preedit_commits_once_then_ends_edit() {
    let mut ws = Workspace::default();
    let (a, _, story) = editing(&mut ws, "ab", 2);
    let steps = ws.get(a).unwrap().session.undo.applied_steps();
    ws.finish_pending_edit_with(a, true, Some((StoryId(story), 2, "한".to_string()))).unwrap();
    assert_eq!(story_text(&ws, a, story), "ab한");
    let s = &ws.get(a).unwrap().session;
    assert!(s.text_edit.is_none());
    assert_eq!(s.undo.applied_steps(), steps + 1);
}

#[test]
fn finish_without_commit_drops_preedit() {
    let mut ws = Workspace::default();
    let (a, _, story) = editing(&mut ws, "ab", 2);
    let steps = ws.get(a).unwrap().session.undo.applied_steps();
    let version = ws.get(a).unwrap().session.document.version;
    ws.finish_pending_edit_with(a, false, Some((StoryId(story), 2, "한".to_string()))).unwrap();
    assert_eq!(story_text(&ws, a, story), "ab");
    let s = &ws.get(a).unwrap().session;
    assert_eq!(s.undo.applied_steps(), steps);
    assert_eq!(s.document.version, version);
}

#[test]
fn finish_with_empty_preedit_changes_nothing() {
    let mut ws = Workspace::default();
    let (a, _, story) = editing(&mut ws, "ab", 1);
    let version = ws.get(a).unwrap().session.document.version;
    ws.finish_pending_edit_with(a, true, Some((StoryId(story), 1, String::new()))).unwrap();
    assert_eq!(story_text(&ws, a, story), "ab");
    assert_eq!(ws.get(a).unwrap().session.document.version, version);
}

#[test]
fn tab_switch_commits_core_preedit_once() {
    let mut ws = Workspace::default();
    let (a, b, story) = editing(&mut ws, "ab", 2);
    ws.get_mut(a).unwrap().session.run_named("text.edit.compose", json!({ "text": "글" })).unwrap();
    ws.activate(b).unwrap();
    assert_eq!(story_text(&ws, a, story), "ab글");
    let s = &ws.get(a).unwrap().session;
    assert!(s.text_edit.is_none());
    assert_eq!(s.text_composing, None);
}

#[test]
fn ui_preedit_replaces_core_preedit_no_double_insert() {
    let mut ws = Workspace::default();
    let (a, _, story) = editing(&mut ws, "ab", 2);
    ws.get_mut(a).unwrap().session.run_named("text.edit.compose", json!({ "text": "하" })).unwrap();
    ws.finish_pending_edit_with(a, true, Some((StoryId(story), 2, "한".to_string()))).unwrap();
    assert_eq!(story_text(&ws, a, story), "ab한");
}