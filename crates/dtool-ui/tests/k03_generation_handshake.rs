#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::Command;
use dtool_ui::shell::AppShell;
use serde_json::json;

#[test]
fn mutating_command_refreshes_once_with_real_snapshot() {
    let _home = test_home::enter("mutating_command_refreshes_once_with_real_snapshot");
    let mut shell = AppShell::new();
    shell.workspace.new_document("k03");
    shell.reconcile_active();
    let before = shell.refresh_count();
    shell.run_and_reconcile(Command::with_params("table.create", json!({"rows":2,"cols":2,"insert":true}))).unwrap();
    let generation = shell.workspace.session().unwrap().generation;
    assert_eq!(shell.last_drawn_generation(), generation);
    assert_eq!(shell.refresh_count(), before + 1);
    assert!(shell.last_snapshot().get("snapshot").is_some());
    assert_eq!(shell.last_snapshot()["doc_version"], json!(generation));
}

#[test]
fn unchanged_generation_does_not_refresh() {
    let _home = test_home::enter("unchanged_generation_does_not_refresh");
    let mut shell = AppShell::new();
    shell.workspace.new_document("k03");
    shell.reconcile_active();
    let count = shell.refresh_count();
    shell.reconcile_active();
    shell.reconcile_active();
    assert_eq!(shell.refresh_count(), count);
}
