use dtool_ui::dock::PanelId;
use dtool_ui::panel_ids;
use serde_json::json;

#[test]
fn u06_panel_ids_no_named_duplicates() {
    for id in panel_ids::remaining_panel_ids() {
        assert!(!id.is_named());
        assert_eq!(panel_ids::canonical(id.clone()), id);
    }
    let ids = panel_ids::all_panel_ids();
    let mut keys: Vec<&str> = ids.iter().map(|id| id.key()).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), ids.len());
}

#[test]
fn u06_old_other_ids_map_to_named() {
    assert_eq!(PanelId::from_value(&json!({"Other": "history"})), Some(PanelId::History));
    assert_eq!(PanelId::from_value(&json!({"Other": "layers"})), Some(PanelId::Layers));
    assert_eq!(PanelId::from_value(&json!("properties")), Some(PanelId::Properties));
}
