use dtool_ui::dock::{Layout, PanelId};
use dtool_ui::panels::{self, PanelRegistry};
use dtool_ui::presets::PresetStore;
use serde_json::json;

#[test]
fn u06_preset_store_persist_and_load_restores_tree() {
    let mut layout = Layout::default();
    layout.set_panel_visible(&PanelId::Properties, false);
    let mut store = PresetStore::default();
    assert!(store.save("mine", &layout));
    let dir = std::env::temp_dir().join(format!("dtool_u06_presets_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("presets.json");
    store.persist(&path).unwrap();
    assert!(path.exists());
    assert!(!path.with_extension("json.tmp").exists());
    let loaded = PresetStore::load_persisted(&path);
    let mut restored = Layout::default();
    assert!(loaded.apply("mine", &mut restored));
    let original_vis = layout.panel_state(&PanelId::Properties).unwrap().visible;
    let restored_vis = restored.panel_state(&PanelId::Properties).unwrap().visible;
    assert_eq!(original_vis, restored_vis);
    assert!(!restored_vis);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn u06_ui_panel_list_is_implemented() {
    let mut layout = Layout::default();
    let registry = PanelRegistry::with_existing_bodies();
    let reply = panels::run_command(&mut layout, &registry, "ui.panel.list", &json!({}))
        .expect("implemented")
        .expect("ok");
    let rows = reply["panels"].as_array().expect("panels array");
    assert!(!rows.is_empty());
}

#[test]
fn u06_header_drag_outside_dock_floats_panel() {
    let mut layout = Layout::default();
    let id = PanelId::Layers;
    assert_eq!(layout.panel_state(&id).map(|state| state.floated), Some(false));
    let dock = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 200.0));
    assert!(!layout.on_header_drag_released(&id, egui::pos2(40.0, 50.0), dock));
    assert_eq!(layout.panel_state(&id).map(|state| state.floated), Some(false));
    assert!(layout.on_header_drag_released(&id, egui::pos2(250.0, 50.0), dock));
    assert_eq!(layout.panel_state(&id).map(|state| state.floated), Some(true));
}

#[test]
fn u06_run_ui_dock_command_preset_save_persists() {
    let mut layout = Layout::default();
    let registry = PanelRegistry::with_existing_bodies();
    let mut store = PresetStore::default();
    let dir = std::env::temp_dir().join(format!("dtool_u06_cmd_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("presets.json");
    let reply = panels::run_ui_dock_command(
        &mut layout,
        &registry,
        &mut store,
        "ui.preset.save",
        &json!({"name": "alpha"}),
        Some(&path),
    )
    .expect("implemented")
    .expect("ok");
    assert_eq!(reply["name"], "alpha");
    let loaded = PresetStore::load_persisted(&path);
    assert!(loaded.contains("alpha"));
    let _ = std::fs::remove_dir_all(&dir);
}

