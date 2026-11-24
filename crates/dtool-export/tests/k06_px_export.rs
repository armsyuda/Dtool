use std::path::PathBuf;
use dtool_core::document::{Document, Units};
use dtool_export::pdf::{export_pdf, PdfOptions};
use dtool_export::raster::{target_size, ExportOptions};
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportRequest};
use serde_json::json;

fn temp_path(name: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dtool_k06_{name}_{}.{extension}", std::process::id()))
}

fn pdf_request(path: &PathBuf) -> dtool_export::pdf::ExportRequest {
    dtool_export::pdf::ExportRequest { format: dtool_export::ExportFormat::Pdf, area: dtool_export::pdf::ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) }
}

fn svg_request(path: &PathBuf) -> ExportRequest {
    ExportRequest { format: ExportFormat::Svg, area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) }
}

#[test]
fn raster_export_px_doc_at_96dpi_is_1_to_1() {
    let region = (0.0, 0.0, 800.0, 600.0);
    assert_eq!(target_size(region, 96.0, &ExportOptions::default()).unwrap(), (800, 600));
    assert_eq!(target_size(region, 96.0, &ExportOptions { dpi: Some(192.0), ..ExportOptions::default() }).unwrap(), (1600, 1200));
    assert_eq!(target_size(region, 96.0, &ExportOptions { scale: Some(0.5), ..ExportOptions::default() }).unwrap(), (400, 300));
}

#[test]
fn pdf_page_size_in_points() {
    let doc = Document::new([960.0, 480.0], Units::Px, 96.0, 1);
    let path = temp_path("pdf", "pdf");
    export_pdf(&doc, &pdf_request(&path), &PdfOptions::default(), &dtool_export::pdf::ExportControl::new()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/MediaBox[0 0 720 360]"), "{}", text.chars().take(2000).collect::<String>());
}

#[test]
fn svg_physical_size() {
    let doc = Document::new([960.0, 480.0], Units::Px, 96.0, 1);
    let path = temp_path("svg", "svg");
    export_svg(&doc, &svg_request(&path), &SvgOptions::default(), &ExportControl::new()).unwrap();
    let svg = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(svg.contains("width=\"720.000pt\" height=\"360.000pt\""), "{svg}");
    assert!(svg.contains("viewBox=\"0 0 960.000 480.000\""), "{svg}");
}
