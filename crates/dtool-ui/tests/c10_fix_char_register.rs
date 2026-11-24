use dtool_ui::dock::PanelId;
use dtool_ui::panels::{window_panel, PanelRegistry, WINDOW_CHARACTER, WINDOW_PANELS};

#[test]
fn character_panel_is_registered_for_window_menu() {
    assert_eq!(window_panel(WINDOW_CHARACTER), Some(PanelId::Character));
    assert!(WINDOW_PANELS.contains(&WINDOW_CHARACTER));
    assert!(PanelRegistry::new().has_body(&PanelId::Character));
    for key in ["char.title", "char.family", "char.size", "char.mixed", "char.no_story"] {
        assert!(dtool_strings::has(key), "{key}");
    }
}