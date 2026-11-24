#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::panels::session_route;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn shell() -> AppShell {
    let mut shell = AppShell::new();
    let doc = shell.workspace.new_document("u06 shell dock");
    shell.workspace.activate(doc).unwrap();
    shell
}

fn mcp(shell: &mut AppShell, name: &str, params: Value) -> Result<Value, String> {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.map(norm).map_err(|error| error.code)
}

fn gui(shell: &mut AppShell, name: &str, params: Value) -> Result<Value, String> {
    shell.run_ui_dock_command(name, &params).expect("dock command").map(norm).map_err(|error| error.code)
}

fn norm(mut value: Value) -> Value {
    fn walk(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, child) in map.iter_mut() {
                    if key == "invisible" {
                        if let Value::Array(items) = child {
                            items.sort_by_key(|item| item.as_u64());
                        }
                    }
                    walk(child);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(walk),
            _ => {}
        }
    }
    walk(&mut value);
    value
}

fn active_tabs(layout: &Value) -> Vec<(String, String, Value)> {
    let mut out = Vec::new();
    for (side, column) in layout["tiles"].as_object().into_iter().flatten() {
        for (id, tile) in column["tiles"]["tiles"].as_object().into_iter().flatten() {
            if let Some(tabs) = tile.pointer("/Container/Tabs").filter(|tabs| tabs["children"].as_array().is_some_and(|children| children.len() > 1)) {
                out.push((side.clone(), id.clone(), tabs["active"].clone()));
            }
        }
    }
    out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    out
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
}

#[test]
fn shell_and_session_route_share_one_dock_state() {
    let _home = test_home::enter("shell_and_session_route_share_one_dock_state");
    let mut shell = shell();
    let ctx = egui::Context::default();
    let before_first = active_tabs(&gui(&mut shell, "ui.layout.get", json!({})).unwrap());
    assert!(!before_first.is_empty());
    frame(&mut shell, &ctx);
    assert_eq!(active_tabs(&gui(&mut shell, "ui.layout.get", json!({})).unwrap()), before_first);
    assert_eq!(active_tabs(&mcp(&mut shell, "ui.layout.get", json!({})).unwrap()), before_first);
    assert!(session_route::ready());
    assert!(session_route::dock(&shell.workspace).is_some_and(|dock| dock.persist_path.is_some()));
    let base = mcp(&mut shell, "ui.layout.get", json!({})).expect("route layout");
    assert_eq!(gui(&mut shell, "ui.layout.get", json!({})).unwrap(), base);

    gui(&mut shell, "ui.panel.float", json!({ "id": "layers" })).expect("shell float");
    let after_gui = gui(&mut shell, "ui.layout.get", json!({})).unwrap();
    assert_ne!(after_gui, base);
    assert_eq!(mcp(&mut shell, "ui.layout.get", json!({})).unwrap(), after_gui);

    mcp(&mut shell, "ui.panel.open", json!({ "id": "stroke" })).expect("route open");
    mcp(&mut shell, "ui.panel.float", json!({ "id": "stroke", "os_window": false })).expect("route float");
    let after_mcp = mcp(&mut shell, "ui.layout.get", json!({})).unwrap();
    assert_ne!(after_mcp, after_gui);
    assert!(shell.pull_dock());
    assert_eq!(gui(&mut shell, "ui.layout.get", json!({})).unwrap(), after_mcp);

    mcp(&mut shell, "ui.panel.float", json!({ "id": "colour", "os_window": false })).expect("route float between frames");
    let floated = mcp(&mut shell, "ui.layout.get", json!({})).unwrap();
    assert_ne!(floated, after_mcp);
    frame(&mut shell, &ctx);
    let gui_after = gui(&mut shell, "ui.layout.get", json!({})).unwrap();
    assert_eq!(gui_after["panels"], floated["panels"]);
    assert_eq!(gui_after["columns"], floated["columns"]);
    assert_eq!(active_tabs(&gui_after), active_tabs(&floated));
    frame(&mut shell, &ctx);
    assert_eq!(active_tabs(&gui(&mut shell, "ui.layout.get", json!({})).unwrap()), active_tabs(&floated));
    assert_eq!(active_tabs(&mcp(&mut shell, "ui.layout.get", json!({})).unwrap()), active_tabs(&floated));
    let shared = mcp(&mut shell, "ui.layout.get", json!({})).unwrap();
    assert_eq!(shared["panels"], gui_after["panels"]);
    assert_eq!(shared["columns"], gui_after["columns"]);
}