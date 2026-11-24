use dtool_ui::dock::{Layout, PanelId};
use dtool_ui::panels::{record_window_panels, register_named_panels, toggle_panel, window_panel, window_panel_checked, PanelRegistry, WINDOW_SWATCHES};

#[test]
fn window_menu_checkmark_follows_swatches_panel() {
    let registry = PanelRegistry::with_existing_bodies();
    let mut layout = Layout::default();
    register_named_panels(&mut layout, &registry);
    let id = window_panel(WINDOW_SWATCHES).unwrap();
    record_window_panels(&layout);
    assert_eq!(window_panel_checked(WINDOW_SWATCHES), layout.is_panel_shown(&PanelId::Swatches));
    let shown = toggle_panel(&mut layout, &id);
    assert_eq!(window_panel_checked(WINDOW_SWATCHES), shown);
    let shown = toggle_panel(&mut layout, &id);
    assert_eq!(window_panel_checked(WINDOW_SWATCHES), shown);
    if !shown {
        toggle_panel(&mut layout, &id);
    }
    assert!(window_panel_checked(WINDOW_SWATCHES));
    layout.set_panel_visible(&id, false);
    record_window_panels(&layout);
    assert!(!window_panel_checked(WINDOW_SWATCHES));
    assert!(!window_panel_checked("window.general.unknown"));
}