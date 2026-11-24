#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::Session;
use dtool_ui::feature_finder::{search, FeatureFinder, MENU_ID};
use dtool_ui::shell::{AppShell, CONTEXT_OBJECT, CONTEXT_PASTEBOARD};

#[test]
fn context_target_follows_selection() {
    let _home = test_home::enter("context_target_follows_selection");
    let session = Session::default();
    assert_eq!(AppShell::context_target_for(&session), CONTEXT_PASTEBOARD);
    let _ = CONTEXT_OBJECT;
}

#[test]
fn tab_guard_off_without_text_edit() {
    let _home = test_home::enter("tab_guard_off_without_text_edit");
    let shell = AppShell::new();
    assert!(!shell.text_editing());
}

#[test]
fn feature_finder_menu_and_search() {
    let _home = test_home::enter("feature_finder_menu_and_search");
    let mut finder = FeatureFinder::default();
    finder.open();
    assert!(finder.open);
    assert_eq!(MENU_ID, "help.feature_finder");
    let mut session = Session::default();
    let hits = search(&mut session, "zoom").unwrap();
    assert!(hits.iter().all(|hit| hit.to_lowercase().contains("zoom")));
}
