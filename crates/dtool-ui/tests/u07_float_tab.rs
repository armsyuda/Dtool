#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::CloseAnswer;
use dtool_ui::shell::AppShell;

#[test]
fn float_keeps_its_context_when_active_tab_changes() {
    let _home = test_home::enter("float_keeps_its_context_when_active_tab_changes");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("a");
    let b = shell.workspace.new_document("b");
    shell.workspace.activate(a).unwrap();
    shell.float_manager.toggle_float(a);
    shell.workspace.get_mut(a).unwrap().view_state.zoom = 2.0;
    shell.workspace.activate(b).unwrap();
    assert!(shell.float_manager.is_floating(a));
    assert!(!shell.float_manager.is_floating(b));
    assert_eq!(shell.workspace.get(a).unwrap().view_state.zoom, 2.0);
    assert_eq!(shell.float_title(a).as_deref(), Some("a"));
    shell.float_manager.toggle_float(a);
    assert!(!shell.float_manager.is_floating(a));
}

#[test]
fn closed_document_leaves_no_float() {
    let _home = test_home::enter("closed_document_leaves_no_float");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("a");
    shell.float_manager.float(a);
    shell.float_canvases.entry(a).or_default();
    shell.workspace.close_with(a, CloseAnswer::No).unwrap();
    shell.retain_floating_documents();
    assert!(!shell.float_manager.is_floating(a));
    assert!(shell.float_canvases.is_empty());
    assert_eq!(shell.float_title(a), None);
}

#[test]
fn floating_frame_runs_headless() {
    let _home = test_home::enter("floating_frame_runs_headless");
    let ctx = egui::Context::default();
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("a");
    shell.float_manager.float(a);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| shell.show_floating_documents(ui.ctx()));
    output.textures_delta.clear();
    assert!(shell.workspace.get(a).is_some());
}
