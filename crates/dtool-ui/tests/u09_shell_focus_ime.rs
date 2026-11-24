#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::shortcuts::Focus;
use dtool_core::workspace::ContextId;
use dtool_ui::canvas::frame_text_input;
use dtool_ui::shell::AppShell;
use egui::{Event, Key, Modifiers, RawInput};
use serde_json::json;

fn story_text(shell: &AppShell, id: ContextId, story: u64) -> String {
    shell.workspace.get(id).unwrap().session.document.stories.iter().find(|item| item.id.0 == story).unwrap().text.clone()
}

fn prime(shell: &mut AppShell, name: &str) -> (ContextId, u64) {
    let id = shell.workspace.new_document(name);
    shell.workspace.activate(id).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    session.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "abc"})).unwrap();
    let story = session.document.stories.last().unwrap().id.0;
    session.run_named("text.select", json!({"story": story, "range": {"start": 3, "end": 3}})).unwrap();
    shell.finish_ime_on_switch();
    (id, story)
}

fn frame(shell: &mut AppShell, events: Vec<Event>) {
    let ctx = egui::Context::default();
    ctx.begin_pass(RawInput { events: events.clone(), ..Default::default() });
    shell.focus = shell.current_focus(&ctx);
    shell.ingest_ime(&ctx);
    if let Some(session) = shell.workspace.session_mut() {
        frame_text_input::feed_text_input(session, &events);
    }
    if !shell.input_blocked(&ctx) {
        shell.handle_canvas_tab(&ctx);
        shell.handle_shortcuts(&ctx);
    }
    shell.finish_ime_on_switch();
    ctx.end_pass().textures_delta.clear();
}

fn preedit(text: &str) -> Event {
    Event::Ime(egui::ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

fn key(key: Key) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn tool(shell: &AppShell) -> String {
    shell.workspace.session().unwrap().active_tool.clone()
}

#[test]
fn focus_model_dialog_menu_text_canvas() {
    let _home = test_home::enter("focus_model_dialog_menu_text_canvas");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    assert_eq!(shell.current_focus(&ctx), Focus::Canvas);
    shell.about.open = true;
    assert_eq!(shell.current_focus(&ctx), Focus::Dialog);
    shell.about.open = false;
    shell.add_preset.open = true;
    assert_eq!(shell.current_focus(&ctx), Focus::Dialog);
    shell.add_preset.open = false;
    prime(&mut shell, "a");
    assert_eq!(shell.current_focus(&ctx), Focus::TextField);
}

#[test]
fn canvas_widget_focus_is_not_a_text_field() {
    let _home = test_home::enter("canvas_widget_focus_is_not_a_text_field");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let mut text = String::new();
    let mut field = None;
    for _ in 0..2 {
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            let response = ui.text_edit_singleline(&mut text);
            response.request_focus();
            field = Some(response.id);
        });
        output.textures_delta.clear();
    }
    assert_eq!(ctx.memory(|m| m.focused()), field);
    assert_eq!(shell.current_focus(&ctx), Focus::TextField);
    shell.canvas_id = field;
    assert_eq!(shell.current_focus(&ctx), Focus::Canvas);
}

#[test]
fn korean_preedit_then_commit_inserts_hangul_through_shell() {
    let _home = test_home::enter("korean_preedit_then_commit_inserts_hangul_through_shell");
    let mut shell = AppShell::new();
    let (id, story) = prime(&mut shell, "a");
    let version = shell.workspace.session().unwrap().generation;
    for text in ["ㅎ", "하", "한"] {
        frame(&mut shell, vec![preedit(text)]);
        assert!(frame_text_input::is_composing());
        assert_eq!(story_text(&shell, id, story), "abc");
    }
    assert_eq!(shell.workspace.session().unwrap().generation, version);
    frame(&mut shell, vec![Event::Ime(egui::ImeEvent::Commit("한".into()))]);
    assert!(!frame_text_input::is_composing());
    assert_eq!(story_text(&shell, id, story), "abc한");
}

#[test]
fn letter_keys_do_not_switch_tool_while_editing_text() {
    let _home = test_home::enter("letter_keys_do_not_switch_tool_while_editing_text");
    let mut control = AppShell::new();
    control.workspace.new_document("c");
    let before = tool(&control);
    frame(&mut control, vec![key(Key::V), key(Key::T), key(Key::P)]);
    let control_switched = tool(&control) != before;
    let mut shell = AppShell::new();
    prime(&mut shell, "a");
    let before = tool(&shell);
    frame(&mut shell, vec![preedit("ㅎ"), key(Key::V), key(Key::T), key(Key::P)]);
    assert_eq!(tool(&shell), before);
    assert!(shell.text_editing());
    assert!(control_switched, "control: letter shortcuts switch tools when not editing");
}

#[test]
fn escape_during_composition_commits_preedit() {
    let _home = test_home::enter("escape_during_composition_commits_preedit");
    let mut shell = AppShell::new();
    let (id, story) = prime(&mut shell, "a");
    frame(&mut shell, vec![preedit("한")]);
    frame(&mut shell, vec![key(Key::Escape)]);
    assert_eq!(story_text(&shell, id, story), "abc한");
    assert!(!frame_text_input::is_composing());
}

#[test]
fn tool_switch_mid_composition_commits_preedit() {
    let _home = test_home::enter("tool_switch_mid_composition_commits_preedit");
    let mut shell = AppShell::new();
    let (id, story) = prime(&mut shell, "a");
    frame(&mut shell, vec![preedit("한")]);
    let other = if tool(&shell) == "tool.move" { "tool.view" } else { "tool.move" };
    shell.pick_tool(other).unwrap();
    shell.finish_ime_on_switch();
    assert_eq!(story_text(&shell, id, story), "abc한");
    assert!(!shell.text_editing());
}

#[test]
fn tab_switch_mid_composition_commits_on_old_document() {
    let _home = test_home::enter("tab_switch_mid_composition_commits_on_old_document");
    let mut shell = AppShell::new();
    let b = shell.workspace.new_document("b");
    let (a, story) = prime(&mut shell, "a");
    frame(&mut shell, vec![preedit("한")]);
    shell.activate_context(b).unwrap();
    assert_eq!(shell.workspace.active(), Some(b));
    assert_eq!(story_text(&shell, a, story), "abc한");
    assert!(!frame_text_input::is_composing());
}

#[test]
fn composing_reports_text_field_focus() {
    let _home = test_home::enter("composing_reports_text_field_focus");
    let mut shell = AppShell::new();
    prime(&mut shell, "a");
    let ctx = egui::Context::default();
    frame(&mut shell, vec![preedit("ㅎ")]);
    assert!(frame_text_input::is_composing());
    assert_eq!(shell.current_focus(&ctx), Focus::TextField);
}

#[test]
fn shell_does_not_ingest_canvas_ime_twice() {
    let _home = test_home::enter("shell_does_not_ingest_canvas_ime_twice");
    let mut shell = AppShell::new();
    let (id, story) = prime(&mut shell, "a");
    let ctx = egui::Context::default();
    ctx.begin_pass(RawInput { events: vec![Event::Ime(egui::ImeEvent::Commit("한".into()))], ..Default::default() });
    shell.focus = shell.current_focus(&ctx);
    shell.ingest_ime(&ctx);
    ctx.end_pass().textures_delta.clear();
    assert_eq!(story_text(&shell, id, story), "abc");
    assert!(!shell.input.is_ime_composing());
}

#[test]
fn close_active_tab_mid_composition_commits_first() {
    let _home = test_home::enter("close_active_tab_mid_composition_commits_first");
    let mut shell = AppShell::new();
    let (id, story) = prime(&mut shell, "a");
    frame(&mut shell, vec![preedit("한")]);
    shell.request_close(id);
    assert!(!frame_text_input::is_composing());
    if let Some(context) = shell.workspace.get(id) {
        assert_eq!(context.session.document.stories.iter().find(|item| item.id.0 == story).unwrap().text, "abc한");
        assert!(context.session.text_edit.is_none());
    }
}

#[test]
fn window_focus_loss_commits_preedit_keeps_editing() {
    let _home = test_home::enter("window_focus_loss_commits_preedit_keeps_editing");
    let mut shell = AppShell::new();
    let (id, story) = prime(&mut shell, "a");
    frame(&mut shell, vec![preedit("한")]);
    let ctx = egui::Context::default();
    ctx.begin_pass(RawInput { events: vec![Event::WindowFocused(false)], ..Default::default() });
    shell.focus = shell.current_focus(&ctx);
    assert!(shell.ingest_ime(&ctx));
    ctx.end_pass().textures_delta.clear();
    assert_eq!(story_text(&shell, id, story), "abc한");
    assert!(shell.text_editing());
}
