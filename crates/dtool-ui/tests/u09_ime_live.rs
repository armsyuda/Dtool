use dtool_core::view_state::ViewState;
use dtool_core::workspace::Workspace;
use dtool_ui::canvas::frame_text_input;
use dtool_ui::input::{EditRoute, InputHandler};
use dtool_ui::shortcuts::Focus;
use egui::{Context, Event, ImeEvent, Key, Pos2, RawInput, Rect};
use serde_json::json;

const HIEUT: &str = "\u{314E}";
const HA: &str = "\u{D558}";
const HAN: &str = "\u{D55C}";

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
    workspace.session().unwrap().document.stories.iter().find(|s| s.id.0 == story).unwrap().text.clone()
}

fn steps(workspace: &Workspace) -> usize {
    workspace.session().unwrap().undo.applied_steps()
}

fn version(workspace: &Workspace) -> u64 {
    workspace.session().unwrap().document.version
}

fn preedit(text: &str) -> Event {
    Event::Ime(ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

fn commit(text: &str) -> Event {
    Event::Ime(ImeEvent::Commit(text.into()))
}

fn escape() -> Event {
    Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

fn ingest(handler: &mut InputHandler, workspace: &mut Workspace, events: Vec<Event>, focus: Focus) {
    let ctx = Context::default();
    ctx.begin_pass(RawInput { events, ..Default::default() });
    handler.ingest_into(&ctx, focus, workspace);
}

fn feed(workspace: &mut Workspace, events: Vec<Event>) -> bool {
    frame_text_input::feed_text_input(workspace.session_mut().unwrap(), &events)
}

#[test]
fn preedit_steps_are_shown_but_never_reach_the_document() {
    let (mut ws, story) = primed();
    let (v, n) = (version(&ws), steps(&ws));
    let mut handler = InputHandler::new();
    for step in [HIEUT, HA, HAN] {
        ingest(&mut handler, &mut ws, vec![preedit(step)], Focus::Canvas);
        assert!(handler.is_ime_composing());
        assert_eq!(handler.state().ime_text, step);
        assert_eq!(text(&ws, story), "abc");
    }
    assert_eq!(version(&ws), v);
    assert_eq!(steps(&ws), n);
}

#[test]
fn commit_inserts_hangul_at_caret_with_one_undo_step() {
    let (mut ws, story) = primed();
    let mut handler = InputHandler::new();
    for step in [HIEUT, HA, HAN] {
        ingest(&mut handler, &mut ws, vec![preedit(step)], Focus::Canvas);
    }
    ingest(&mut handler, &mut ws, vec![commit(HAN)], Focus::Canvas);
    assert!(!handler.is_ime_composing());
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
    assert_eq!(ws.session().unwrap().text_edit.as_ref().unwrap().caret, 4);
    ws.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(text(&ws, story), "abc");
    ws.session_mut().unwrap().run_named("edit.redo", json!({})).unwrap();
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
}

#[test]
fn escape_during_composition_commits_and_ends_edit() {
    let (mut ws, story) = primed();
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut ws, vec![preedit(HA)], Focus::Canvas);
    ingest(&mut handler, &mut ws, vec![escape()], Focus::Canvas);
    assert!(!handler.is_ime_composing());
    assert_eq!(text(&ws, story), format!("abc{HA}"));
    assert!(ws.session().unwrap().text_edit.is_none());
    ws.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(text(&ws, story), "abc");
}

#[test]
fn escape_in_same_frame_as_preedit_still_commits() {
    let (mut ws, story) = primed();
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut ws, vec![preedit(HIEUT), preedit(HAN), escape()], Focus::Canvas);
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
    assert!(ws.session().unwrap().text_edit.is_none());
}

#[test]
fn ime_disabled_commits_pending_preedit() {
    let (mut ws, story) = primed();
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut ws, vec![preedit(HAN)], Focus::Canvas);
    #[allow(deprecated)]
    ingest(&mut handler, &mut ws, vec![Event::Ime(ImeEvent::Disabled)], Focus::Canvas);
    assert!(!handler.is_ime_composing());
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
}

#[test]
fn empty_preedit_cancels_without_document_change() {
    let (mut ws, story) = primed();
    let (v, n) = (version(&ws), steps(&ws));
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut ws, vec![preedit(HIEUT)], Focus::Canvas);
    ingest(&mut handler, &mut ws, vec![preedit("")], Focus::Canvas);
    assert!(!handler.is_ime_composing());
    ingest(&mut handler, &mut ws, vec![escape()], Focus::Canvas);
    assert_eq!(text(&ws, story), "abc");
    assert_eq!((version(&ws), steps(&ws)), (v, n));
}

#[test]
fn tool_and_tab_switch_commit_pending_preedit() {
    for tab in [false, true] {
        let (mut ws, story) = primed();
        let mut handler = InputHandler::new();
        ingest(&mut handler, &mut ws, vec![preedit(HAN)], Focus::Canvas);
        let inserted = if tab { handler.commit_on_tab_switch(&mut ws) } else { handler.commit_on_tool_switch(&mut ws) };
        assert!(inserted);
        assert_eq!(text(&ws, story), format!("abc{HAN}"));
        assert!(ws.session().unwrap().text_edit.is_none());
    }
}

#[test]
fn canvas_path_preedit_is_overlay_only_then_commit_inserts_once() {
    let (mut ws, story) = primed();
    let (v, n) = (version(&ws), steps(&ws));
    assert!(!feed(&mut ws, vec![preedit(HIEUT)]));
    assert!(!feed(&mut ws, vec![preedit(HAN)]));
    assert_eq!(frame_text_input::preedit_text().as_deref(), Some(HAN));
    assert!(frame_text_input::is_composing());
    assert_eq!((text(&ws, story).as_str(), version(&ws), steps(&ws)), ("abc", v, n));
    assert!(feed(&mut ws, vec![commit(HAN), Event::Text(HAN.into())]));
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
    assert_eq!(frame_text_input::preedit_text(), None);
    ws.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(text(&ws, story), "abc");
}

#[test]
fn canvas_path_escape_commits_preedit_then_finish_ends_edit() {
    let (mut ws, story) = primed();
    feed(&mut ws, vec![preedit(HAN)]);
    assert!(feed(&mut ws, vec![escape()]));
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
    assert!(!frame_text_input::is_composing());
    frame_text_input::finish_text_input(ws.session_mut().unwrap());
    assert!(ws.session().unwrap().text_edit.is_none());
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
}

#[test]
fn canvas_path_finish_while_composing_commits_once() {
    let (mut ws, story) = primed();
    feed(&mut ws, vec![preedit(HAN)]);
    assert!(frame_text_input::finish_text_input(ws.session_mut().unwrap()));
    assert!(!frame_text_input::finish_text_input(ws.session_mut().unwrap()));
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
    ws.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(text(&ws, story), "abc");
}

#[test]
fn ime_candidate_window_follows_caret() {
    let (ws, _) = primed();
    let canvas = Rect::from_min_size(Pos2::new(100.0, 50.0), egui::vec2(600.0, 400.0));
    let view = ViewState::default();
    let ctx = Context::default();
    ctx.begin_pass(RawInput::default());
    frame_text_input::publish_caret_ime(&ctx, ws.session().unwrap(), &view, canvas);
    let first = ctx.output(|o| o.ime.clone()).expect("ime").cursor_rect;
    assert!(canvas.contains(first.min), "{first:?}");
}

#[test]
fn ime_candidate_window_moves_with_caret_after_typing() {
    let (mut ws, _) = primed();
    let canvas = Rect::from_min_size(Pos2::new(100.0, 50.0), egui::vec2(600.0, 400.0));
    let view = ViewState::default();
    let ctx = Context::default();
    ctx.begin_pass(RawInput::default());
    frame_text_input::publish_caret_ime(&ctx, ws.session().unwrap(), &view, canvas);
    let first = ctx.output(|o| o.ime.clone()).expect("ime").cursor_rect;
    feed(&mut ws, vec![Event::Text("dd".into())]);
    let ctx = Context::default();
    ctx.begin_pass(RawInput::default());
    frame_text_input::publish_caret_ime(&ctx, ws.session().unwrap(), &view, canvas);
    let second = ctx.output(|o| o.ime.clone()).expect("ime").cursor_rect;
    assert!(second.min.x > first.min.x, "{first:?} {second:?}");
}

#[test]
fn typing_in_a_focused_panel_field_does_not_reach_canvas_text() {
    let (mut ws, story) = primed();
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut ws, vec![Event::Text("7".into())], Focus::TextField);
    assert_eq!(text(&ws, story), "abc");
}

#[test]
fn core_route_commit_inserts_hangul() {
    let (mut ws, story) = primed();
    frame_text_input::set_edit_route(EditRoute::Core);
    feed(&mut ws, vec![preedit(HAN)]);
    assert_eq!(text(&ws, story), "abc");
    feed(&mut ws, vec![commit(HAN)]);
    frame_text_input::set_edit_route(EditRoute::Session);
    assert_eq!(text(&ws, story), format!("abc{HAN}"));
}