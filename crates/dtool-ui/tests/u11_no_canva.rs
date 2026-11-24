use dtool_core::document::{Document, Units};
use dtool_core::workspace::{host_channel, Scope, Workspace};
use dtool_core::Command;
use dtool_core::native_io::{native_body, parse_native};
use dtool_core::prefs::Prefs;
use dtool_export::raster::{options_schema, parse_format};
use dtool_protocol::catalog::CommandCatalog;
use dtool_ui::dock::Layout;
use dtool_ui::panel_ids::panel_registry;
use dtool_ui::panels::{run_ui_dock_command, PanelRegistry};
use dtool_ui::persona::{install, persona_catalog, PersonaEntry, PersonaStore};
use dtool_ui::presets::{default_presets, PresetStore, WorkspacePersona};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

fn mentions_canva(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.match_indices("canva").any(|(at, _)| !lower[at + 5..].starts_with('s')) || text.contains("캔바")
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("u11_no_canva_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn u11_no_canva_matcher_ignores_canvas() {
    assert!(mentions_canva("window.canva") && mentions_canva("Canva AI") && mentions_canva("캔바 AI") && mentions_canva("CanvaAi"));
    assert!(!mentions_canva("document.setup.resize.canvas") && !mentions_canva("Canvas"));
}

#[test]
fn u11_no_canva_in_registries_catalog_prefs_icons_or_exports() {
    for persona in WorkspacePersona::ALL {
        assert!(!mentions_canva(persona.key()) && !mentions_canva(&format!("{persona:?}")), "{persona:?}");
    }
    assert!(WorkspacePersona::from_key("canva_ai").is_none());
    for preset in default_presets() {
        assert!(!mentions_canva(&preset.name), "{}", preset.name);
    }
    for entry in panel_registry(&[]).iter().chain(PanelRegistry::with_existing_bodies().entries().iter()) {
        assert!(!mentions_canva(&entry.id.key()), "{}", entry.id.key());
    }
    for def in persona_catalog() {
        assert!(!mentions_canva(&def.entry.key()) && !mentions_canva(&def.name_key) && !mentions_canva(&def.icon), "{}", def.entry.key());
    }
    assert!(PersonaEntry::parse("canva_ai").is_err());
    for spec in CommandCatalog::new().all() {
        assert!(!mentions_canva(&spec.name), "{}", spec.name);
    }
    let prefs = serde_json::to_value(Prefs::default()).unwrap();
    assert!(!mentions_canva(&prefs.to_string()));
    for (id, _) in dtool_design::icons::SVG_ICONS {
        assert!(!mentions_canva(id), "{id}");
    }
    for schema in options_schema() {
        assert!(!mentions_canva(&schema.format) && !mentions_canva(&schema.extension));
    }
    assert!(parse_format("canva").is_err());
    for persona in dtool_ui::menu_tree::PERSONAS {
        let menu = dtool_ui::menu_tree::menu_tree_json(persona);
        assert!(!mentions_canva(&menu.to_string()), "{persona}");
    }
    let mut layout = Layout::default();
    let registry = PanelRegistry::with_existing_bodies();
    let mut store = PresetStore::default();
    for preset in default_presets() {
        store.save(&preset.name, &preset.layout);
    }
    for name in ["ui.panel.list", "ui.preset.list", "ui.layout.get"] {
        let value = run_ui_dock_command(&mut layout, &registry, &mut store, name, &json!({}), None).unwrap().unwrap();
        assert!(!mentions_canva(&value.to_string()), "{name}");
    }
}

#[test]
fn u11_no_canva_in_ko_keys_or_menu_rows() {
    let ko: Value = serde_json::from_str(include_str!("../../dtool-strings/ko.json")).unwrap();
    for (key, value) in ko.as_object().unwrap() {
        assert!(!mentions_canva(key) && !mentions_canva(value.as_str().unwrap_or_default()), "{key}");
    }
    for key in dtool_ui::menu_tree::b11_ui_keys() {
        assert!(!mentions_canva(&key), "{key}");
    }
    for (number, line) in include_str!("../src/menu_tree.rs").lines().enumerate() {
        assert!(!mentions_canva(line), "menu_tree.rs:{}", number + 1);
    }
}

#[test]
fn u11_no_canva_old_prefs_file_with_removed_keys_still_loads() {
    let dir = scratch("prefs");
    let path = dir.join("prefs.json");
    let mut old = serde_json::to_value(Prefs::default()).unwrap();
    old["mcp_canva_ai"] = Value::Bool(true);
    old["script_canva_ai"] = Value::Bool(true);
    old["mcp_enabled"] = Value::Bool(false);
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let mut loaded = Prefs::load(&path);
    assert!(!loaded.mcp_enabled);
    let query = loaded.run("pref.query", &json!({}), &path).unwrap();
    assert!(!mentions_canva(&query.to_string()));
    assert_eq!(loaded.run("pref.set", &json!({ "mcp_canva_ai": true }), &path).unwrap_err().code, "INVALID_PARAMS");
    loaded.run("pref.set", &json!({ "mcp_enabled": true }), &path).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(!mentions_canva(&saved));
    assert!(Prefs::load(&path).mcp_enabled);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn u11_no_canva_old_preset_store_with_removed_workspace_still_loads() {
    let dir = scratch("presets");
    let path = dir.join("presets.json");
    let mut store = PresetStore::default();
    store.save("mine", &Layout::default());
    store.choose_workspace(WorkspacePersona::Pixel, &mut Layout::default());
    let mut old = serde_json::to_value(&store).unwrap();
    old["workspace"] = json!("CanvaAi");
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let loaded = PresetStore::load_persisted(&path);
    assert_eq!(loaded.workspace(), WorkspacePersona::default());
    assert_eq!(loaded.list(), vec!["mine".to_string()]);
    old["workspace"] = json!("Pixel");
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(PresetStore::load_persisted(&path).workspace(), WorkspacePersona::Pixel);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn u11_no_canva_old_document_with_removed_merge_source_still_opens() {
    let doc = Document::new([600.0, 400.0], Units::Px, 72.0, 1);
    let mut raw: Value = serde_json::from_str(&native_body(&doc, &Value::Null).unwrap()).unwrap();
    let source = |kind: Value, name: &str| json!({
        "name": name, "kind": kind, "format": "Json", "fields": ["name"], "records": [["a"]], "enabled": true,
        "include_headers": true, "numbers_as_text": false, "case_sensitive": false, "records_range": "All",
        "pages": "All", "rules": [], "scripts": [], "fetched_hash": ""
    });
    raw["merge_sources"] = json!([
        source(json!({ "CanvaDesign": { "url": "https://example.invalid/design" } }), "legacy"),
        source(json!({ "Url": { "url": "https://example.invalid/data.json" } }), "kept"),
    ]);
    let (opened, _) = parse_native(&raw.to_string()).unwrap();
    assert_eq!(opened.merge_sources.len(), 1);
    assert_eq!(opened.merge_sources[0].name, "kept");
    raw["merge_sources"][1]["name"] = json!(7);
    assert!(parse_native(&raw.to_string()).is_err());
}
fn live(workspace: &mut Workspace, name: &str, params: Value) -> Result<Value, String> {
    let (handle, inbox) = host_channel(std::sync::Arc::new(|| {}));
    inbox.publish(workspace);
    let command = Command { name: name.into(), params };
    let worker = std::thread::spawn(move || handle.request(Scope::active(), command, None, Duration::from_secs(10)));
    while !worker.is_finished() {
        inbox.drain(workspace, 32);
        std::thread::sleep(Duration::from_millis(1));
    }
    worker.join().unwrap().result.map_err(|error| error.code)
}

#[test]
fn u11_no_canva_live_persona_set_refuses_canva_and_keeps_state() {
    install(PersonaStore { dir: scratch("persona") }).unwrap();
    let mut workspace = Workspace::default();
    workspace.new_document("u11_no_canva_persona");
    live(&mut workspace, "ui.persona.set", json!({ "id": "pixel" })).unwrap();
    let before = live(&mut workspace, "ui.persona.query", json!({})).unwrap();
    for id in ["canva_ai", "canva", "CanvaAi"] {
        assert_eq!(live(&mut workspace, "ui.persona.set", json!({ "id": id })).unwrap_err(), "INVALID_PARAMS", "{id}");
        let after = live(&mut workspace, "ui.persona.query", json!({})).unwrap();
        assert_eq!((after["id"].clone(), after["revision"].clone()), (before["id"].clone(), json!(before["revision"])));
    }
    assert_eq!(before["id"], "pixel");
    let list = live(&mut workspace, "ui.persona.list", json!({})).unwrap();
    assert!(!list.as_array().unwrap().is_empty());
    assert!(!mentions_canva(&list.to_string()));
}
