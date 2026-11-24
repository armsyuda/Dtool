use dtool_ui::dock::{Layout, PanelId};
use dtool_ui::panels::{register_named_panels, toggle_panel, window_panel, PanelRegistry, WINDOW_SWATCHES};

#[test]
fn window_menu_toggles_registered_swatches_panel() {
    let registry = PanelRegistry::with_existing_bodies();
    assert!(registry.has_body(&PanelId::Swatches));
    let mut layout = Layout::default();
    register_named_panels(&mut layout, &registry);
    let id = window_panel(WINDOW_SWATCHES).unwrap();
    assert_eq!(id, PanelId::Swatches);
    assert!(window_panel("window.general.unknown").is_none());
    let before = layout.is_panel_shown(&id);
    assert_eq!(toggle_panel(&mut layout, &id), !before);
    assert_eq!(layout.is_panel_shown(&id), !before);
    toggle_panel(&mut layout, &id);
    assert_eq!(layout.is_panel_shown(&id), before);
}