use dtool_core::{Command, Scope, Workspace};
use dtool_protocol::CommandCatalog;
use dtool_ui::console::ConsolePanel;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const SCREEN: [f32; 2] = [480.0, 320.0];

fn frame(ctx: &egui::Context, panel: &mut ConsolePanel, workspace: &mut Workspace, events: Vec<egui::Event>) {
    let raw = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SCREEN[0], SCREEN[1]))), ..Default::default() };
    let mut output = ctx.run_ui(raw, |ui| panel.show(ui, workspace));
    output.textures_delta.clear();
}

fn click(ctx: &egui::Context, panel: &mut ConsolePanel, workspace: &mut Workspace, pos: egui::Pos2) {
    let press = egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() };
    let release = egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() };
    frame(ctx, panel, workspace, vec![egui::Event::PointerMoved(pos), press]);
    frame(ctx, panel, workspace, vec![release]);
}

fn console_run(workspace: &mut Workspace, name: &str, params: &str) -> ConsolePanel {
    let ctx = egui::Context::default();
    let mut panel = ConsolePanel { command_name: name.to_string(), params_json: params.to_string(), result_json: None, error_message: None };
    frame(&ctx, &mut panel, workspace, Vec::new());
    for y in (2..SCREEN[1] as u32).step_by(6) {
        for x in (4..SCREEN[0] as u32).step_by(12) {
            click(&ctx, &mut panel, workspace, egui::pos2(x as f32, y as f32));
            if panel.result_json.is_some() || panel.error_message.is_some() {
                assert_eq!(panel.command_name, name);
                assert_eq!(panel.params_json, params);
                return panel;
            }
        }
    }
    panic!("console run button was not reached");
}

fn workspace() -> Workspace {
    let mut workspace = Workspace::default();
    workspace.new_document("b15");
    workspace
}

#[test]
fn console_route_runs_app_about_like_the_direct_workspace() {
    let mut workspace = workspace();
    let direct = workspace.run(Scope::active(), Command::with_params("app.about", json!({}))).1.unwrap();
    let panel = console_run(&mut workspace, "app.about", "{}");
    assert!(panel.error_message.is_none());
    let console: Value = serde_json::from_str(panel.result_json.as_deref().unwrap()).unwrap();
    assert_eq!(console, direct);
}

#[test]
fn console_route_reports_invalid_params_like_the_direct_workspace() {
    let mut workspace = workspace();
    let direct = workspace.run(Scope::active(), Command::with_params("app.about", json!({ "invalid": true }))).1.unwrap_err();
    assert_eq!(direct.code, "INVALID_PARAMS");
    let panel = console_run(&mut workspace, "app.about", r#"{"invalid":true}"#);
    assert!(panel.error_message.as_deref().unwrap().starts_with("INVALID_PARAMS"));
    let console: Value = serde_json::from_str(panel.result_json.as_deref().unwrap()).unwrap();
    assert_eq!(console["code"], "INVALID_PARAMS");
}

#[test]
fn console_route_rejects_names_outside_the_catalog() {
    let mut workspace = workspace();
    let name = "b15.not_a_command";
    assert!(!CommandCatalog::new().names().iter().any(|known| known == name));
    let panel = console_run(&mut workspace, name, "{}");
    assert!(panel.error_message.as_deref().unwrap().starts_with("UNKNOWN_COMMAND"));
}

#[test]
fn console_workspace_catalog_is_the_protocol_catalog_used_by_stdio() {
    let workspace = workspace();
    let console_names = workspace.session().unwrap().catalog.names();
    assert!(console_names.iter().any(|name| name == "app.about"));
    assert_eq!(console_names.iter().collect::<BTreeSet<_>>().len(), console_names.len());
    assert_eq!(console_names, CommandCatalog::new().names());
}