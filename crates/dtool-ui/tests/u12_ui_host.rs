#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::preview::{reply_kind, PREVIEW_KIND, UI_SCREENSHOT_KIND, UI_STATE_KIND};
use dtool_core::workspace::{HostReply, HostRequest, Scope};
use dtool_protocol::Command;
use dtool_ui::shell::AppShell;
use serde_json::json;
use std::sync::mpsc::Receiver;

fn send(shell: &mut AppShell, ctx: &egui::Context, name: &str, params: serde_json::Value) -> Receiver<HostReply> {
    let (reply, rx) = std::sync::mpsc::channel();
    shell.apply_host_request(ctx, HostRequest { scope: Scope::active(), command: Command::with_params(name, params), expected_active_path: None, reply });
    rx
}

fn code(reply: HostReply) -> String {
    reply.result.expect_err("error reply").code
}

#[test]
fn u12_state_query_reports_frame_context_generation_after_action() {
    let _home = test_home::enter("u12_state_query_reports_frame_context_generation_after_action");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("a");
    shell.run_and_reconcile(Command::with_params("ui.tool.set", json!({ "id": "tool.move" }))).ok();
    shell.frame = 7;
    let generation = shell.workspace.session().unwrap().generation;
    let ctx = egui::Context::default();
    let reply = send(&mut shell, &ctx, "ui.state.query", json!({})).try_recv().unwrap();
    assert_eq!(reply.context, Some(id));
    assert_eq!(reply.generation, Some(generation));
    let value = reply.result.unwrap();
    assert_eq!(value["kind"], UI_STATE_KIND);
    assert_eq!(value["frame"], 7);
    assert_eq!(value["context_id"], id.0);
    assert_eq!(value["generation"], generation);
    assert_eq!(value["viewport"]["id"], "root");
    assert_eq!(value["documents"][0]["active"], true);
    assert_eq!(reply_kind(&value), Some(UI_STATE_KIND));
}

#[test]
fn u12_capture_waits_for_next_frame_and_respects_max_px() {
    let _home = test_home::enter("u12_capture_waits_for_next_frame_and_respects_max_px");
    let mut shell = AppShell::new();
    shell.workspace.new_document("a");
    shell.frame = 3;
    let ctx = egui::Context::default();
    let mut rx = None;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        rx = Some(send(&mut shell, ui.ctx(), "ui.screenshot.capture", json!({ "max_px": 100 })));
    });
    output.textures_delta.clear();
    let rx = rx.unwrap();
    assert!(rx.try_recv().is_err(), "no reply before the frame is captured");
    let callback = output.viewport_output.get(&egui::ViewportId::ROOT).into_iter().flat_map(|viewport| viewport.commands.iter()).find_map(|command| match command {
        egui::ViewportCommand::Screenshot(callback) => Some(callback.clone()),
        _ => None,
    }).expect("screenshot command sent to the root viewport");
    callback.complete(std::sync::Arc::new(egui::ColorImage::new([400, 200], vec![egui::Color32::from_rgb(10, 20, 30); 400 * 200])));
    let value = rx.try_recv().unwrap().result.unwrap();
    assert_eq!(value["kind"], UI_SCREENSHOT_KIND);
    assert_eq!(value["frame"], 4);
    assert_eq!(value["mime_type"], "image/png");
    assert_eq!(value["width"], 100);
    assert_eq!(value["height"], 50);
    assert!(value["png_base64"].as_str().is_some_and(|text| text.starts_with("iVBOR")));
}

#[test]
fn u12_capture_oversize_returns_request_too_large() {
    let _home = test_home::enter("u12_capture_oversize_returns_request_too_large");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    assert_eq!(code(send(&mut shell, &ctx, "ui.screenshot.capture", json!({ "max_px": 4096 })).try_recv().unwrap()), "REQUEST_TOO_LARGE");
}

#[test]
fn u12_capture_unknown_viewport_is_not_found() {
    let _home = test_home::enter("u12_capture_unknown_viewport_is_not_found");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    assert_eq!(code(send(&mut shell, &ctx, "ui.screenshot.capture", json!({ "viewport": "float.9" })).try_recv().unwrap()), "NOT_FOUND");
}

#[test]
fn u12_capture_other_context_is_context_not_active() {
    let _home = test_home::enter("u12_capture_other_context_is_context_not_active");
    let mut shell = AppShell::new();
    let a = shell.workspace.new_document("a");
    let b = shell.workspace.new_document("b");
    shell.workspace.activate(b).unwrap();
    let ctx = egui::Context::default();
    assert_eq!(code(send(&mut shell, &ctx, "ui.screenshot.capture", json!({ "context": a.0 })).try_recv().unwrap()), "CONTEXT_NOT_ACTIVE");
    assert_eq!(shell.workspace.active(), Some(b));
}

#[test]
fn u12_screenshot_kind_differs_from_document_preview() {
    let _home = test_home::enter("u12_screenshot_kind_differs_from_document_preview");
    assert_ne!(UI_SCREENSHOT_KIND, PREVIEW_KIND);
    let png = dtool_ui::ui_host::screenshot_reply(&dtool_ui::ui_host::UiFrame { frame: 1, context_id: None, generation: 0, viewport: [1, 1] }, &egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]), 10.0).unwrap();
    assert_eq!(reply_kind(&png), Some(UI_SCREENSHOT_KIND));
}