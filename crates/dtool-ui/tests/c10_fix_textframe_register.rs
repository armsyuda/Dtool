use dtool_ui::dock::PanelId;
use dtool_ui::panels::{window_panel, PanelRegistry, TEXT_FRAME_PANEL_KEY, WINDOW_PANELS, WINDOW_TEXT_FRAME};

#[test]
fn text_frame_panel_is_registered() {
    let id = PanelId::Other(TEXT_FRAME_PANEL_KEY.into());
    assert_eq!(window_panel(WINDOW_TEXT_FRAME), Some(id.clone()));
    assert!(WINDOW_PANELS.contains(&WINDOW_TEXT_FRAME));
    assert!(PanelRegistry::new().has_body(&id));
    assert!(dtool_ui::panel_ids::all_panel_ids().contains(&id));
    for key in ["panel.text_frame", "frame_panel.title", "frame_panel.no_frame"] {
        assert!(dtool_strings::has(key), "{key}");
    }
}
