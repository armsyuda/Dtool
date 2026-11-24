#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use serde_json::json;

#[test]
fn export_action_refuses_non_owner_context() {
    let _home = test_home::enter("export_action_refuses_non_owner_context");
    let mut shell = dtool_ui::shell::AppShell::new();
    assert!(dtool_core::commands::ui::export_action_registered());
    let a = shell.workspace.new_document("a");
    let b = shell.workspace.new_document("b");
    shell.workspace.activate(a).unwrap();
    let (_, opened) = shell.workspace.run(Scope::of(a), Command::with_params("ui.export.open", json!({ "format": "png" })));
    opened.unwrap();
    let (_, foreign) = shell.workspace.run(Scope::of(b), Command::new("ui.export.query"));
    assert_eq!(foreign.unwrap_err().code, "CONTEXT_NOT_FOUND");
    let (_, own) = shell.workspace.run(Scope::of(a), Command::new("ui.export.query"));
    own.unwrap();
    assert_eq!(dtool_ui::shell::export_dialog().lock().unwrap().owner(), Some(a));
}