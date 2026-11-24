use dtool_core::shortcuts::{Focus, Modifiers};
use dtool_core::workspace::Workspace;
use dtool_ui::input::{insert_command, InputHandler, ShortcutDispatcher};
use egui::{Event, ImeEvent, Key, RawInput};
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

fn primed() -> (Workspace, u64, String, u64) {
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
    workspace.session_mut().unwrap().active_tool = "text".into();
    let tool = workspace.session().unwrap().active_tool.clone();
    let version = workspace.session().unwrap().document.version;
    (workspace, story, tool, version)
}

fn ingest(handler: &mut InputHandler, workspace: &mut Workspace, events: Vec<Event>) {
    let ctx = egui::Context::default();
    ctx.begin_pass(RawInput { events, ..Default::default() });
    handler.ingest_into(&ctx, Focus::Canvas, workspace);
    ctx.end_pass().textures_delta.clear();
}

fn preedit(text: &str) -> Event {
    Event::Ime(ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

fn commit(text: &str) -> Event {
    Event::Ime(ImeEvent::Commit(text.into()))
}

fn key_v() -> Event {
    Event::Key { key: Key::V, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::default() }
}

#[test]
fn ime_preedit_blocks_canvas_shortcuts_and_document_unchanged() {
    let (mut workspace, story, tool, version) = primed();
    let before = story_text(&workspace, story);
    let generation = workspace.session().unwrap().generation;
    let mut dispatcher = ShortcutDispatcher::new(InputHandler::new());
    ingest(dispatcher.handler_mut(), &mut workspace, vec![preedit("ㅎ")]);
    assert!(dispatcher.handler().is_ime_composing());
    assert_eq!(dispatcher.handler().state().ime_text, "ㅎ");
    assert_eq!(dispatcher.dispatch_shortcut(Focus::Canvas, "v".into(), Modifiers { ctrl: false, shift: false, alt: false }), None);
    ingest(dispatcher.handler_mut(), &mut workspace, vec![key_v()]);
    assert!(dispatcher.handler().is_ime_composing());
    assert_eq!(workspace.session().unwrap().active_tool, tool);
    assert_eq!(workspace.session().unwrap().document.version, version);
    assert_eq!(workspace.session().unwrap().generation, generation);
    assert_eq!(story_text(&workspace, story), before);
}

#[test]
fn ime_commit_inserts_into_focused_story_and_is_one_undo() {
    let (mut workspace, story, _, _) = primed();
    let mut handler = InputHandler::new();
    ingest(&mut handler, &mut workspace, vec![preedit("ㅎ")]);
    assert_eq!(story_text(&workspace, story), "abc");
    ingest(&mut handler, &mut workspace, vec![commit("한")]);
    assert!(!handler.is_ime_composing());
    assert_eq!(story_text(&workspace, story), "abc한");
    assert_eq!(insert_command(story, 3, "한").name, "text.insert");
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(story_text(&workspace, story), "abc");
}
