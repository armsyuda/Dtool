#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::shortcuts::Focus;
use dtool_ui::shell::AppShell;
use egui::{Context, Event, Key, RawInput};
use serde_json::json;

fn tab() -> Event {
    Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

fn shell() -> AppShell {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("u09");
    shell.workspace.activate(id).unwrap();
    shell
}

fn run(ctx: &Context, shell: &mut AppShell, events: Vec<Event>, tab_first: bool, mut extra: impl FnMut(&egui::Context)) -> Focus {
    let mut focus = Focus::Other;
    let mut out = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
        if tab_first {
            shell.handle_canvas_tab(ui.ctx());
        }
        let mut value = String::from("10");
        ui.text_edit_singleline(&mut value);
        if !tab_first {
            shell.handle_canvas_tab(ui.ctx());
        }
        extra(ui.ctx());
        focus = shell.current_focus(ui.ctx());
    });
    out.textures_delta.clear();
    focus
}

#[test]
fn tab_on_canvas_toggles_ui_even_when_panels_drew_first() {
    let _home = test_home::enter("tab_on_canvas_toggles_ui_even_when_panels_drew_first");
    let ctx = Context::default();
    let mut shell = shell();
    run(&ctx, &mut shell, vec![tab()], false, |_| {});
    assert!(shell.ui_hidden());
    assert_eq!(ctx.memory(|m| m.focused()), None);
}

#[test]
fn second_tab_on_canvas_restores_ui() {
    let _home = test_home::enter("second_tab_on_canvas_restores_ui");
    let ctx = Context::default();
    let mut shell = shell();
    run(&ctx, &mut shell, vec![tab()], true, |_| {});
    assert!(shell.ui_hidden());
    assert_eq!(run(&ctx, &mut shell, vec![], true, |_| {}), Focus::Canvas);
    run(&ctx, &mut shell, vec![tab()], true, |_| {});
    assert!(!shell.ui_hidden());
}

#[test]
fn tab_in_focused_field_moves_focus_not_ui() {
    let _home = test_home::enter("tab_in_focused_field_moves_focus_not_ui");
    let ctx = Context::default();
    let mut shell = shell();
    let mut out = ctx.run_ui(RawInput::default(), |ui| {
        let mut value = String::from("10");
        ui.text_edit_singleline(&mut value).request_focus();
        shell.handle_canvas_tab(ui.ctx());
    });
    out.textures_delta.clear();
    run(&ctx, &mut shell, vec![tab()], false, |_| {});
    assert!(!shell.ui_hidden());
}

#[test]
fn open_popup_reports_menu_focus() {
    let _home = test_home::enter("open_popup_reports_menu_focus");
    let ctx = Context::default();
    let mut shell = shell();
    let id = egui::Id::NULL.with("u09_menu");
    let focus = run(&ctx, &mut shell, vec![], true, |ctx| egui::Popup::open_id(ctx, id));
    assert_eq!(focus, Focus::Menu);
}

#[test]
fn modal_dialog_reports_dialog_focus() {
    let _home = test_home::enter("modal_dialog_reports_dialog_focus");
    let ctx = Context::default();
    let mut shell = shell();
    shell.open_about();
    for _ in 0..2 {
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            shell.show_about(ui.ctx());
        });
        out.textures_delta.clear();
    }
    assert_eq!(shell.current_focus(&ctx), Focus::Dialog);
}

#[test]
fn text_editing_reports_text_field_so_letters_do_not_switch_tools() {
    let _home = test_home::enter("text_editing_reports_text_field_so_letters_do_not_switch_tools");
    let ctx = Context::default();
    let mut shell = shell();
    let session = shell.workspace.session_mut().unwrap();
    session.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "abc"})).unwrap();
    let story = session.document.stories.last().unwrap().id.0;
    session.run_named("text.select", json!({"story": story, "range": {"start": 3, "end": 3}})).unwrap();
    let before = shell.workspace.session().unwrap().active_tool.clone();
    let v = Event::Key { key: Key::V, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    let mut out = ctx.run_ui(RawInput { events: vec![v], ..Default::default() }, |ui| {
        assert_eq!(shell.current_focus(ui.ctx()), Focus::TextField);
        shell.handle_shortcuts(ui.ctx());
    });
    out.textures_delta.clear();
    assert_eq!(shell.workspace.session().unwrap().active_tool, before);
}