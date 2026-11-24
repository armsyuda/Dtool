use dtool_core::workspace::Workspace;
use dtool_core::Command;
use dtool_ui::dock::PanelId;
use dtool_ui::headless::run_on_workspace;
use dtool_ui::panels::session_route;
use serde_json::{json, Value};

fn workspace() -> Workspace {
    let mut workspace = Workspace::default();
    workspace.new_document("u06_session_route");
    workspace
}

fn run(workspace: &mut Workspace, name: &str, params: Value) -> Result<Value, String> {
    run_on_workspace(workspace, Command { name: name.into(), params }).map_err(|error| error.code)
}

#[test]
fn headless_run_installs_the_route_and_answers_layout_get() {
    let mut ws = workspace();
    let layout = run(&mut ws, "ui.layout.get", json!({})).expect("ui.layout.get");
    assert!(session_route::ready());
    assert!(layout.is_object());
    assert!(session_route::dock(&ws).is_some());
}

#[test]
fn each_workspace_keeps_its_own_dock_state() {
    let mut first = workspace();
    let mut second = workspace();
    let base = run(&mut second, "ui.layout.get", json!({})).unwrap();
    run(&mut first, "ui.panel.float", json!({ "id": "layers" })).unwrap();
    assert_ne!(run(&mut first, "ui.layout.get", json!({})).unwrap(), base);
    assert_eq!(run(&mut second, "ui.layout.get", json!({})).unwrap(), base);
    run(&mut first, "ui.preset.save", json!({ "name": "only_first" })).unwrap();
    assert_eq!(run(&mut second, "ui.preset.load", json!({ "name": "only_first" })).unwrap_err(), "NOT_FOUND");
}

#[test]
fn refused_dock_commands_leave_layout_and_document_untouched() {
    let mut ws = workspace();
    let base = run(&mut ws, "ui.layout.get", json!({})).unwrap();
    let generation = ws.session().unwrap().generation;
    assert_eq!(run(&mut ws, "ui.panel.open", json!({ "id": "no_such_panel" })).unwrap_err(), "NOT_FOUND");
    assert_eq!(run(&mut ws, "ui.layout.set", json!({ "layout": 7 })).unwrap_err(), "INVALID_PARAMS");
    assert_eq!(run(&mut ws, "ui.preset.save", json!({ "name": " " })).unwrap_err(), "INVALID_PARAMS");
    assert_eq!(run(&mut ws, "ui.layout.get", json!({})).unwrap(), base);
    assert_eq!(ws.session().unwrap().generation, generation);
    assert!(session_route::dock(&ws).unwrap().layout.panel_state(&PanelId::from_key("no_such_panel")).is_none());
    assert!(session_route::dock(&ws).unwrap().persist_path.is_none());
}

#[test]
fn dock_commands_do_not_touch_the_document_generation() {
    let mut ws = workspace();
    let generation = ws.session().unwrap().generation;
    run(&mut ws, "ui.panel.open", json!({ "id": "stroke" })).unwrap();
    run(&mut ws, "ui.panel.float", json!({ "id": "stroke", "os_window": false })).unwrap();
    run(&mut ws, "ui.preset.save", json!({ "name": "g" })).unwrap();
    run(&mut ws, "ui.preset.load", json!({ "name": "g" })).unwrap();
    assert_eq!(ws.session().unwrap().generation, generation);
}

#[test]
fn swatches_panel_set_still_runs_on_the_document_session() {
    let mut ws = workspace();
    let reply = run(&mut ws, "ui.panel.set", json!({ "panel": "swatches", "set": { "view": "list" } })).unwrap();
    assert_eq!(reply["panel"], "swatches");
    assert_eq!(reply["state"]["view"], "list");
    assert!(!session_route::is_session_routed(&Command { name: "ui.panel.set".into(), params: json!({ "panel": "swatches", "set": { "view": "list" } }) }));
    assert!(session_route::is_session_routed(&Command { name: "ui.panel.set".into(), params: json!({ "id": "stroke", "side": "left" }) }));
}

#[test]
fn ui_host_commands_stay_no_window_and_non_dock_commands_are_not_routed() {
    let mut ws = workspace();
    assert_eq!(run(&mut ws, "ui.state.query", json!({})).unwrap_err(), "NO_WINDOW");
    assert!(!session_route::is_session_routed(&Command { name: "ui.menu.query".into(), params: json!({}) }));
    assert!(!session_route::is_session_routed(&Command { name: "node.update".into(), params: json!({}) }));
}