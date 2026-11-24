#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::shortcuts::{Focus, Modifiers};
use dtool_core::{Command, Scope};
use dtool_ui::shell::AppShell;
use dtool_ui::shortcut_commands::{target_for, ShortcutTarget};
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn specials(shell: &mut AppShell, story: u64) -> Vec<(usize, dtool_core::document::Special)> {
    let session = shell.workspace.session_mut().unwrap();
    session.document.stories.iter().find(|s| s.id.0 == story).map(|s| s.specials.clone()).unwrap()
}

const CTRL: Modifiers = Modifiers { ctrl: true, shift: false, alt: false };

#[test]
fn ctrl_backslash_inserts_indent_to_here_at_the_caret_only_while_editing() {
    let _home = test_home::enter("c11_indent_shortcut");
    assert_eq!(target_for("Serif.Interop.Persona.Commands.InsertIndentToHereCommand"), Some(ShortcutTarget::Run { command: "text.special.insert", params: r#"{"what":"indent_to_here"}"# }));
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c11 indent");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "abcd" }));
    let object = added["id"].as_u64().unwrap();
    let story = added["story"].as_u64().unwrap();
    frame(&mut shell, &ctx);
    assert!(shell.handle_shortcut_key(Focus::Canvas, "Oem5", CTRL).is_none(), "no text edit: key not consumed");
    assert!(specials(&mut shell, story).is_empty());
    run(&mut shell, "text.select", json!({ "story": story, "id": object, "range": { "start": 2, "end": 2 } }));
    frame(&mut shell, &ctx);
    assert!(shell.handle_shortcut_key(Focus::Canvas, "Oem5", CTRL).is_some());
    assert_eq!(specials(&mut shell, story), vec![(2, dtool_core::document::Special::IndentToHere)]);
    run(&mut shell, "edit.undo", json!({}));
    assert!(specials(&mut shell, story).is_empty());
}
