#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::document::Document;
use dtool_core::workspace::Workspace;
use dtool_ui::settings::UiSettings;
use dtool_ui::shell::AppShell;
use dtool_ui::status_bar::{zoom_label, StatusBar};
use dtool_ui::window_menu::{
    activate_document, document_items, is_status_bar_visible, set_status_bar_visible, window_title,
    window_title_for,
};

#[test]
fn window_title_is_name_plus_star_when_dirty_without_zoom() {
    let _home = test_home::enter("window_title_is_name_plus_star_when_dirty_without_zoom");
    let clean = window_title("Poster", false);
    assert_eq!(clean, "Poster");
    assert!(!clean.contains('%'));
    let dirty = window_title("Poster", true);
    assert_eq!(dirty, "Poster*");
    assert!(!dirty.contains('%'));
    let untitled = window_title("", true);
    assert!(untitled.ends_with('*'));
    assert!(!untitled.contains('%'));
    assert!(!untitled.contains('@'));
}

#[test]
fn hint_parts_parse_bold_markup() {
    let _home = test_home::enter("hint_parts_parse_bold_markup");
    let parts = StatusBar::hint_parts("<b>A</b>b");
    assert_eq!(parts, vec![("A".to_string(), true), ("b".to_string(), false)]);
    assert_eq!(StatusBar::hint_parts("plain"), vec![("plain".to_string(), false)]);
    assert!(StatusBar::hint_parts("").is_empty());
}

#[test]
fn set_tool_move_selects_move_hint() {
    let _home = test_home::enter("set_tool_move_selects_move_hint");
    let mut bar = StatusBar::new();
    bar.set_tool("move");
    if dtool_strings::text("status.hint.move") != "status.hint.move" {
        assert_eq!(bar.hint_key, "status.hint.move");
    }
}

#[test]
fn zoom_lives_on_the_status_bar_not_the_title() {
    let _home = test_home::enter("zoom_lives_on_the_status_bar_not_the_title");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("Doc");
    shell.workspace.activate(id).unwrap();
    shell.workspace.get_mut(id).unwrap().view_state.zoom = 1.06;
    shell.sync_status_chrome();
    assert_eq!(shell.status_bar.zoom_text.as_deref(), Some(zoom_label(1.06).as_str()));
    assert!(zoom_label(1.06).contains("106"));
    assert!(!shell.window_title().contains("106"));
    assert!(!window_title("Doc", true).contains("106"));
}

#[test]
fn shell_title_and_window_menu_title_agree() {
    let _home = test_home::enter("shell_title_and_window_menu_title_agree");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("alpha");
    shell.workspace.activate(a).unwrap();
    assert_eq!(shell.window_title(), window_title_for(&shell.workspace));
}

#[test]
fn document_items_number_active_and_dirty_star() {
    let _home = test_home::enter("document_items_number_active_and_dirty_star");
    let mut ws = Workspace::default();
    let a = ws.insert_context("One", None, Document::default(), 0);
    let b = ws.insert_context("Two", None, Document::default(), 0);
    ws.activate(b).unwrap();
    let settings = UiSettings::default();
    let items = document_items(&ws, &settings);
    assert_eq!(items.len(), 2);
    assert!(items[0].1.starts_with("1 "));
    assert!(items[1].1.starts_with("2 "));
    assert!(!items[0].2);
    assert!(items[1].2);
    assert_eq!(items[0].0, a);
    assert_eq!(items[1].0, b);
    let title = window_title_for(&ws);
    assert_eq!(title, "Two");
    activate_document(&mut ws, a);
    assert_eq!(window_title_for(&ws), "One");
}

#[test]
fn status_visibility_helpers_follow_settings() {
    let _home = test_home::enter("status_visibility_helpers_follow_settings");
    let mut s = UiSettings::default();
    assert!(is_status_bar_visible(&s));
    set_status_bar_visible(&mut s, false);
    assert!(!is_status_bar_visible(&s));
}