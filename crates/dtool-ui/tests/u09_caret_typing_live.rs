use dtool_core::workspace::Workspace;
use dtool_core::Session;
use dtool_ui::canvas::frame_text_input;
use dtool_ui::input::{edit_at_caret, place_caret, type_at_caret, EditKey};
use egui::{Event, Key, Modifiers};
use serde_json::json;

const HAN: &str = "\u{D55C}";
const GEUL: &str = "\u{AE00}";

fn primed() -> (Workspace, u64) {
    let mut workspace = Workspace::default();
    workspace.new_document("u09");
    let session = workspace.session_mut().unwrap();
    session.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "abc"})).unwrap();
    let story = session.document.stories.last().unwrap().id.0;
    session.run_named("text.select", json!({"story": story, "range": {"start": 3, "end": 3}})).unwrap();
    (workspace, story)
}

fn s(ws: &mut Workspace) -> &mut Session {
    ws.session_mut().unwrap()
}

fn text(ws: &Workspace, story: u64) -> String {
    ws.session().unwrap().document.stories.iter().find(|x| x.id.0 == story).unwrap().text.clone()
}

fn caret(ws: &Workspace) -> (usize, usize) {
    let edit = ws.session().unwrap().text_edit.as_ref().unwrap();
    (edit.caret, edit.anchor)
}

fn steps(ws: &Workspace) -> usize {
    ws.session().unwrap().undo.applied_steps()
}

fn undo(ws: &mut Workspace) {
    s(ws).run_named("edit.undo", json!({})).unwrap();
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

#[test]
fn typing_inserts_at_caret_and_moves_caret() {
    let (mut ws, story) = primed();
    assert!(place_caret(s(&mut ws), 1, 1));
    assert!(type_at_caret(s(&mut ws), "x"));
    assert_eq!((text(&ws, story).as_str(), caret(&ws)), ("axbc", (2, 2)));
    assert!(type_at_caret(s(&mut ws), HAN));
    assert_eq!(text(&ws, story), format!("ax{HAN}bc"));
    assert_eq!(caret(&ws), (3, 3));
}

#[test]
fn typed_text_undoes_and_redoes() {
    let (mut ws, story) = primed();
    type_at_caret(s(&mut ws), "x");
    assert_eq!(text(&ws, story), "abcx");
    undo(&mut ws);
    assert_eq!(text(&ws, story), "abc");
    s(&mut ws).run_named("edit.redo", json!({})).unwrap();
    assert_eq!(text(&ws, story), "abcx");
}

#[test]
fn backspace_deletes_before_caret_and_stops_at_start() {
    let (mut ws, story) = primed();
    assert!(edit_at_caret(s(&mut ws), EditKey::Backspace, false));
    assert_eq!((text(&ws, story).as_str(), caret(&ws)), ("ab", (2, 2)));
    place_caret(s(&mut ws), 0, 0);
    assert!(!edit_at_caret(s(&mut ws), EditKey::Backspace, false));
    assert_eq!(text(&ws, story), "ab");
    undo(&mut ws);
    assert_eq!(text(&ws, story), "abc");
}

#[test]
fn delete_removes_after_caret_and_stops_at_end() {
    let (mut ws, story) = primed();
    assert!(!edit_at_caret(s(&mut ws), EditKey::Delete, false));
    place_caret(s(&mut ws), 1, 1);
    assert!(edit_at_caret(s(&mut ws), EditKey::Delete, false));
    assert_eq!((text(&ws, story).as_str(), caret(&ws)), ("ac", (1, 1)));
    undo(&mut ws);
    assert_eq!(text(&ws, story), "abc");
}

#[test]
fn arrows_home_end_move_caret_without_document_change() {
    let (mut ws, story) = primed();
    let (v, n) = (ws.session().unwrap().document.version, steps(&ws));
    edit_at_caret(s(&mut ws), EditKey::Left, false);
    assert_eq!(caret(&ws), (2, 2));
    edit_at_caret(s(&mut ws), EditKey::Home, false);
    assert_eq!(caret(&ws), (0, 0));
    edit_at_caret(s(&mut ws), EditKey::Left, false);
    assert_eq!(caret(&ws), (0, 0));
    edit_at_caret(s(&mut ws), EditKey::Right, false);
    assert_eq!(caret(&ws), (1, 1));
    edit_at_caret(s(&mut ws), EditKey::End, false);
    edit_at_caret(s(&mut ws), EditKey::Right, false);
    assert_eq!(caret(&ws), (3, 3));
    assert_eq!(text(&ws, story), "abc");
    assert_eq!((ws.session().unwrap().document.version, steps(&ws)), (v, n));
}

#[test]
fn shift_arrows_select_then_typing_replaces_selection() {
    let (mut ws, story) = primed();
    edit_at_caret(s(&mut ws), EditKey::Left, true);
    edit_at_caret(s(&mut ws), EditKey::Left, true);
    assert_eq!(caret(&ws), (1, 3));
    edit_at_caret(s(&mut ws), EditKey::Right, false);
    assert_eq!(caret(&ws), (3, 3));
    edit_at_caret(s(&mut ws), EditKey::Left, true);
    edit_at_caret(s(&mut ws), EditKey::Left, true);
    assert!(type_at_caret(s(&mut ws), GEUL));
    assert_eq!(text(&ws, story), format!("a{GEUL}"));
    assert_eq!(caret(&ws), (2, 2));
    edit_at_caret(s(&mut ws), EditKey::Home, true);
    assert!(edit_at_caret(s(&mut ws), EditKey::Backspace, false));
    assert_eq!(text(&ws, story), "");
}

#[test]
fn enter_breaks_line_and_home_end_stay_on_line() {
    let (mut ws, story) = primed();
    assert!(edit_at_caret(s(&mut ws), EditKey::Enter, false));
    type_at_caret(s(&mut ws), "de");
    assert_eq!(text(&ws, story), "abc\nde");
    edit_at_caret(s(&mut ws), EditKey::Home, false);
    assert_eq!(caret(&ws), (4, 4));
    edit_at_caret(s(&mut ws), EditKey::Left, false);
    edit_at_caret(s(&mut ws), EditKey::Home, false);
    assert_eq!(caret(&ws), (0, 0));
    edit_at_caret(s(&mut ws), EditKey::End, false);
    assert_eq!(caret(&ws), (3, 3));
}

#[test]
fn egui_key_and_text_events_drive_caret_editing() {
    let (mut ws, story) = primed();
    let events = vec![
        Event::Text("x".into()),
        key(Key::ArrowLeft, Modifiers::NONE),
        key(Key::ArrowLeft, Modifiers::NONE),
        key(Key::Backspace, Modifiers::NONE),
        Event::Text("Y".into()),
        key(Key::End, Modifiers::NONE),
        key(Key::Delete, Modifiers::NONE),
    ];
    assert!(frame_text_input::feed_text_input(s(&mut ws), &events));
    assert_eq!(text(&ws, story), "aYcx");
    assert_eq!(caret(&ws), (4, 4));
}

#[test]
fn modified_keys_and_control_text_do_not_edit() {
    let (mut ws, story) = primed();
    let events = vec![key(Key::Backspace, Modifiers::COMMAND), key(Key::ArrowLeft, Modifiers::ALT), Event::Text("\u{7f}".into())];
    assert!(!frame_text_input::feed_text_input(s(&mut ws), &events));
    assert_eq!((text(&ws, story).as_str(), caret(&ws)), ("abc", (3, 3)));
}

#[test]
fn no_text_edit_means_keys_do_nothing() {
    let (mut ws, story) = primed();
    s(&mut ws).run_named("text.edit.end", json!({})).unwrap();
    assert!(!type_at_caret(s(&mut ws), "x"));
    assert!(!edit_at_caret(s(&mut ws), EditKey::Backspace, false));
    assert!(!frame_text_input::feed_text_input(s(&mut ws), &[Event::Text("x".into())]));
    assert_eq!(text(&ws, story), "abc");
}

#[test]
fn consecutive_typing_is_one_undo_step() {
    let (mut ws, story) = primed();
    let n = steps(&ws);
    type_at_caret(s(&mut ws), HAN);
    type_at_caret(s(&mut ws), GEUL);
    edit_at_caret(s(&mut ws), EditKey::Backspace, false);
    type_at_caret(s(&mut ws), GEUL);
    assert_eq!(text(&ws, story), format!("abc{HAN}{GEUL}"));
    assert!(steps(&ws) <= n + 1);
    undo(&mut ws);
    assert_eq!(text(&ws, story), "abc");
}

#[test]
fn caret_move_splits_typing_steps() {
    let (mut ws, story) = primed();
    type_at_caret(s(&mut ws), "x");
    type_at_caret(s(&mut ws), "y");
    edit_at_caret(s(&mut ws), EditKey::Home, false);
    type_at_caret(s(&mut ws), "z");
    assert_eq!(text(&ws, story), "zabcxy");
    undo(&mut ws);
    assert_eq!(text(&ws, story), "abcxy");
}
