#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::rulers::Rulers;
use dtool_ui::shell::AppShell;

#[test]
fn canvas_rect_insets_only_when_rulers_shown() {
    let _home = test_home::enter("canvas_rect_insets_only_when_rulers_shown");
    let area = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(410.0, 320.0));
    assert_eq!(Rulers::canvas_rect(area, false), area);
    let t = Rulers::thickness();
    let inner = Rulers::canvas_rect(area, true);
    assert_eq!(inner.min, egui::pos2(10.0 + t, 20.0 + t));
    assert_eq!(inner.max, area.max);
}

#[test]
fn appshell_owns_rulers_and_guides_dialog() {
    let _home = test_home::enter("appshell_owns_rulers_and_guides_dialog");
    let mut shell = AppShell::new();
    assert!(!shell.guides_dialog.open);
    shell.guides_dialog.open();
    assert!(shell.guides_dialog.open);
    assert!(shell.rulers.drag.is_none());
    assert_eq!(dtool_ui::guides_dialog::SHOW_GUIDES_MANAGER, "view.guides_manager");
}
