#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;

#[test]
fn equal_generation_tabs_refresh_by_context() {
    let _home = test_home::enter("equal_generation_tabs_refresh_by_context");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("a");
    let b = shell.workspace.new_document("b");
    shell.workspace.activate(a).unwrap();
    shell.reconcile_active();
    let count = shell.refresh_count();
    let generation = shell.workspace.session().unwrap().generation;
    shell.workspace.activate(b).unwrap();
    assert_eq!(generation, shell.workspace.session().unwrap().generation);
    shell.reconcile_active();
    assert_eq!(shell.last_drawn(), Some((b, generation)));
    assert_eq!(shell.refresh_count(), count + 1);
    shell.reconcile_active();
    assert_eq!(shell.refresh_count(), count + 1);
}
