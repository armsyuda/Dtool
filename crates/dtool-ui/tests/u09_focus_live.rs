#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::input::{InputHandler, ShortcutDispatcher};
use dtool_ui::shell::AppShell;
use dtool_ui::shortcuts::{Focus, Modifiers, ShortcutMap, TabAction};
use egui::{Context, Event, ImeEvent, Key, RawInput};
use serde_json::json;

const HIEUT: &str = "\u{314E}";
const HA: &str = "\u{D558}";

fn plain() -> Modifiers {
    Modifiers::default()
}

fn key(key: Key, modifiers: egui::Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn preedit(text: &str) -> Event {
    Event::Ime(ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

fn frame(ctx: &Context, field: bool, events: Vec<Event>, mut extra: impl FnMut(&egui::Context)) -> Focus {
    let mut shell = AppShell::new();
    frame_with(&mut shell, ctx, field, false, events, |_, ctx| extra(ctx))
}

fn frame_with(shell: &mut AppShell, ctx: &Context, field: bool, extra_first: bool, events: Vec<Event>, mut extra: impl FnMut(&mut AppShell, &egui::Context)) -> Focus {
    let mut focus = Focus::Other;
    let mut out = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
        if extra_first {
            extra(shell, ui.ctx());
        }
        let mut value = String::from("10");
        let response = ui.text_edit_singleline(&mut value);
        if field {
            response.request_focus();
        }
        if !extra_first {
            extra(shell, ui.ctx());
        }
        focus = shell.current_focus(ui.ctx());
    });
    out.textures_delta.clear();
    focus
}

fn shell() -> AppShell {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("u09");
    shell.workspace.activate(id).unwrap();
    shell
}

fn start_text_edit(shell: &mut AppShell) {
    let session = shell.workspace.session_mut().unwrap();
    session.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "abc"})).unwrap();
    let story = session.document.stories.last().unwrap().id.0;
    session.run_named("text.select", json!({"story": story, "range": {"start": 3, "end": 3}})).unwrap();
}

fn tab_frame(ctx: &Context, shell: &mut AppShell, field: bool) {
    frame_with(shell, ctx, field, true, vec![key(Key::Tab, egui::Modifiers::NONE)], |shell, ctx| shell.handle_canvas_tab(ctx));
}

#[test]
fn canvas_owns_keyboard_when_no_widget_is_focused() {
    let _home = test_home::enter("canvas_owns_keyboard_when_no_widget_is_focused");
    let ctx = Context::default();
    assert_eq!(frame(&ctx, false, vec![], |_| {}), Focus::Canvas);
    assert_eq!(frame(&ctx, false, vec![key(Key::V, egui::Modifiers::NONE)], |_| {}), Focus::Canvas);
}

#[test]
fn focused_text_field_owns_keyboard_until_escape() {
    let _home = test_home::enter("focused_text_field_owns_keyboard_until_escape");
    let ctx = Context::default();
    frame(&ctx, true, vec![], |_| {});
    assert_eq!(frame(&ctx, false, vec![], |_| {}), Focus::TextField);
    frame(&ctx, false, vec![key(Key::Escape, egui::Modifiers::NONE)], |_| {});
    assert_eq!(frame(&ctx, false, vec![], |_| {}), Focus::Canvas);
}

#[test]
fn tool_shortcuts_fire_on_canvas_only() {
    let _home = test_home::enter("tool_shortcuts_fire_on_canvas_only");
    let map = ShortcutMap::affinity_default();
    assert!(map.dispatch(Focus::Canvas, "v", plain()).is_some());
    assert_eq!(map.dispatch(Focus::TextField, "v", plain()), None);
    assert_eq!(map.dispatch(Focus::Dialog, "v", plain()), None);
    assert!(map.dispatch_all(Focus::TextField, "v", plain()).is_empty());
    let ctx = Context::default();
    frame(&ctx, true, vec![], |_| {});
    let focus = frame(&ctx, false, vec![key(Key::V, egui::Modifiers::NONE)], |_| {});
    assert_eq!(map.dispatch(focus, "v", plain()), None);
}

#[test]
fn ime_composition_blocks_shortcuts_until_commit() {
    let _home = test_home::enter("ime_composition_blocks_shortcuts_until_commit");
    let mut dispatcher = ShortcutDispatcher::new(InputHandler::new());
    assert!(dispatcher.dispatch_shortcut(Focus::Canvas, "v".into(), plain()).is_some());
    dispatcher.handler_mut().ingest_events(&[preedit(HIEUT), preedit(HA)], Focus::Canvas);
    assert!(dispatcher.handler().is_ime_composing());
    assert_eq!(dispatcher.dispatch_shortcut(Focus::Canvas, "v".into(), plain()), None);
    dispatcher.handler_mut().ingest_events(&[Event::Ime(ImeEvent::Commit(HA.into()))], Focus::Canvas);
    assert!(!dispatcher.handler().is_ime_composing());
    assert!(dispatcher.dispatch_shortcut(Focus::Canvas, "v".into(), plain()).is_some());
}

#[test]
fn tab_in_field_or_dialog_moves_to_next_field() {
    let _home = test_home::enter("tab_in_field_or_dialog_moves_to_next_field");
    let map = ShortcutMap::affinity_default();
    assert_eq!(map.tab_action(Focus::TextField, plain()), TabAction::NextField);
    assert_eq!(map.tab_action(Focus::Dialog, Modifiers { shift: true, ..plain() }), TabAction::NextField);
    assert_eq!(map.tab_action(Focus::TextField, Modifiers { ctrl: true, ..plain() }), TabAction::Ignore);
    assert_eq!(map.tab_action(Focus::Menu, plain()), TabAction::Ignore);
    assert_eq!(map.tab_action_for_edit(Focus::Canvas, plain(), true), TabAction::Ignore);
}

#[test]
fn tab_on_canvas_hides_ui() {
    let _home = test_home::enter("tab_on_canvas_hides_ui");
    let ctx = Context::default();
    let mut shell = shell();
    assert!(!shell.ui_hidden());
    tab_frame(&ctx, &mut shell, false);
    assert!(shell.ui_hidden());
}

#[test]
fn second_tab_on_canvas_restores_ui() {
    let _home = test_home::enter("second_tab_on_canvas_restores_ui");
    let ctx = Context::default();
    let mut shell = shell();
    tab_frame(&ctx, &mut shell, false);
    assert_eq!(frame(&ctx, false, vec![], |_| {}), Focus::Canvas);
    tab_frame(&ctx, &mut shell, false);
    assert!(!shell.ui_hidden());
}

#[test]
fn tab_does_not_toggle_ui_while_text_field_focused_or_text_editing() {
    let _home = test_home::enter("tab_does_not_toggle_ui_while_text_field_focused_or_text_editing");
    let ctx = Context::default();
    let mut shell = shell();
    frame(&ctx, true, vec![], |_| {});
    tab_frame(&ctx, &mut shell, true);
    assert!(!shell.ui_hidden());
    let ctx = Context::default();
    start_text_edit(&mut shell);
    assert!(shell.text_editing());
    tab_frame(&ctx, &mut shell, false);
    assert!(!shell.ui_hidden());
}

#[test]
fn open_popup_reports_menu_focus() {
    let _home = test_home::enter("open_popup_reports_menu_focus");
    let ctx = Context::default();
    let id = egui::Id::NULL.with("u09_menu");
    let focus = frame(&ctx, false, vec![], |ctx| egui::Popup::open_id(ctx, id));
    assert!(egui::Popup::is_any_open(&ctx));
    assert_eq!(focus, Focus::Menu);
}

#[test]
fn modal_dialog_reports_dialog_focus() {
    let _home = test_home::enter("modal_dialog_reports_dialog_focus");
    let ctx = Context::default();
    let mut shell = shell();
    shell.open_about();
    frame(&ctx, false, vec![], |ctx| {
        shell.show_about(ctx);
    });
    let focus = frame_with(&mut shell, &ctx, false, false, vec![], |shell, ctx| {
        shell.show_about(ctx);
    });
    assert!(shell.input_blocked(&ctx));
    assert_eq!(focus, Focus::Dialog);
}

#[test]
fn text_editing_reports_text_field_so_letters_do_not_switch_tools() {
    let _home = test_home::enter("text_editing_reports_text_field_so_letters_do_not_switch_tools");
    let ctx = Context::default();
    let mut shell = shell();
    start_text_edit(&mut shell);
    let focus = frame_with(&mut shell, &ctx, false, false, vec![key(Key::V, egui::Modifiers::NONE)], |_, _| {});
    assert_eq!(focus, Focus::TextField);
    assert_eq!(ShortcutMap::affinity_default().dispatch(focus, "v", plain()), None);
}

#[test]
fn tab_on_canvas_toggles_ui_even_when_panels_drew_first() {
    let _home = test_home::enter("tab_on_canvas_toggles_ui_even_when_panels_drew_first");
    let ctx = Context::default();
    let mut shell = shell();
    frame(&ctx, false, vec![key(Key::Tab, egui::Modifiers::NONE)], |ctx| shell.handle_canvas_tab(ctx));
    assert!(shell.ui_hidden());
}