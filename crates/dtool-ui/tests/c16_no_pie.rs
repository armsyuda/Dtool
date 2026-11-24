use dtool_core::workspace::{host_channel, Scope, Workspace};
use dtool_core::Command;
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

#[test]
fn c16_no_pie_commands_run_on_session_path() {
    let mut workspace = Workspace::default();
    workspace.new_document("c16_no_pie");
    for (name, params) in [("ui.pie.list", json!({})), ("ui.pie.open", json!({ "menu": "vector_tools" })), ("ui.pie.set", json!({ "menu": {} }))] {
        assert!(live(&mut workspace, name, params).is_err(), "{name}");
    }
}

#[test]
fn c16_no_pie_core_or_design_code() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert!(!root.join("dtool-core/src/commands/pie.rs").exists());
    let components = std::fs::read_to_string(root.join("dtool-design/src/components.rs")).unwrap();
    assert!(!components.contains("draw_pie") && !components.contains("PIE_"));
}

#[test]
fn c16_no_pie_in_catalog_or_ko() {
    let catalog = dtool_protocol::CommandCatalog::new();
    for name in ["ui.pie.open", "ui.pie.list", "ui.pie.set"] {
        assert!(catalog.resolve(name).is_none(), "{name}");
    }
    let ko: Value = serde_json::from_str(include_str!("../../dtool-strings/ko.json")).unwrap();
    let keys: Vec<&String> = ko.as_object().unwrap().keys().filter(|key| key.starts_with("pie.")).collect();
    assert!(keys.is_empty(), "{keys:?}");
}

#[test]
fn c16_no_pie_ui_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!root.join("src/pie_menu.rs").exists());
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert!(!lib.contains("pie_menu") && !lib.contains("draw_pie"));
}