use dtool_core::shortcuts::Focus;
use dtool_core::workspace::Workspace;
use dtool_ui::canvas::frame_text_input::caret_screen_rect;
use dtool_ui::input::{compose_command, ime_output, publish_ime_caret, ImeEvent, IME_COMPOSE, InputHandler};
use egui::{Event, Pos2, RawInput, Rect, Vec2};
use serde_json::json;

fn story_text(workspace: &Workspace, story: u64) -> String {
    workspace
        .session()
        .unwrap()
        .document
        .stories
        .iter()
        .find(|item| item.id.0 == story)
        .unwrap()
        .text
        .clone()
}

fn primed() -> (Workspace, u64) {
    let mut workspace = Workspace::default();
    workspace.new_document("u09");
    workspace
        .session_mut()
        .unwrap()
        .run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "abc"}))
        .unwrap();
    let story = workspace.session().unwrap().document.stories.last().unwrap().id.0;
    workspace
        .session_mut()
        .unwrap()
        .run_named("text.select", json!({"story": story, "range": {"start": 3, "end": 3}}))
        .unwrap();
    (workspace, story)
}

fn ingest(handler: &mut InputHandler, workspace: &mut Workspace, events: Vec<Event>) {
    let ctx = egui::Context::default();
    ctx.begin_pass(RawInput { events, ..Default::default() });
    handler.ingest_into(&ctx, Focus::Canvas, workspace);
    ctx.end_pass().textures_delta.clear();
}

fn preedit(text: &str) -> Event {
    Event::Ime(egui::ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

#[test]
fn preedit_then_escape_commits_one_insert_and_clears_ime() {
    let (mut workspace, story) = primed();
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut workspace, vec![preedit("한")]);
    assert!(handler.is_ime_composing());
    assert_eq!(story_text(&workspace, story), "abc");
    assert!(handler.commit_on_escape(&mut workspace));
    assert!(!handler.is_ime_composing());
    assert_eq!(story_text(&workspace, story), "abc한");
}

#[test]
fn preedit_then_tool_switch_commits_one_insert_and_clears_ime() {
    let (mut workspace, story) = primed();
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut workspace, vec![preedit("한")]);
    workspace.session_mut().unwrap().active_tool = "select".into();
    assert!(handler.commit_on_tool_switch(&mut workspace));
    assert!(!handler.is_ime_composing());
    assert_eq!(story_text(&workspace, story), "abc한");
}

#[test]
fn empty_preedit_then_escape_leaves_document_and_clears_ime() {
    let (mut workspace, story) = primed();
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut workspace, vec![preedit("ㅎ")]);
    handler.handle_ime_event(ImeEvent::CompositionUpdate { text: String::new() });
    assert!(handler.is_ime_composing());
    assert!(!handler.commit_on_escape(&mut workspace));
    assert!(!handler.is_ime_composing());
    assert_eq!(story_text(&workspace, story), "abc");
}

#[test]
fn ime_caret_rect_is_published_for_compose_window() {
    let (workspace, _story) = primed();
    let canvas = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 200.0));
    let view = workspace.session().unwrap().view.clone();
    let rect = caret_screen_rect(workspace.session().unwrap(), &view, canvas).expect("caret");
    let output = ime_output(rect, rect);
    assert_eq!(output.cursor_rect, rect);
    let ctx = egui::Context::default();
    ctx.begin_pass(RawInput::default());
    publish_ime_caret(&ctx, rect);
    let published = ctx.output(|output| output.ime.clone());
    ctx.end_pass().textures_delta.clear();
    let published = published.expect("ime");
    assert_eq!(published.cursor_rect, rect);
}

#[test]
fn ui_ime_compose_follows_the_same_commit_path() {
    let (mut workspace, story) = primed();
    let mut handler = InputHandler::new();
    let command = compose_command(json!({"story": story, "at": 3, "preedit": "한"}));
    assert_eq!(command.name, IME_COMPOSE);
    assert!(handler.compose(&mut workspace, &json!({"story": story, "at": 3, "preedit": "한"})));
    assert!(handler.is_ime_composing());
    assert_eq!(story_text(&workspace, story), "abc");
    assert!(handler.compose(&mut workspace, &json!({"story": story, "at": 3, "commit": "한"})));
    assert!(!handler.is_ime_composing());
    assert_eq!(story_text(&workspace, story), "abc한");
}
