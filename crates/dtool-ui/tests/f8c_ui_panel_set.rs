use dtool_ui::dock::{Layout, PanelId};
use dtool_ui::panels::{register_named_panels, run_command, PanelRegistry};
use serde_json::json;

#[test]
fn ui_panel_set_swatches_goes_to_palette_view_state() {
    let registry = PanelRegistry::with_existing_bodies();
    let mut layout = Layout::default();
    register_named_panels(&mut layout, &registry);
    let params = json!({ "panel": "swatches", "visible": true, "set": { "scope": "document", "palette": "Fixture", "view": "grid", "sort": "hue", "search": "red" } });
    let reply = run_command(&mut layout, &registry, "ui.panel.set", &params).unwrap().unwrap();
    assert_eq!(reply["panel"], "swatches");
    assert_eq!(reply["state"]["view"], "grid");
    assert_eq!(reply["state"]["sort"], "hue");
    let state = dtool_core::palette::ui_state();
    assert_eq!(state.palette, "Fixture");
    assert_eq!(state.search, "red");
    assert!(layout.is_panel_shown(&PanelId::Swatches) || layout.panel_state(&PanelId::Swatches).is_some_and(|panel| panel.visible));
}