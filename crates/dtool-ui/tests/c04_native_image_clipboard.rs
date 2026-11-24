use dtool_core::clipboard::ClipboardKind;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use dtool_ui::os_clipboard::{accept_native_image, image_for_copy, NativeImage};
use serde_json::json;
fn image() -> NativeImage { NativeImage { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 128] } }

#[test]
fn external_image_pastes_with_embedded_source_and_one_undo() {
    let mut ws = Workspace::default();
    ws.new_document("image");
    assert!(accept_native_image(ws.app_clipboard(), image()).unwrap());
    assert_eq!(ws.app_clipboard().kind(), ClipboardKind::Objects);
    ws.run(Scope::active(), Command::with_params("edit.paste", json!({"at": [0.0, 0.0]}))).1.unwrap();
    let doc = &ws.session().unwrap().document;
    assert_eq!(doc.objects.len(), 1);
    assert_eq!(doc.objects[0].raster_data.as_ref().unwrap(), &image().rgba);
    let source = doc.resources[0].data.as_ref().unwrap();
    let decoded = dtool_export::raster::decode(source, dtool_export::ExportFormat::Png).unwrap();
    assert_eq!(decoded.as_raw(), &image().rgba);
    ws.run(Scope::active(), Command::with_params("edit.undo", json!({}))).1.unwrap();
    assert!(ws.session().unwrap().document.objects.is_empty());
    ws.run(Scope::active(), Command::with_params("edit.redo", json!({}))).1.unwrap();
    assert_eq!(ws.session().unwrap().document.objects.len(), 1);
}

#[test]
fn new_from_native_image_is_sized_to_pixels_and_bad_image_preserves_clipboard() {
    let mut ws = Workspace::default();
    accept_native_image(ws.app_clipboard(), image()).unwrap();
    let id = dtool_core::clipboard::new_document_from_clipboard(&mut ws).unwrap();
    let doc = &ws.get(id).unwrap().session.document;
    assert_eq!(doc.pages[0].size, [2.0, 1.0]);
    assert_eq!(doc.objects.len(), 1);
    let before = ws.app_clipboard().get_payload();
    assert!(accept_native_image(ws.app_clipboard(), NativeImage { width: 2, height: 1, rgba: vec![0] }).is_err());
    assert!(std::sync::Arc::ptr_eq(&before, &ws.app_clipboard().get_payload()));
}

#[test]
fn copying_an_image_keeps_internal_objects_when_native_image_is_unchanged() {
    let mut ws = Workspace::default();
    ws.new_document("image");
    accept_native_image(ws.app_clipboard(), image()).unwrap();
    ws.run(Scope::active(), Command::with_params("edit.paste", json!({"at": [0.0, 0.0]}))).1.unwrap();
    ws.run(Scope::active(), Command::with_params("edit.copy", json!({}))).1.unwrap();
    let native = image_for_copy(ws.session().unwrap()).unwrap().unwrap();
    let before = ws.app_clipboard().get_payload();
    assert!(!accept_native_image(ws.app_clipboard(), native).unwrap());
    assert!(std::sync::Arc::ptr_eq(&before, &ws.app_clipboard().get_payload()));
}
