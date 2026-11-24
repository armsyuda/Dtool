use dtool_ui::dock::{Layout, PanelId};
use dtool_ui::persona::PersonaLayout;
use serde_json::json;

#[test]
fn u06_old_layout_other_history_maps_to_named() {
    assert_eq!(PanelId::from_value(&json!({"Other": "history"})), Some(PanelId::History));
    assert_eq!(PanelId::from_value(&json!("layers")), Some(PanelId::Layers));
    let value = json!({
        "tiles": {},
        "panels": [
            {"id": {"Other": "history"}, "visible": true, "floated": false, "os_window": false, "tab_group": null},
            {"id": "Layers", "visible": true, "floated": false, "os_window": false, "tab_group": null},
            {"id": {"Other": "history"}, "visible": false, "floated": false, "os_window": false, "tab_group": null}
        ]
    });
    let path = std::env::temp_dir().join("dtool_u06_old_layout.json");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let layout = Layout::load_file(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let ids: Vec<PanelId> = layout.panels.iter().map(|state| state.id.clone()).collect();
    assert_eq!(ids.iter().filter(|id| **id == PanelId::History).count(), 1);
    assert_eq!(ids.iter().filter(|id| **id == PanelId::Layers).count(), 1);
    assert!(!ids.iter().any(|id| matches!(id, PanelId::Other(key) if key == "history")));
}

#[test]
fn u06_old_persona_layout_dedupes() {
    let layout: PersonaLayout = serde_json::from_value(json!({
        "panels": [{"Other": "properties"}, "Properties"],
        "dock": null
    }))
    .unwrap();
    assert_eq!(layout.panels, vec![PanelId::Properties]);
}
