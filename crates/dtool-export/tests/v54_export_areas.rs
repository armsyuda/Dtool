use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{Colour, Paint};
use dtool_export::pdf::{export_pdf, ExportArea as PdfArea, ExportControl as PdfControl, ExportRequest as PdfRequest, PdfOptions};
use dtool_export::png::{export_png_to, PngOptions};
use dtool_export::raster::{area_name_in, artboard_file_name, artboard_region};
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportRequest};
use serde_json::json;
use std::path::PathBuf;

fn fixture() -> Document {
    let mut doc = Document::new([100.0, 60.0], Units::Pt, 72.0, 1);
    doc.objects.push(Object { id: ObjectId(1), name: "box".to_owned(), pos: [10.0, 10.0], size: [20.0, 10.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    doc.next_object_id = 2;
    doc
}

fn temp_path(name: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dtool_export_v54_{name}_{}.{extension}", std::process::id()))
}

fn request(format: ExportFormat, area: ExportArea, path: &PathBuf) -> ExportRequest {
    ExportRequest { format, area, pages: None, path: path.clone(), preset: None, options: json!({}) }
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    (u32::from_be_bytes(bytes[16..20].try_into().unwrap()), u32::from_be_bytes(bytes[20..24].try_into().unwrap()))
}

#[test]
fn v54_rect_area_is_x_y_w_h_in_svg_and_png() {
    let doc = fixture();
    let area = ExportArea::Rect([10.0, 10.0, 20.0, 10.0]);
    let svg_path = temp_path("rect", "svg");
    export_svg(&doc, &request(ExportFormat::Svg, area.clone(), &svg_path), &SvgOptions::default(), &ExportControl::new()).expect("svg rect export");
    let svg = std::fs::read_to_string(&svg_path).unwrap();
    let _ = std::fs::remove_file(&svg_path);
    assert!(svg.contains("viewBox=\"0 0 20.000 10.000\" width=\"20.000pt\" height=\"10.000pt\""), "{svg}");
    let png_path = temp_path("rect", "png");
    export_png_to(&doc, &request(ExportFormat::Png, area, &png_path), &PngOptions::default(), &ExportControl::new()).expect("png rect export");
    let png = std::fs::read(&png_path).unwrap();
    let _ = std::fs::remove_file(&png_path);
    assert_eq!(png_size(&png), (20, 10));
}

#[test]
fn v54_missing_artboard_is_not_found_in_every_codec_and_writes_nothing() {
    let doc = fixture();
    let area = ExportArea::Artboard(ObjectId(99));
    let svg_path = temp_path("missing", "svg");
    let svg = export_svg(&doc, &request(ExportFormat::Svg, area.clone(), &svg_path), &SvgOptions::default(), &ExportControl::new()).unwrap_err();
    assert_eq!(svg.code, "NOT_FOUND");
    assert!(!svg_path.exists());
    let png_path = temp_path("missing", "png");
    let png = export_png_to(&doc, &request(ExportFormat::Png, area, &png_path), &PngOptions::default(), &ExportControl::new()).unwrap_err();
    assert_eq!(png.code, "NOT_FOUND");
    assert!(!png_path.exists());
    let pdf_path = temp_path("missing", "pdf");
    let req = PdfRequest { format: dtool_export::ExportFormat::Pdf, area: PdfArea::Artboard(ObjectId(99)), pages: None, path: pdf_path.clone(), preset: None, options: json!({}) };
    let pdf = export_pdf(&doc, &req, &PdfOptions::default(), &PdfControl::new()).unwrap_err();
    assert_eq!(pdf.code, "NOT_FOUND");
    assert!(!pdf_path.exists());
}

#[test]
fn v54_empty_selection_is_invalid_params_in_svg_and_png() {
    let doc = fixture();
    let area = ExportArea::SelectionWithBackground;
    let svg_path = temp_path("selection", "svg");
    let svg = export_svg(&doc, &request(ExportFormat::Svg, area.clone(), &svg_path), &SvgOptions::default(), &ExportControl::new()).unwrap_err();
    assert_eq!(svg.code, "INVALID_PARAMS");
    let png_path = temp_path("selection", "png");
    let png = export_png_to(&doc, &request(ExportFormat::Png, area, &png_path), &PngOptions::default(), &ExportControl::new()).unwrap_err();
    assert_eq!(png.code, "INVALID_PARAMS");
}
fn with_artboard(name: &str) -> Document {
    let mut doc = fixture();
    doc.objects.push(Object { id: ObjectId(2), name: name.to_owned(), pos: [40.0, 20.0], size: [30.0, 24.0], kind: ObjectKind::Artboard, ..Object::default() });
    doc.next_object_id = 3;
    doc
}

#[test]
fn v54_plain_object_id_is_not_an_artboard() {
    let doc = with_artboard("A");
    assert!(artboard_region(&doc, ObjectId(1)).is_none());
    assert_eq!(artboard_region(&doc, ObjectId(2)), Some((40.0, 20.0, 30.0, 24.0)));
    let area = ExportArea::Artboard(ObjectId(1));
    let svg_path = temp_path("plain", "svg");
    assert_eq!(export_svg(&doc, &request(ExportFormat::Svg, area.clone(), &svg_path), &SvgOptions::default(), &ExportControl::new()).unwrap_err().code, "NOT_FOUND");
    let png_path = temp_path("plain", "png");
    assert_eq!(export_png_to(&doc, &request(ExportFormat::Png, area, &png_path), &PngOptions::default(), &ExportControl::new()).unwrap_err().code, "NOT_FOUND");
    let pdf_path = temp_path("plain", "pdf");
    let req = PdfRequest { format: dtool_export::ExportFormat::Pdf, area: PdfArea::Artboard(ObjectId(1)), pages: None, path: pdf_path.clone(), preset: None, options: json!({}) };
    assert_eq!(export_pdf(&doc, &req, &PdfOptions::default(), &PdfControl::new()).unwrap_err().code, "NOT_FOUND");
}

#[test]
fn v54_artboard_export_uses_artboard_size_and_name() {
    let doc = with_artboard("Cover: A/B?");
    let area = ExportArea::Artboard(ObjectId(2));
    let png_path = temp_path("artboard", "png");
    export_png_to(&doc, &request(ExportFormat::Png, area.clone(), &png_path), &PngOptions::default(), &ExportControl::new()).expect("png artboard export");
    let png = std::fs::read(&png_path).unwrap();
    let _ = std::fs::remove_file(&png_path);
    assert_eq!(png_size(&png), (30, 24));
    let svg_path = temp_path("artboard", "svg");
    export_svg(&doc, &request(ExportFormat::Svg, area.clone(), &svg_path), &SvgOptions::default(), &ExportControl::new()).expect("svg artboard export");
    let svg = std::fs::read_to_string(&svg_path).unwrap();
    let _ = std::fs::remove_file(&svg_path);
    assert!(svg.contains("viewBox=\"0 0 30.000 24.000\" width=\"30.000pt\" height=\"24.000pt\""));
    assert_eq!(artboard_file_name(&doc, ObjectId(2)), "Cover_ A_B_");
    assert_eq!(area_name_in(&doc, &area), "Cover_ A_B_");
    assert_eq!(artboard_file_name(&with_artboard("  "), ObjectId(2)), "artboard_2");
    assert_eq!(artboard_file_name(&doc, ObjectId(1)), "artboard_1");
}