#[path = "support/test_home.rs"]
mod test_home;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dtool_core::document::Units;
use dtool_core::workspace::{host_channel, HostHandle, HostInbox, HostReply, Scope};
use dtool_core::Command;
use dtool_ui::dialogs::export::{UI_EXPORT_OPEN, UI_EXPORT_SET};
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

struct Live {
    shell: AppShell,
    handle: HostHandle,
    inbox: HostInbox,
    ctx: egui::Context,
}

impl Live {
    fn new(units: Units, width: f64, height: f64) -> Self {
        install_export_host();
        if let Ok(mut dialog) = export_dialog().lock() { dialog.close(); }
        let (handle, inbox) = host_channel(Arc::new(|| {}));
        let mut live = Live { shell: AppShell::new(), handle, inbox, ctx: egui::Context::default() };
        let reply = live.call("doc.new", json!({ "width": width, "height": height, "units": units, "dpi": 300.0, "pages": 1 }));
        assert!(reply.result.is_ok(), "{:?}", reply.result.err());
        let added = live.call("shape.add", json!({ "kind": dtool_core::shape::ShapeKind::Rectangle, "size": [20.0, 10.0], "pos": [5.0, 5.0] }));
        let added = added.result.expect("shape added");
        let id = added.get("id").or_else(|| added.get("object_id")).or_else(|| added.pointer("/ids/0")).and_then(|v| v.as_u64()).unwrap_or_else(|| panic!("shape id in {added}"));
        let filled = live.call("object.update", json!({ "id": id, "fill": dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(200, 30, 40, 255) } }));
        assert!(filled.result.is_ok(), "{:?}", filled.result.err());
        frame(&mut live.shell, &live.ctx, false);
        live
    }

    fn call(&mut self, name: &str, params: Value) -> HostReply {
        mcp(&mut self.shell, &self.handle, &mut self.inbox, name, params)
    }

    fn export_pdf(&mut self, name: &str, options: Value) -> Result<Vec<u8>, String> {
        let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13_b10_pdf_{}_{name}.pdf", std::process::id()));
        let _ = std::fs::remove_file(&path);
        self.shell.open_export_dialog();
        let opened = self.call(UI_EXPORT_OPEN, json!({ "format": "pdf" }));
        assert!(opened.result.is_ok(), "{:?}", opened.result.err());
        let mut params = options;
        params["path"] = json!(path.to_string_lossy());
        params["overwrite"] = json!(true);
        if let Err(error) = self.call(UI_EXPORT_SET, params).result {
            return Err(error.code);
        }
        frame(&mut self.shell, &self.ctx, false);
        frame(&mut self.shell, &self.ctx, true);
        frame(&mut self.shell, &self.ctx, false);
        let error = export_dialog().lock().unwrap().error.clone();
        if let Some(error) = error {
            if let Ok(mut dialog) = export_dialog().lock() { dialog.close(); }
            return Err(error.code().to_string());
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let admitted = dtool_io::import_pdf::admit(&path, dtool_io::import_pdf::ImportFormat::Pdf);
        assert!(admitted.as_ref().is_ok_and(|profile| profile.supported && profile.pages == 1), "{name}: exported PDF parses back: {admitted:?}");
        if !text(&bytes).contains("/OutputIntents") {
            let parsed = dtool_io::import_pdf::import_pdf(&path, &dtool_io::import_pdf::ImportOptions::default());
            assert!(parsed.is_ok(), "{name}: exported PDF imports back: {:?}", parsed.err());
        }
        let _ = std::fs::remove_file(&path);
        Ok(bytes)
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

fn pdf_box(pdf: &str, key: &str) -> Vec<f32> {
    let at = pdf.find(key).unwrap_or_else(|| panic!("{key} missing")) + key.len();
    let rest = &pdf[at..];
    let open = rest.find('[').unwrap();
    let close = rest.find(']').unwrap();
    rest[open + 1..close].split_whitespace().map(|v| v.parse().unwrap()).collect()
}

fn close(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len(), "{a:?} {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 0.05, "{a:?} vs {b:?}");
    }
}

#[test]
fn c13_b10_pdf_export_bleed_boxes_follow_document_units() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _home = test_home::enter("c13_b10_pdf_bleed");
    let mut live = Live::new(Units::Mm, 210.0, 297.0);
    let mm = 72.0 / 25.4;
    let plain = text(&live.export_pdf("trim", json!({})).unwrap());
    close(&pdf_box(&plain, "/MediaBox"), &[0.0, 0.0, 210.0 * mm, 297.0 * mm]);
    let bleed = text(&live.export_pdf("bleed", json!({ "include_bleed": true })).unwrap());
    close(&pdf_box(&bleed, "/MediaBox"), &[0.0, 0.0, 216.0 * mm, 303.0 * mm]);
    close(&pdf_box(&bleed, "/TrimBox"), &[3.0 * mm, 3.0 * mm, 213.0 * mm, 300.0 * mm]);
    close(&pdf_box(&bleed, "/BleedBox"), &[0.0, 0.0, 216.0 * mm, 303.0 * mm]);
    assert_eq!(live.export_pdf("bleed_rect", json!({ "include_bleed": true, "area": { "Rect": [0.0, 0.0, 10.0, 10.0] } })), Err("EXPORT_OPTION_UNAVAILABLE".to_string()));
}

#[test]
fn c13_b10_pdf_export_embeds_icc_output_intent_and_metadata() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _home = test_home::enter("c13_b10_pdf_icc");
    let mut live = Live::new(Units::Px, 200.0, 100.0);
    let plain = text(&live.export_pdf("plain", json!({})).unwrap());
    assert!(!plain.contains("/OutputIntents") && !plain.contains("/ICCBased"), "no ICC unless asked");
    let reply = live.call("doc.metadata.set", json!({ "title": "Poster Title", "author": "Hyoung Wook Park", "keywords": "print, bleed" }));
    assert!(reply.result.is_ok(), "{:?}", reply.result.err());
    assert!(!text(&live.export_pdf("no_meta", json!({})).unwrap()).contains("Poster Title"));
    let pdf = text(&live.export_pdf("icc_meta", json!({ "embed_icc": true, "embed_metadata": true })).unwrap());
    assert!(pdf.contains("/ICCBased"), "ICCBased colour space");
    assert!(pdf.contains("/OutputIntents") && pdf.contains("/GTS_PDFX") && pdf.contains("/DestOutputProfile"), "OutputIntent");
    let app_cmyk = dtool_core::colour::services().read().unwrap().preferences.cmyk.clone();
    assert!(pdf.contains(&format!("/OutputConditionIdentifier ({app_cmyk})")), "app default CMYK profile for an RGB document");
    assert!(pdf.contains("Poster Title") && pdf.contains("Hyoung Wook Park"), "Info/XMP metadata");
    let fogra = text(&live.export_pdf("fogra", json!({ "embed_icc": true, "icc": dtool_core::colour::ProfileStore::FOGRA39L })).unwrap());
    assert!(fogra.contains(&format!("({})", dtool_core::colour::ProfileStore::FOGRA39L)));
    assert_eq!(live.export_pdf("grey", json!({ "embed_icc": true, "icc": dtool_core::colour::ProfileStore::GREY_D50 })), Err("EXPORT_OPTION_UNAVAILABLE".to_string()));
    assert_eq!(live.export_pdf("icc_only", json!({ "icc": dtool_core::colour::ProfileStore::FOGRA39L })), Err("EXPORT_OPTION_UNAVAILABLE".to_string()));
    assert_eq!(live.export_pdf("intent", json!({ "embed_icc": true, "intent": "Saturation" })), Err("EXPORT_OPTION_UNAVAILABLE".to_string()));
}