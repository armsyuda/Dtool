#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;

#[test]
fn legacy_workspace_holds_no_session_of_its_own() {
    let _home = test_home::enter("legacy_workspace_holds_no_session_of_its_own");
    let mut legacy = dtool_ui::Workspace::default();
    while let Some(id) = legacy.core.active() {
        legacy.core.close_with(id, dtool_core::workspace::CloseAnswer::No).unwrap();
    }
    assert!(legacy.session().is_none());
    assert_eq!(legacy.run_named("ui.menu.query", serde_json::json!({})).unwrap_err().code, "NO_DOCUMENT");
}

#[test]
fn legacy_workspace_acts_on_the_shell_session() {
    let _home = test_home::enter("legacy_workspace_acts_on_the_shell_session");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("shared");
    shell.workspace.activate(id).unwrap();
    let mut legacy = dtool_ui::Workspace::default();
    std::mem::swap(&mut legacy.core, &mut shell.workspace);
    let before = legacy.session().unwrap().generation;
    legacy.run_named("doc.object.create", serde_json::json!({ "kind": "rect", "x": 0.0, "y": 0.0, "w": 10.0, "h": 10.0 })).ok();
    let seen = legacy.session().unwrap().generation;
    std::mem::swap(&mut legacy.core, &mut shell.workspace);
    assert_eq!(shell.workspace.active(), Some(id));
    assert_eq!(shell.workspace.session().unwrap().generation, seen);
    assert!(seen >= before);
}
