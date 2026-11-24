#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::prefs_cmd;
use dtool_protocol::Command;
use dtool_ui::shell::AppShell;
use serde_json::json;

#[test]
fn shell_installs_core_prefs_and_routes_pref_commands_through_prefs_dialog() {
    let _home = test_home::enter("shell_installs_core_prefs_and_routes_pref_commands_through_prefs_dialog");
    let root = _home.root.clone();
    let mut shell = AppShell::new();
    assert!(prefs_cmd::installed());
    assert!(shell.prefs_dialog.prefs_path().starts_with(&root));
    assert!(prefs_cmd::snapshot().unwrap() == shell.prefs);
    assert!(AppShell::is_pref_command("pref.set") && !AppShell::is_pref_command("object.update"));

    let doc = shell.workspace.new_document("u prefs");
    shell.workspace.activate(doc).unwrap();
    shell.run_and_reconcile(Command::with_params("pref.set", json!({ "recent_count": 3 }))).expect("pref.set via shell");
    assert_eq!(shell.prefs.recent_count, 3);
    assert_eq!(prefs_cmd::snapshot().unwrap().recent_count, 3);
    assert!(shell.prefs_dialog.prefs_path().exists());
    assert!(!shell.adopt_core_prefs());

    let (_, result) = shell.workspace.run(dtool_core::Scope::active(), Command::with_params("pref.set", json!({ "recent_count": 5 })));
    result.expect("pref.set via session path");
    assert!(shell.adopt_core_prefs());
    assert_eq!(shell.prefs.recent_count, 5);
    let queried = shell.run_and_reconcile(Command::with_params("pref.query", json!({ "keys": ["recent_count"] }))).expect("pref.query");
    assert!(queried.to_string().contains('5'), "{queried}");

    // Preferences window edit -> core store (no session needed), via prefs_cmd::replace.
    shell.prefs_dialog.open = true;
    shell.prefs.recent_count = 7;
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show_preferences(ui));
    out.textures_delta.clear();
    assert_eq!(prefs_cmd::snapshot().unwrap().recent_count, 7);
    assert!(!shell.push_core_prefs());
    assert!(!shell.adopt_core_prefs());
}