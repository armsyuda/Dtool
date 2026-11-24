#[path = "support/test_home.rs"]
mod test_home;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dtool_core::document::Units;
use dtool_export::raster::ExportOptions;
use dtool_export::ExportFormat;
use dtool_core::workspace::{host_channel, HostHandle, HostInbox, HostReply, Scope};
use dtool_core::Command;
use dtool_ui::dialogs::export::UI_EXPORT_OPEN;
use dtool_ui::shell::{export_dialog, install_export_host, AppShell};
use serde_json::{json, Value};

static SERIAL: Mutex<()> = Mutex::new(());

fn mcp(shell: &mut AppShell, handle: &HostHandle, inbox: &mut HostInbox, name: &str, params: Value) -> HostReply {
    let handle = handle.clone();
    let command = Command::with_params(name, params);
    let worker = std::thread::spawn(move || handle.request(Scope::active(), command, None, Duration::from_secs(20)));
    while !worker.is_finished() {
        inbox.drain(&mut shell.workspace, 32);
        std::thread::yield_now();
    }
    worker.join().unwrap()
}

fn frame(shell: &mut AppShell, ctx: &egui::Context, enter: bool) {
    let mut input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    if enter {
        input.events.push(egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
    }
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

#[test]
fn c13_8_h1_saved_preset_options_apply_when_the_export_dialog_opens() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _home = test_home::enter("c13_8_h1_open");
    let store = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13_8_h1_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&store);
    dtool_core::commands::export_presets::set_store_path(Some(store.clone()));
    install_export_host();
    if let Ok(mut dialog) = export_dialog().lock() { dialog.close(); }
    let (handle, mut inbox) = host_channel(Arc::new(|| {}));
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let made = mcp(&mut shell, &handle, &mut inbox, "doc.new", json!({ "width": 200.0, "height": 100.0, "units": Units::Px, "dpi": 300.0, "pages": 1 }));
    assert!(made.result.is_ok(), "{:?}", made.result.err());
    frame(&mut shell, &ctx, false);
    let mut saved = ExportOptions::for_format(ExportFormat::Png);
    saved.dpi = Some(150.0);
    saved.include_bleed = true;
    saved.icc = Some(dtool_core::colour::ProfileStore::SRGB.to_string());
    saved.embed_icc = true;
    saved.embed_metadata = true;
    let mut options = serde_json::to_value(&saved).unwrap();
    options["future_key"] = json!({ "kept": true });
    let session = shell.workspace.session_mut().expect("session");
    let created = dtool_core::commands::export_presets::run(session, &Command::with_params("export.options_preset.create", json!({ "name": "Print PNG", "options": options }))).expect("preset command").expect("preset created");
    dtool_core::commands::export_presets::run(session, &Command::with_params("export.options_preset.load", json!({ "preset_id": created["preset_id"].clone() }))).expect("preset command").expect("preset loaded");
    frame(&mut shell, &ctx, false);
    let opened = mcp(&mut shell, &handle, &mut inbox, UI_EXPORT_OPEN, json!({ "format": "png" }));
    assert!(opened.result.is_ok(), "{:?}", opened.result.err());
    let applied = export_dialog().lock().unwrap().options.clone();
    assert_eq!(applied, saved, "saved preset applied on open, include_bleed/icc round-trip");
    if let Ok(mut dialog) = export_dialog().lock() { dialog.close(); }
    let other = mcp(&mut shell, &handle, &mut inbox, UI_EXPORT_OPEN, json!({ "format": "jpeg" }));
    assert!(other.result.is_ok(), "{:?}", other.result.err());
    assert_eq!(export_dialog().lock().unwrap().options, ExportOptions::for_format(ExportFormat::Jpeg), "a preset for another format is not forced onto this one");
    if let Ok(mut dialog) = export_dialog().lock() { dialog.close(); }
    dtool_core::commands::export_presets::set_store_path(None);
    let _ = std::fs::remove_file(&store);
}
