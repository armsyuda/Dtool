#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;

fn shell() -> AppShell {
    let mut shell = AppShell::new();
    let doc = shell.workspace.new_document("c04 bot02 shell paste");
    shell.workspace.activate(doc).unwrap();
    shell
}

fn frame(shell: &mut AppShell, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| shell.show(ui));
    out.textures_delta.clear();
    out
}

fn count(shell: &mut AppShell) -> usize {
    shell.workspace.session_mut().unwrap().document.objects().len()
}

fn key_v() -> egui::Event {
    egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }
}

#[test]
fn os_paste_makes_one_object_with_or_without_the_key_event() {
    let _home = test_home::enter("c04_bot02_shell_paste");
    let mut shell = shell();
    let ctx = egui::Context::default();
    frame(&mut shell, &ctx, Vec::new());
    let before = count(&mut shell);
    frame(&mut shell, &ctx, vec![egui::Event::Paste("hello OS".into())]);
    assert_eq!(count(&mut shell), before + 1);
    assert_eq!(shell.workspace.session_mut().unwrap().clipboard.get_payload().text.as_deref(), Some("hello OS"));
    frame(&mut shell, &ctx, Vec::new());
    frame(&mut shell, &ctx, vec![egui::Event::ModifiersChanged(egui::Modifiers::COMMAND), egui::Event::Paste("second".into()), key_v()]);
    frame(&mut shell, &ctx, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)]);
    assert_eq!(count(&mut shell), before + 2);
    assert_eq!(shell.workspace.session_mut().unwrap().clipboard.get_payload().text.as_deref(), Some("second"));
}

#[test]
fn copy_event_mirrors_app_clipboard_text_to_the_os() {
    let _home = test_home::enter("c04_bot02_shell_copy");
    let mut shell = shell();
    let ctx = egui::Context::default();
    frame(&mut shell, &ctx, Vec::new());
    shell.workspace.session_mut().unwrap().clipboard.set_text("app text".into());
    let out = frame(&mut shell, &ctx, vec![egui::Event::Copy]);
    let copied = out.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "app text"));
    assert!(copied, "{:?}", out.platform_output.commands);
}

#[test]
fn paste_special_menu_opens_the_dialog_with_formats() {
    let _home = test_home::enter("c04_bot02_shell_special");
    let mut shell = shell();
    let ctx = egui::Context::default();
    shell.workspace.session_mut().unwrap().clipboard.set_text("special".into());
    assert!(shell.open_paste_special());
    let state = shell.paste_special_dialog.as_ref().expect("dialog");
    assert!(!state.formats.is_empty());
    frame(&mut shell, &ctx, Vec::new());
    assert!(shell.paste_special_dialog.is_some());
    let mut empty = AppShell::new();
    assert!(!empty.open_paste_special());
    assert!(empty.paste_special_dialog.is_none());
}