#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, Object};
use dtool_export::{ExportArea, ExportContext, ExportFormat};
use dtool_ui::dialogs::export::ExportDialog;
use serde_json::{json, Value};
use std::sync::Mutex;
static SERIAL: Mutex<()> = Mutex::new(());

fn workspace(pages: u32) -> Workspace {
    let mut ws = Workspace::default();
    ws.new_document("areas");
    ws.session_mut().unwrap().document = Document::new([32.0, 24.0], Units::Px, 72.0, pages);
    dtool_ui::shell::install_export_host();
    *dtool_ui::shell::export_dialog().lock().unwrap() = ExportDialog::new();
    ws
}
fn call(ws: &mut Workspace, name: &str, params: Value) -> Result<Value, dtool_core::CommandError> {
    ws.run(Scope::active(), Command::with_params(name, params)).1
}

#[test]
fn all_pages_job_writes_every_requested_page_and_lists_real_paths() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let home = test_home::enter("c13_all_pages");
    let mut ws = workspace(3);
    let path = home.root.join("pages.png");
    let reply = call(&mut ws, "export.job.run", json!({"items": [{"path": path, "format": "png", "area": "AllPages", "pages": [2, 3]}], "overwrite": "replace"})).unwrap();
    let written = reply["written"].as_array().unwrap();
    assert_eq!(written.len(), 2, "{reply}");
    assert!(!path.exists(), "the unsuffixed path is not reported as a written file");
    for item in written {
        let file = std::path::Path::new(item["path"].as_str().unwrap());
        assert!(file.exists());
        assert_eq!(dtool_export::raster::decode_file(file, ExportFormat::Png).unwrap().dimensions(), (32, 24));
        assert_eq!(item["bytes"].as_u64().unwrap(), std::fs::metadata(file).unwrap().len());
    }
}

#[test]
fn invalid_page_request_creates_no_file_and_preserves_job_queue() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let home = test_home::enter("c13_bad_pages");
    let mut ws = workspace(2);
    let before = call(&mut ws, "export.job.list", json!({})).unwrap();
    let result = call(&mut ws, "export.job.run", json!({"items": [{"path": home.root.join("bad.png"), "format": "png", "area": "AllPages", "pages": [1, 99]}]}));
    assert!(result.is_err(), "{result:?}");
    assert_eq!(call(&mut ws, "export.job.list", json!({})).unwrap(), before);
    assert!(!home.root.join("bad.png").exists());
    assert!(!home.root.join("bad-01.png").exists());
}

#[test]
fn selection_only_excludes_overlapping_unselected_objects() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let home = test_home::enter("c13_selection");
    let mut ws = workspace(1);
    let doc = &mut ws.session_mut().unwrap().document;
    let id = doc.create_object(Object { pos: [10.0, 10.0], size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Default::default() });
    doc.create_object(Object { pos: [15.0, 10.0], size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, ..Default::default() });
    ws.session_mut().unwrap().selection.ids = vec![id];
    for (area, name, expected) in [("SelectionWithoutBackground", "only.png", [255, 0, 0, 255]), ("SelectionWithBackground", "background.png", [0, 0, 255, 255])] {
        let path = home.root.join(name);
        call(&mut ws, "export.job.run", json!({"items": [{"path": path, "format": "png", "area": area}], "overwrite": "replace"})).unwrap();
        let image = dtool_export::raster::decode_file(&path, ExportFormat::Png).unwrap();
        assert_eq!(image.dimensions(), (10, 10));
        assert_eq!(image.get_pixel(7, 5).0, expected, "{area}");
    }
    assert_eq!(ws.session().unwrap().document.objects.len(), 2);
}

#[test]
fn current_page_follows_view_and_schema_routes_to_real_writer_capabilities() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _home = test_home::enter("c13_context_schema");
    let mut ws = workspace(3);
    let schema = call(&mut ws, "export.options.schema", json!({})).unwrap();
    let formats = schema["formats"].as_array().unwrap();
    assert_eq!(formats.iter().filter(|f| f["available"] == true).count(), 6);
    assert!(formats.iter().any(|f| f["format"] == "tiff" && f["available"] == false));
    let rect = dtool_core::scene::page_rects(&ws.session().unwrap().document)[1];
    let session = ws.session_mut().unwrap();
    session.view.zoom = 1.0;
    session.view.scroll = [100.0 - rect[0] - rect[2] / 2.0, 80.0 - rect[1] - rect[3] / 2.0];
    assert_eq!(ExportContext::for_view(session, [200.0, 160.0]).current_page, 1);
    let mut dialog = ExportDialog::new();
    dialog.document_viewport = Some([200.0, 160.0]);
    dialog.set_session_context(ws.session().unwrap());
    dialog.open_with(&ws.session().unwrap().document, Some(ExportFormat::Png));
    dialog.options.area = ExportArea::CurrentPage;
    dialog.refresh(&ws.session().unwrap().document);
    assert_eq!(dialog.preview_size(), Some((32, 24)));
    assert_eq!(dialog.context.current_page, 1);
}
