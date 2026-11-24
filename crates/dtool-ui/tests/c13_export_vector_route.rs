#[path = "support/test_home.rs"]
mod test_home;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dtool_core::document::Units;
use dtool_core::workspace::{host_channel, HostHandle, HostInbox, HostReply, Scope};
use dtool_core::Command;
use dtool_ui::dialogs::export::{UI_EXPORT_OPEN, UI_EXPORT_QUERY, UI_EXPORT_SET};
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
fn file_export_sends_pdf_svg_and_eps_to_the_vector_writers() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let home = test_home::enter("c13_export_vector_route");
    install_export_host();
    if let Ok(mut dialog) = export_dialog().lock() { dialog.close(); }
    let (handle, mut inbox) = host_channel(Arc::new(|| {}));
    let mut shell = AppShell::new();
    let reply = mcp(&mut shell, &handle, &mut inbox, "doc.new", json!({ "width": 200.0, "height": 100.0, "units": Units::Px, "dpi": 72.0, "pages": 1 }));
    assert!(reply.result.is_ok(), "{:?}", reply.result.err());
    let added = mcp(&mut shell, &handle, &mut inbox, "shape.add", json!({ "kind": dtool_core::shape::ShapeKind::Rectangle, "size": [40.0, 20.0], "pos": [10.0, 10.0] }));
    assert!(added.result.is_ok(), "{:?}", added.result.err());
    let ctx = egui::Context::default();
    frame(&mut shell, &ctx, false);
    let dir = std::env::temp_dir().join(format!("dtool_c13_vector_route_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (format, magic) in [("pdf", "%PDF"), ("svg", "<svg"), ("eps", "%!PS-Adobe")] {
        let path = dir.join(format!("route.{format}"));
        let _ = std::fs::remove_file(&path);
        shell.open_export_dialog();
        let opened = mcp(&mut shell, &handle, &mut inbox, UI_EXPORT_OPEN, json!({ "format": format }));
        assert!(opened.result.is_ok(), "{format}: {:?}", opened.result.err());
        let set = mcp(&mut shell, &handle, &mut inbox, UI_EXPORT_SET, json!({ "path": path.to_string_lossy(), "overwrite": true }));
        assert!(set.result.is_ok(), "{format}: {:?}", set.result.err());
        let query = mcp(&mut shell, &handle, &mut inbox, UI_EXPORT_QUERY, json!({})).result.unwrap();
        assert_eq!(query["preview"]["format"], format);
        assert!(query["preview"]["width"].as_u64().unwrap_or(0) > 0, "{query}");
        assert!(export_dialog().lock().unwrap().is_open());
        frame(&mut shell, &ctx, false);
        frame(&mut shell, &ctx, true);
        frame(&mut shell, &ctx, false);
        let dialog = export_dialog().lock().unwrap();
        assert!(!dialog.is_open(), "{format}: {:?}", dialog.error);
        let receipt = dialog.last_receipt.clone().expect("receipt");
        drop(dialog);
        assert_eq!(receipt.path, path);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(receipt.bytes, bytes.len() as u64);
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_string();
        assert!(head.contains(magic), "{format}: {head}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    drop(home);
}