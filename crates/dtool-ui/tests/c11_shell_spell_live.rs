#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::canvas::frame_text_input;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn overlay(shell: &mut AppShell, ctx: &egui::Context) {
    let session = shell.workspace.session_mut().unwrap();
    let view = session.view.clone();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let rect = ui.max_rect();
        frame_text_input::paint_overlay(ui, session, rect, &view);
    });
    out.textures_delta.clear();
}

#[test]
fn c11_live_spelling_underlines_follow_the_edited_story() {
    let _home = test_home::enter("c11_shell_spell_live");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c11 spell live");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.frame.add", json!({ "rect": [20, 20, 400, 300], "text": "the cat" }));
    let object = added["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&shell.workspace.session_mut().unwrap().document, dtool_core::ObjectId(object)).unwrap().0;
    frame(&mut shell, &ctx);
    assert!(frame_text_input::spell_underlines(shell.workspace.session_mut().unwrap()).is_empty(), "no edit, no underlines");

    run(&mut shell, "text.select", json!({ "story": story, "id": object, "range": { "start": 7, "end": 7 } }));
    frame(&mut shell, &ctx);
    overlay(&mut shell, &ctx);
    assert!(frame_text_input::spell_underlines(shell.workspace.session_mut().unwrap()).is_empty(), "correct text has no underlines");

    let typed: Vec<egui::Event> = [" ", "s", "a", "t", "t"].into_iter().map(|s| egui::Event::Text(s.into())).collect();
    assert!(frame_text_input::feed_text_input(shell.workspace.session_mut().unwrap(), &typed));
    assert!(frame_text_input::spell_pending(shell.workspace.session_mut().unwrap()), "typing schedules a settled re-check");
    std::thread::sleep(dtool_tools::dtool_text::spell_live::SETTLE + std::time::Duration::from_millis(50));
    overlay(&mut shell, &ctx);
    let session = shell.workspace.session_mut().unwrap();
    assert_eq!(session.document.stories.iter().find(|s| s.id.0 == story).unwrap().text, "the cat satt");
    let lines = frame_text_input::spell_underlines(session);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!((lines[0].1.start, lines[0].1.end), (8, 12));
    assert_eq!(lines[0].0 .0, object);
    assert!(lines[0].1.x1 > lines[0].1.x0);
    assert!(!frame_text_input::spell_pending(session));
    let version = session.document.version;
    frame(&mut shell, &ctx);
    assert_eq!(shell.workspace.session_mut().unwrap().document.version, version, "underlines never write the document");
}