#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::shortcuts::{Focus, Modifiers};
use dtool_core::workspace::{host_channel, Scope, Workspace};
use dtool_core::Command;
use dtool_ui::shortcuts::{install, publish_focus, publish_map, sync_shared, ShortcutMap};
use serde_json::{json, Value};
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

fn bound(list: &Value, command: &str) -> Vec<Value> {
    list.as_array().unwrap().iter().filter(|item| item["command"] == command).cloned().collect()
}

#[test]
fn c16_shortcuts_and_focus_run_on_the_live_session_path() {
    let home = test_home::enter("c16_shortcut_bus");
    let mut workspace = Workspace::default();
    workspace.new_document("c16_bus");
    assert_eq!(live(&mut workspace, "ui.shortcuts.list", json!({})).unwrap_err(), "NOT_AVAILABLE");
    let mut local = ShortcutMap::affinity_default();
    let mut seen = 0;
    assert!(install(local.clone()));
    assert!(!install(local.clone()));
    let list = live(&mut workspace, "ui.shortcuts.list", json!({})).unwrap();
    let total = list.as_array().unwrap().len();
    assert!(total > 50);
    assert_eq!(live(&mut workspace, "ui.focus.query", json!({})).unwrap()["focus"], "canvas");
    publish_focus(Focus::TextField);
    assert_eq!(live(&mut workspace, "ui.focus.query", json!({})).unwrap()["focus"], "text_field");
    assert!(!sync_shared(&mut local, &mut seen));
    let set = live(&mut workspace, "ui.shortcut.set", json!({ "command": "view.zoom.fit", "key": "F9", "modifiers": { "ctrl": true } })).unwrap();
    assert_eq!(set.as_array().unwrap().len(), 1);
    assert_eq!(set[0]["key"], "F9");
    assert!(home.root.join("AppData").join("Roaming").join("DTool").join("shortcuts.json").exists());
    assert!(sync_shared(&mut local, &mut seen));
    assert_eq!(local.dispatch(Focus::Canvas, "F9", Modifiers { ctrl: true, ..Default::default() }).as_deref(), Some("view.zoom.fit"));
    assert!(!sync_shared(&mut local, &mut seen));
    let empty = home.root.join("empty_keymap.json");
    std::fs::write(&empty, r#"{"bindings":{}}"#).unwrap();
    assert_eq!(live(&mut workspace, "ui.shortcuts.import", json!({ "path": empty.to_string_lossy() })).unwrap_err(), "INVALID_PARAMS");
    assert!(!sync_shared(&mut local, &mut seen));
    assert_eq!(bound(&live(&mut workspace, "ui.shortcuts.list", json!({})).unwrap(), "view.zoom.fit").len(), 1);
    assert_eq!(live(&mut workspace, "ui.shortcut.set", json!({})).unwrap_err(), "INVALID_PARAMS");
    let exported = home.root.join("exported_keymap.json");
    live(&mut workspace, "ui.shortcuts.export", json!({ "path": exported.to_string_lossy() })).unwrap();
    assert!(exported.exists());
    live(&mut workspace, "ui.shortcuts.reset", json!({})).unwrap();
    assert!(sync_shared(&mut local, &mut seen));
    assert!(bound(&live(&mut workspace, "ui.shortcuts.list", json!({})).unwrap(), "view.zoom.fit").iter().all(|item| item["key"] != "F9"));
    live(&mut workspace, "ui.shortcuts.import", json!({ "path": exported.to_string_lossy() })).unwrap();
    assert!(sync_shared(&mut local, &mut seen));
    assert_eq!(local.dispatch(Focus::Canvas, "F9", Modifiers { ctrl: true, ..Default::default() }).as_deref(), Some("view.zoom.fit"));
    let mut edited = local.clone();
    edited.set("view.zoom.fit", "F8", Modifiers::default());
    publish_map(&edited);
    let after = bound(&live(&mut workspace, "ui.shortcuts.list", json!({})).unwrap(), "view.zoom.fit");
    assert_eq!(after.len(), 1);
    assert_eq!(after[0]["key"], "F8");
}