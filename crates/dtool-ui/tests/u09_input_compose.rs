use dtool_core::workspace::Workspace;
use dtool_ui::canvas::frame_text_input;
use dtool_ui::input::{
    apply_to_session, core_edit_command, ComposeMachine, EditAction, EditKey, EditRoute, CORE_EDIT_COMMANDS,
};
use egui::{Event, ImeEvent, Key, Modifiers};
use serde_json::json;

const JAMO_G: &str = "\u{3131}";
const GA: &str = "\u{AC00}";
const GAK: &str = "\u{AC01}";
const NA: &str = "\u{B098}";

fn preedit(text: &str) -> Event {
    Event::Ime(ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

fn commit(text: &str) -> Event {
    Event::Ime(ImeEvent::Commit(text.into()))
}

fn key(key: Key, shift: bool) -> Event {
    let modifiers = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn primed() -> (Workspace, u64) {
    let mut workspace = Workspace::default();
    workspace.new_document("u09");
    let session = workspace.session_mut().unwrap();
    session.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "abc"})).unwrap();
    let story = session.document.stories.last().unwrap().id.0;
    session.run_named("text.select", json!({"story": story, "range": {"start": 3, "end": 3}})).unwrap();
    (workspace, story)
}

fn text(workspace: &Workspace, story: u64) -> String {
    let session = workspace.session().unwrap();
    session.document.stories.iter().find(|item| item.id.0 == story).unwrap().text.clone()
}

fn caret(workspace: &Workspace) -> (usize, usize) {
    let edit = workspace.session().unwrap().text_edit.as_ref().unwrap();
    (edit.caret, edit.anchor)
}

fn run(workspace: &mut Workspace, machine: &mut ComposeMachine, events: Vec<Event>) -> Vec<EditAction> {
    let actions = machine.feed(&events);
    apply_to_session(workspace.session_mut().unwrap(), &actions, EditRoute::Session);
    actions
}

#[test]
fn hangul_jamo_steps_then_commit_insert_at_caret() {
    let (mut workspace, story) = primed();
    let version = workspace.session().unwrap().document.version;
    let mut machine = ComposeMachine::new();
    for step in [JAMO_G, GA, GAK] {
        let actions = run(&mut workspace, &mut machine, vec![preedit(step)]);
        assert_eq!(actions, vec![EditAction::Preedit(step.into())]);
        assert_eq!(machine.preedit(), Some(step));
        assert_eq!(text(&workspace, story), "abc");
    }
    assert_eq!(workspace.session().unwrap().document.version, version);
    let actions = run(&mut workspace, &mut machine, vec![commit(GAK)]);
    assert_eq!(actions, vec![EditAction::Commit(GAK.into())]);
    assert!(!machine.is_composing());
    assert_eq!(text(&workspace, story), format!("abc{GAK}"));
    assert_eq!(caret(&workspace), (4, 4));
}

#[test]
fn preedit_then_cancel_leaves_document() {
    let (mut workspace, story) = primed();
    let mut machine = ComposeMachine::new();
    run(&mut workspace, &mut machine, vec![preedit(GA)]);
    let actions = run(&mut workspace, &mut machine, vec![preedit("")]);
    assert_eq!(actions, vec![EditAction::Cancel]);
    assert!(!machine.is_composing());
    assert_eq!(machine.preedit(), None);
    assert_eq!(text(&workspace, story), "abc");
}

#[test]
fn keys_during_composition_do_not_double_apply() {
    let (mut workspace, story) = primed();
    let mut machine = ComposeMachine::new();
    run(&mut workspace, &mut machine, vec![preedit(GA)]);
    let actions = run(&mut workspace, &mut machine, vec![key(Key::Backspace, false), key(Key::ArrowLeft, false), key(Key::Enter, false)]);
    assert!(actions.is_empty());
    assert_eq!(text(&workspace, story), "abc");
    let actions = run(&mut workspace, &mut machine, vec![commit(GA), key(Key::Enter, false)]);
    assert_eq!(actions, vec![EditAction::Commit(GA.into())]);
    assert_eq!(text(&workspace, story), format!("abc{GA}"));
    let actions = run(&mut workspace, &mut machine, vec![key(Key::Backspace, false)]);
    assert_eq!(actions, vec![EditAction::Key { key: EditKey::Backspace, shift: false }]);
    assert_eq!(text(&workspace, story), "abc");
}

#[test]
fn text_event_after_commit_is_not_inserted_twice() {
    let (mut workspace, story) = primed();
    let mut machine = ComposeMachine::new();
    run(&mut workspace, &mut machine, vec![preedit(GA)]);
    let actions = run(&mut workspace, &mut machine, vec![commit(GA), Event::Text(GA.into())]);
    assert_eq!(actions.len(), 1);
    assert_eq!(text(&workspace, story), format!("abc{GA}"));
}

#[test]
fn two_syllables_in_one_batch_keep_order() {
    let (mut workspace, story) = primed();
    let mut machine = ComposeMachine::new();
    run(&mut workspace, &mut machine, vec![preedit(GA)]);
    run(&mut workspace, &mut machine, vec![commit(GA), preedit(NA)]);
    assert_eq!(machine.preedit(), Some(NA));
    run(&mut workspace, &mut machine, vec![commit(NA)]);
    assert_eq!(text(&workspace, story), format!("abc{GA}{NA}"));
    assert_eq!(caret(&workspace), (5, 5));
}

#[test]
fn disabled_and_escape_commit_pending_preedit() {
    let (mut workspace, story) = primed();
    let mut machine = ComposeMachine::new();
    run(&mut workspace, &mut machine, vec![preedit(GA)]);
    #[allow(deprecated)]
    let actions = run(&mut workspace, &mut machine, vec![Event::Ime(ImeEvent::Disabled)]);
    assert_eq!(actions, vec![EditAction::Commit(GA.into())]);
    run(&mut workspace, &mut machine, vec![preedit(NA)]);
    let actions = run(&mut workspace, &mut machine, vec![key(Key::Escape, false)]);
    assert_eq!(actions, vec![EditAction::Commit(NA.into()), EditAction::Key { key: EditKey::Escape, shift: false }]);
    assert_eq!(text(&workspace, story), format!("abc{GA}{NA}"));
}

#[test]
fn arrows_selection_and_typing_follow_caret() {
    let (mut workspace, story) = primed();
    let mut machine = ComposeMachine::new();
    run(&mut workspace, &mut machine, vec![key(Key::ArrowLeft, false)]);
    assert_eq!(caret(&workspace), (2, 2));
    run(&mut workspace, &mut machine, vec![Event::Text("x".into())]);
    assert_eq!(text(&workspace, story), "abxc");
    run(&mut workspace, &mut machine, vec![key(Key::ArrowLeft, true), key(Key::ArrowLeft, true)]);
    assert_eq!(caret(&workspace), (1, 3));
    run(&mut workspace, &mut machine, vec![preedit(GA)]);
    run(&mut workspace, &mut machine, vec![commit(GA)]);
    assert_eq!(text(&workspace, story), format!("a{GA}c"));
    run(&mut workspace, &mut machine, vec![key(Key::Home, false), key(Key::Delete, false)]);
    assert_eq!(text(&workspace, story), format!("{GA}c"));
    run(&mut workspace, &mut machine, vec![key(Key::End, false), key(Key::Enter, false)]);
    assert_eq!(text(&workspace, story), format!("{GA}c\n"));
}

#[test]
fn edit_session_is_one_undo_step() {
    let (mut workspace, story) = primed();
    let mut machine = ComposeMachine::new();
    run(&mut workspace, &mut machine, vec![preedit(GA), commit(GA)]);
    run(&mut workspace, &mut machine, vec![preedit(NA), commit(NA)]);
    let session = workspace.session_mut().unwrap();
    session.run_named("text.edit.end", json!({})).unwrap();
    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(text(&workspace, story), "abc");
}

#[test]
fn frame_hooks_hold_preedit_and_commit_on_finish() {
    let (mut workspace, story) = primed();
    let session = workspace.session_mut().unwrap();
    assert!(!frame_text_input::feed_text_input(session, &[preedit(JAMO_G), preedit(GA)]));
    assert!(frame_text_input::is_composing());
    assert_eq!(frame_text_input::preedit_text().as_deref(), Some(GA));
    assert!(frame_text_input::finish_text_input(session));
    assert!(session.text_edit.is_none());
    assert_eq!(frame_text_input::preedit_text(), None);
    assert_eq!(text(&workspace, story), format!("abc{GA}"));
}

#[test]
fn core_route_builds_pr_u09_2_commands() {
    let (workspace, _story) = primed();
    let session = workspace.session().unwrap();
    let names: Vec<String> = [
        EditAction::Preedit(GA.into()),
        EditAction::Commit(GA.into()),
        EditAction::Cancel,
        EditAction::Key { key: EditKey::Enter, shift: false },
        EditAction::Key { key: EditKey::Backspace, shift: false },
        EditAction::Key { key: EditKey::Delete, shift: false },
        EditAction::Key { key: EditKey::Left, shift: true },
    ]
    .iter()
    .map(|action| core_edit_command(session, action).unwrap().name)
    .collect();
    assert_eq!(names, CORE_EDIT_COMMANDS.to_vec());
    let caret = core_edit_command(session, &EditAction::Key { key: EditKey::Left, shift: true }).unwrap();
    assert_eq!(caret.params["to"], 2);
    assert_eq!(caret.params["extend"], true);
    assert!(core_edit_command(session, &EditAction::Key { key: EditKey::Escape, shift: false }).is_none());
}

#[test]
fn ime_rect_is_published_while_editing() {
    let (workspace, _story) = primed();
    let session = workspace.session().unwrap();
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 400.0));
    let ctx = egui::Context::default();
    ctx.begin_pass(egui::RawInput::default());
    frame_text_input::publish_caret_ime(&ctx, session, &session.view, canvas);
    let ime = ctx.output(|output| output.ime.clone());
    ctx.end_pass().textures_delta.clear();
    let ime = ime.expect("ime");
    let expected = frame_text_input::caret_screen_rect(session, &session.view, canvas).unwrap();
    assert_eq!(ime.cursor_rect, expected);
}
