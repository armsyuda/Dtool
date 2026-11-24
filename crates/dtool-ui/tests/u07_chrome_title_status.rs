#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::menu_tree::{window_document_index, WINDOW_DOCUMENT_PREFIX};
use dtool_ui::shell::AppShell;
use dtool_ui::status_bar::{zoom_label, StatusBar};

#[test]
fn window_title_follows_active_document() {
    let _home = test_home::enter("window_title_follows_active_document");
    let mut shell = AppShell::new();
    assert_eq!(shell.window_title(), dtool_strings::text("app.title"));
    let a = shell.workspace.new_document("alpha");
    let b = shell.workspace.new_document("beta");
    shell.workspace.activate(a).unwrap();
    assert_eq!(shell.window_title(), "alpha");
    shell.workspace.activate(b).unwrap();
    let title = shell.window_title();
    assert!(title.starts_with("beta"));
    assert!(!title.contains('%'));
}

#[test]
fn status_zoom_and_visibility() {
    let _home = test_home::enter("status_zoom_and_visibility");
    assert_eq!(zoom_label(1.0), "100%");
    assert_eq!(zoom_label(2.5), "250%");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("zoom");
    shell.workspace.activate(id).unwrap();
    shell.workspace.get_mut(id).unwrap().view_state.zoom = 2.0;
    shell.sync_status_chrome();
    assert_eq!(shell.status_bar.zoom_text.as_deref(), Some("200%"));
    assert!(shell.status_bar.visible);
    shell.settings.status_bar_visible = false;
    shell.sync_status_chrome();
    assert!(!shell.status_bar.visible);
}

#[test]
fn window_menu_lists_and_activates_documents() {
    let _home = test_home::enter("window_menu_lists_and_activates_documents");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("one");
    let b = shell.workspace.new_document("two");
    shell.workspace.activate(b).unwrap();
    let items = dtool_ui::window_menu::document_items(&shell.workspace, &shell.settings);
    assert!(items[0].1.starts_with("1 ") && items[1].1.starts_with("2 "));
    assert!(!items[0].2 && items[1].2);
    assert_eq!(window_document_index(&format!("{WINDOW_DOCUMENT_PREFIX}0")), Some(0));
    assert!(shell.activate_document_index(0));
    assert_eq!(shell.workspace.active(), Some(a));
    assert!(!shell.activate_document_index(9));
}

#[test]
fn tool_hint_uses_tool_key() {
    let _home = test_home::enter("tool_hint_uses_tool_key");
    let mut bar = StatusBar::new();
    bar.set_tool("move");
    if dtool_strings::text("status.hint.move") != "status.hint.move" {
        assert_eq!(bar.hint_key, "status.hint.move");
    }
}
