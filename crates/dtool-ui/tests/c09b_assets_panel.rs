use dtool_ui::panels::assets;
use assets::{parse_library, AssetsPanel};
use dtool_core::object::{Object, ObjectId};
use dtool_core::workspace::{Scope, SharedServices, Workspace};
use dtool_protocol::Command;
use serde_json::json;
use std::sync::Arc;

#[test]
fn assets_panel_adds_inserts_deletes_and_persists_in_profile() {
    let profile = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c09b_assets_panel_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&profile);
    std::fs::create_dir_all(&profile).unwrap();
    unsafe { std::env::set_var("APPDATA", &profile) };
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let doc = ws.new_document("a");
    let id = ws.get_mut(doc).unwrap().session.document.create_object(Object { name: "Star".into(), pos: [5.0, 5.0], size: [20.0, 20.0], ..Object::default() });
    ws.get_mut(doc).unwrap().session.selection.ids = vec![id];
    let mut panel = AssetsPanel::default();
    panel.new_category = "Mine".into();
    assert!(panel.add_category(&mut ws), "{:?}", panel.last_error);
    panel.new_group = "Stars".into();
    assert!(panel.add_group(&mut ws));
    let asset = panel.add_selection(&mut ws).expect("asset added");
    let (categories, rows) = panel.library(&mut ws);
    assert_eq!(categories[0].name, "Mine");
    assert_eq!(categories[0].groups, vec!["Stars".to_string()]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Star");
    assert!(profile.join("DTool").join("assets").join("library.json").is_file());

    let second = ws.new_document("b");
    let mut fresh = AssetsPanel::default();
    let (_, rows) = fresh.library(&mut ws);
    assert_eq!(rows[0].id, asset);
    let placed = fresh.insert(&mut ws, asset, Some([50.0, 60.0]));
    assert_eq!(placed.len(), 1);
    let object = ws.get(second).unwrap().session.document.get_object(ObjectId(placed[0])).unwrap().clone();
    assert_eq!(object.pos, [50.0, 60.0]);
    assert_eq!(object.name, "Star");
    ws.run(Scope::of(second), Command::new("edit.undo")).1.unwrap();
    assert!(ws.get(second).unwrap().session.document.objects.is_empty());

    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| fresh.show(ui, &mut ws));
    out.textures_delta.clear();
    assert_eq!(fresh.category.as_deref(), Some("Mine"));
    let panel_id = dtool_ui::panels::window_panel(dtool_ui::panels::WINDOW_ASSETS).expect("window action mapped");
    assert!(dtool_ui::panels::WINDOW_PANELS.contains(&"window.general.assets"));
    let mut registry = dtool_ui::panels::PanelRegistry::new();
    assert!(registry.has_body(&panel_id));
    let mut mounted = false;
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| mounted = dtool_ui::panels::workspace_panel(ui, &panel_id, &mut registry, &mut ws));
    out.textures_delta.clear();
    assert!(mounted);
    assert_eq!(registry.assets.category.as_deref(), Some("Mine"));

    assert!(fresh.delete(&mut ws, asset));
    let (_, rows) = fresh.library(&mut ws);
    assert!(rows.is_empty());
    assert!(!fresh.delete(&mut ws, asset));
    assert!(fresh.last_error.is_some());
    let parsed = parse_library(&json!({ "categories": [{ "name": "X", "groups": ["g"] }], "assets": [{ "id": 3, "name": "n", "category": "X", "group": "g" }] }));
    assert_eq!(parsed.1[0].id, 3);
    assert_eq!(assets::ASSETS_PANEL_KEY, "assets");
    let _ = std::fs::remove_dir_all(&profile);
}