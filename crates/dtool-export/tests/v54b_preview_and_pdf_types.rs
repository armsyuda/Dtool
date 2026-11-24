use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{Colour, Paint};
use dtool_export::pdf::{export_pdf, PdfOptions};
use dtool_export::raster::{area_region, render_area, render_preview, ExportError, ExportOptions};
use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportRequest};
use serde_json::json;

fn fixture() -> Document {
    let mut doc = Document::new([100.0, 60.0], Units::Pt, 72.0, 1);
    doc.objects.push(Object { id: ObjectId(1), name: "box".to_owned(), pos: [10.0, 10.0], size: [20.0, 10.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    doc.objects.push(Object { id: ObjectId(2), name: "Cover".to_owned(), pos: [40.0, 20.0], size: [30.0, 24.0], kind: ObjectKind::Artboard, ..Object::default() });
    doc.next_object_id = 3;
    doc
}

fn options(area: ExportArea) -> ExportOptions {
    ExportOptions { area, ..ExportOptions::for_format(ExportFormat::Png) }
}

#[test]
fn v54b_preview_missing_or_plain_artboard_is_not_found() {
    let doc = fixture();
    let missing = render_preview(&doc, &options(ExportArea::Artboard(ObjectId(99)))).unwrap_err();
    assert_eq!(missing, ExportError::NotFound("artboard:99".to_string()));
    assert_eq!(missing.code(), "NOT_FOUND");
    assert_eq!(missing.message_key(), "error.export.bad_area");
    let plain = area_region(&doc, &ExportArea::Artboard(ObjectId(1))).unwrap_err();
    assert_eq!(plain.code(), "NOT_FOUND");
    assert_eq!(plain.to_command_error().code, "NOT_FOUND");
}

#[test]
fn v54b_preview_bad_rect_is_invalid_params_not_unavailable() {
    let doc = fixture();
    let error = render_preview(&doc, &options(ExportArea::Rect([0.0, 0.0, 0.0, 10.0]))).unwrap_err();
    assert_eq!(error.code(), "INVALID_PARAMS");
    assert!(!matches!(error, ExportError::Unavailable(_)));
    let empty = area_region(&doc, &ExportArea::SelectionWithBackground).unwrap_err();
    assert_eq!(empty.code(), "INVALID_PARAMS");
}

#[test]
fn v54b_preview_real_artboard_renders_at_its_size_with_its_name() {
    let doc = fixture();
    let out = render_area(&doc, &ExportArea::Artboard(ObjectId(2)), &options(ExportArea::Artboard(ObjectId(2)))).expect("artboard preview");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "Cover");
    assert_eq!(out[0].1.dimensions(), (30, 24));
}

#[test]
fn v54b_pdf_uses_the_crate_export_types() {
    let doc = fixture();
    let path = std::env::temp_dir().join(format!("dtool_export_v54b_{}.pdf", std::process::id()));
    let req = ExportRequest { format: ExportFormat::Pdf, area: ExportArea::Artboard(ObjectId(2)), pages: None, path: path.clone(), preset: None, options: json!({}) };
    let same: &dtool_export::pdf::ExportRequest = &req;
    let control = ExportControl::new();
    let receipt = export_pdf(&doc, same, &PdfOptions::default(), &control).expect("pdf artboard export");
    let bytes = std::fs::read(&path).expect("pdf file");
    let _ = std::fs::remove_file(&path);
    assert!(bytes.starts_with(b"%PDF"));
    assert_eq!(receipt.bytes as usize, bytes.len());
    let cancelled = ExportControl::new();
    cancelled.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(export_pdf(&doc, &req, &PdfOptions::default(), &cancelled).is_err());
    assert!(!path.exists());
}