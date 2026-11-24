use dtool_core::workspace::{host_channel, Scope, Workspace};
use dtool_core::Command;
use dtool_ui::dock::{DockSide, Layout, PanelId};
use dtool_ui::panels::{run_ui_dock_command, PanelRegistry};
use dtool_ui::presets::PresetStore;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

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

fn live_workspace() -> Workspace {
    assert!(dtool_ui::panels::session_route::install());
    let mut workspace = Workspace::default();
    workspace.new_document("u06_accept");
    workspace
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("u06_accept_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Dock {
    layout: Layout,
    registry: PanelRegistry,
    store: PresetStore,
    path: Option<PathBuf>,
}

impl Dock {
    fn new(path: Option<PathBuf>) -> Self {
        Dock { layout: Layout::default(), registry: PanelRegistry::with_existing_bodies(), store: PresetStore::default(), path }
    }

    fn run(&mut self, name: &str, params: Value) -> Result<Value, String> {
        run_ui_dock_command(&mut self.layout, &self.registry, &mut self.store, name, &params, self.path.as_deref())
            .unwrap_or_else(|| panic!("{name} not routed"))
            .map_err(|error| error.code)
    }
}

#[test]
fn u06_accept_live_swatches_panel_set_runs_on_the_live_document() {
    let mut workspace = live_workspace();
    let reply = live(&mut workspace, "ui.panel.set", json!({ "panel": "swatches", "set": { "view": "list", "sort": "name" } })).unwrap();
    assert_eq!(reply["panel"], "swatches");
    assert_eq!(reply["state"]["view"], "list");
    assert_eq!(reply["state"]["sort"], "name");
    assert_eq!(live(&mut workspace, "ui.panel.set", json!({ "panel": "swatches", "set": { "view": "cards" } })).unwrap_err(), "INVALID_PARAMS");
}

#[test]
fn u06_accept_live_layout_get_set_round_trip() {
    let mut workspace = live_workspace();
    let before = live(&mut workspace, "ui.layout.get", json!({})).unwrap();
    live(&mut workspace, "ui.panel.float", json!({ "id": "layers" })).unwrap();
    let floated = live(&mut workspace, "ui.layout.get", json!({})).unwrap();
    assert_ne!(floated, before);
    assert_eq!(live(&mut workspace, "ui.layout.set", json!({ "layout": before })).unwrap(), before);
    assert_eq!(live(&mut workspace, "ui.layout.get", json!({})).unwrap(), before);
}

#[test]
fn u06_accept_live_panel_open_float_state() {
    let mut workspace = live_workspace();
    let list = live(&mut workspace, "ui.panel.list", json!({})).unwrap();
    assert!(list["panels"].as_array().unwrap().iter().any(|panel| panel["id"] == "stroke"));
    assert_eq!(live(&mut workspace, "ui.panel.open", json!({ "id": "stroke" })).unwrap()["shown"], true);
    let floated = live(&mut workspace, "ui.panel.float", json!({ "id": "stroke", "os_window": false })).unwrap();
    assert_eq!((floated["floated"].clone(), floated["os_window"].clone()), (json!(true), json!(false)));
    assert_eq!(live(&mut workspace, "ui.panel.state", json!({ "id": "stroke" })).unwrap(), floated);
    assert_eq!(live(&mut workspace, "ui.panel.open", json!({ "id": "no_such_panel" })).unwrap_err(), "NOT_FOUND");
}

#[test]
fn u06_accept_live_preset_save_load_delete() {
    let mut workspace = live_workspace();
    let base = live(&mut workspace, "ui.layout.get", json!({})).unwrap();
    assert!(live(&mut workspace, "ui.preset.save", json!({ "name": "accept" })).unwrap()["presets"].as_array().unwrap().iter().any(|name| name == "accept"));
    live(&mut workspace, "ui.panel.float", json!({ "id": "layers" })).unwrap();
    assert_eq!(live(&mut workspace, "ui.preset.load", json!({ "name": "accept" })).unwrap(), base);
    live(&mut workspace, "ui.preset.delete", json!({ "name": "accept" })).unwrap();
    assert_eq!(live(&mut workspace, "ui.preset.load", json!({ "name": "accept" })).unwrap_err(), "NOT_FOUND");
}

#[test]
fn u06_accept_dock_layout_get_set_round_trip() {
    let mut dock = Dock::new(None);
    let before = dock.run("ui.layout.get", json!({})).unwrap();
    dock.run("ui.panel.float", json!({ "id": "layers", "os_window": true })).unwrap();
    let floated = dock.run("ui.layout.get", json!({})).unwrap();
    assert_ne!(floated, before);
    assert_eq!(dock.run("ui.layout.set", json!({ "layout": before.clone() })).unwrap(), before);
    assert_eq!(dock.run("ui.layout.get", json!({})).unwrap(), before);
    assert_eq!(dock.run("ui.layout.set", json!({ "layout": 7 })).unwrap_err(), "INVALID_PARAMS");
    assert_eq!(dock.run("ui.layout.set", json!({})).unwrap_err(), "INVALID_PARAMS");
    assert_eq!(dock.run("ui.layout.get", json!({})).unwrap(), before);
}

#[test]
fn u06_accept_dock_panel_open_float_set_state() {
    let mut dock = Dock::new(None);
    let list = dock.run("ui.panel.list", json!({})).unwrap();
    let ids: Vec<String> = list["panels"].as_array().unwrap().iter().map(|panel| panel["id"].as_str().unwrap().to_string()).collect();
    assert!(ids.iter().any(|id| id == "stroke") && ids.iter().any(|id| id == "layers"));
    let opened = dock.run("ui.panel.open", json!({ "id": "stroke" })).unwrap();
    assert_eq!((opened["visible"].clone(), opened["shown"].clone()), (json!(true), json!(true)));
    let floated = dock.run("ui.panel.float", json!({ "id": "stroke", "os_window": true })).unwrap();
    assert_eq!((floated["floated"].clone(), floated["os_window"].clone()), (json!(true), json!(true)));
    assert_eq!(dock.run("ui.panel.state", json!({ "id": "stroke" })).unwrap(), floated);
    assert_eq!(dock.run("ui.panel.query", json!({ "id": "stroke" })).unwrap(), floated);
    let docked = dock.run("ui.panel.set", json!({ "id": "stroke", "floated": false, "side": "left" })).unwrap();
    assert_eq!((docked["floated"].clone(), docked["side"].clone()), (json!(false), json!("left")));
    assert_eq!(dock.layout.side_of(&PanelId::Stroke), Some(DockSide::Left));
    assert_eq!(dock.run("ui.panel.set", json!({ "id": "stroke", "side": "nowhere" })).unwrap_err(), "INVALID_PARAMS");
    assert_eq!(dock.run("ui.panel.float", json!({ "id": "stroke", "os_window": "yes" })).unwrap_err(), "INVALID_PARAMS");
    assert_eq!(dock.run("ui.panel.open", json!({})).unwrap_err(), "INVALID_PARAMS");
}

#[test]
fn u06_accept_dock_unknown_panel_ids_are_refused_without_new_state() {
    let mut dock = Dock::new(None);
    let before = dock.run("ui.layout.get", json!({})).unwrap();
    for name in ["ui.panel.open", "ui.panel.float", "ui.panel.state", "ui.panel.query"] {
        assert_eq!(dock.run(name, json!({ "id": "no_such_panel" })).unwrap_err(), "NOT_FOUND", "{name}");
    }
    assert_eq!(dock.run("ui.panel.set", json!({ "id": "no_such_panel", "visible": true })).unwrap_err(), "NOT_FOUND");
    assert_eq!(dock.run("ui.layout.get", json!({})).unwrap(), before);
    assert!(dock.layout.panel_state(&PanelId::from_key("no_such_panel")).is_none());
}

#[test]
fn u06_accept_dock_preset_save_load_delete_persist() {
    let dir = scratch("preset");
    let path = dir.join("presets.json");
    let mut dock = Dock::new(Some(path.clone()));
    let base = dock.run("ui.layout.get", json!({})).unwrap();
    let saved = dock.run("ui.preset.save", json!({ "name": " accept " })).unwrap();
    assert_eq!(saved["name"], "accept");
    assert!(path.exists());
    assert!(PresetStore::load_persisted(&path).list().contains(&"accept".to_string()));
    dock.run("ui.panel.float", json!({ "id": "layers" })).unwrap();
    assert_ne!(dock.run("ui.layout.get", json!({})).unwrap(), base);
    assert_eq!(dock.run("ui.preset.load", json!({ "name": "accept" })).unwrap(), base);
    assert!(dock.run("ui.preset.list", json!({})).unwrap()["presets"].as_array().unwrap().iter().any(|name| name == "accept"));
    dock.run("ui.preset.delete", json!({ "name": "accept" })).unwrap();
    assert!(!PresetStore::load_persisted(&path).list().contains(&"accept".to_string()));
    assert_eq!(dock.run("ui.preset.load", json!({ "name": "accept" })).unwrap_err(), "NOT_FOUND");
    assert_eq!(dock.run("ui.preset.delete", json!({ "name": "accept" })).unwrap_err(), "NOT_FOUND");
    assert_eq!(dock.run("ui.preset.save", json!({ "name": "  " })).unwrap_err(), "INVALID_PARAMS");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn u06_accept_dock_failed_preset_commands_do_not_persist() {
    let dir = scratch("nopersist");
    let path = dir.join("presets.json");
    let mut dock = Dock::new(Some(path.clone()));
    assert!(dock.run("ui.preset.save", json!({})).is_err());
    assert!(dock.run("ui.preset.delete", json!({ "name": "missing" })).is_err());
    assert!(!path.exists());
    assert!(run_ui_dock_command(&mut dock.layout, &dock.registry, &mut dock.store, "ui.layout.nope", &json!({}), Some(&path)).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}