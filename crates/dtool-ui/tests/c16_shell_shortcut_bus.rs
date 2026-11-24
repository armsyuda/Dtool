#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::shortcuts::{Focus, Modifiers};
use dtool_core::{Command, Scope};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Result<Value, String> {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.map_err(|error| error.code)
}

#[test]
fn c16_shell_mounts_the_shortcut_bus() {
    let _home = test_home::enter("c16_shell_shortcut_bus");
    let mut shell = AppShell::new();
    let doc = shell.workspace.new_document("c16 shell bus");
    shell.workspace.activate(doc).unwrap();
    let list = run(&mut shell, "ui.shortcuts.list", json!({})).expect("ui.shortcuts.list is available once the shell is up");
    assert!(list.as_array().unwrap().len() > 50);

    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
    assert_eq!(run(&mut shell, "ui.focus.query", json!({})).unwrap()["focus"], "canvas");

    run(&mut shell, "ui.shortcut.set", json!({ "command": "view.zoom.fit", "key": "F9", "modifiers": { "ctrl": true } })).expect("ui.shortcut.set");
    let ctrl = Modifiers { ctrl: true, ..Default::default() };
    assert_ne!(shell.shortcuts.dispatch(Focus::Canvas, "F9", ctrl).as_deref(), Some("view.zoom.fit"));
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
    assert_eq!(shell.shortcuts.dispatch(Focus::Canvas, "F9", ctrl).as_deref(), Some("view.zoom.fit"), "MCP edit reaches the shell map after one frame");
}
