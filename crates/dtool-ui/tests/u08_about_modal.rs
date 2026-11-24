#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::about::{version_line, ABOUT_MENU_ID, LICENCES};
use dtool_ui::dialog_chrome::DialogResult;
use dtool_ui::menu_tree::shell_action;
use dtool_ui::shell::AppShell;

fn frame(shell: &mut AppShell, ctx: &egui::Context, events: Vec<egui::Event>) -> DialogResult {
    let mut result = DialogResult::Open;
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        result = shell.show_about(ui.ctx());
    });
    output.textures_delta.clear();
    result
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

fn click(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
    ]
}

#[test]
fn about_is_an_app_action_with_body_content() {
    let _home = test_home::enter("about_is_an_app_action_with_body_content");
    assert_eq!(ABOUT_MENU_ID, "help.about");
    assert!(shell_action(ABOUT_MENU_ID));
    assert!(version_line().contains(env!("CARGO_PKG_VERSION")));
    assert!(LICENCES.contains(&"egui"));
}

#[test]
fn escape_closes_about() {
    let _home = test_home::enter("escape_closes_about");
    let ctx = egui::Context::default();
    let mut shell = AppShell::new();
    shell.open_about();
    assert_eq!(frame(&mut shell, &ctx, Vec::new()), DialogResult::Open);
    assert!(shell.about.open);
    assert_eq!(frame(&mut shell, &ctx, vec![key(egui::Key::Escape)]), DialogResult::Cancel);
    assert!(!shell.about.open);
}

#[test]
fn click_outside_closes_about_after_first_frame() {
    let _home = test_home::enter("click_outside_closes_about_after_first_frame");
    let ctx = egui::Context::default();
    let mut shell = AppShell::new();
    shell.open_about();
    let outside = egui::pos2(1.0, 1.0);
    assert_eq!(frame(&mut shell, &ctx, click(outside)), DialogResult::Open);
    assert!(shell.about.open);
    frame(&mut shell, &ctx, click(outside));
    assert!(!shell.about.open);
}

#[test]
fn app_about_json_unchanged() {
    let _home = test_home::enter("app_about_json_unchanged");
    let mut session = dtool_core::Session::default();
    let about = session.run_named("app.about", serde_json::json!({})).unwrap();
    assert!(about.is_object());
}
