use std::path::PathBuf;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Hatch, HatchLine, HatchUnits, Paint};
use dtool_export::pdf::{export_pdf, PdfOptions};
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportReceipt, ExportRequest};

fn gradient(kind: GradientKind, ratio: f64) -> Paint {
    Paint::Gradient {
        kind,
        stops: vec![
            GradientStop { offset: 0.0, colour: Colour::new(255, 0, 0, 255) },
            GradientStop { offset: 1.0, colour: Colour::new(0, 0, 255, 128) },
        ],
        geom: GradientGeom { start: [10.0, 10.0], end: [50.0, 10.0], ratio, ..GradientGeom::default() },
    }
}

fn hatch() -> Paint {
    Paint::Hatch(Hatch {
        lines: vec![HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing: 5.0, dash: vec![] }],
        colour: Colour::new(0, 128, 0, 255),
        background: None,
        weight: 1.0,
        units: HatchUnits::Document,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    })
}

fn doc_with(fill: Paint) -> Document {
    let mut doc = Document::new([80.0, 80.0], Units::Pt, 72.0, 1);
    doc.objects.push(Object { id: ObjectId(7), name: "swatch".to_owned(), pos: [10.0, 10.0], size: [40.0, 40.0], fill, kind: ObjectKind::Shape, ..Object::default() });
    doc.next_object_id = 8;
    doc
}

fn out_path(name: &str, ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dtool_c13_paint_{name}_{}.{ext}", std::process::id()))
}

fn svg(doc: &Document, name: &str) -> (String, ExportReceipt) {
    let path = out_path(name, "svg");
    let request = ExportRequest { format: ExportFormat::Svg, area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: serde_json::json!({}) };
    let receipt = export_svg(doc, &request, &SvgOptions::default(), &ExportControl::new()).expect("svg export");
    let text = std::fs::read_to_string(&path).expect("svg written");
    let _ = std::fs::remove_file(&path);
    (text, receipt)
}

fn pdf(doc: &Document, name: &str) -> Vec<u8> {
    let path = out_path(name, "pdf");
    let request = ExportRequest { format: ExportFormat::Pdf, area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: serde_json::json!({}) };
    export_pdf(doc, &request, &PdfOptions::default(), &ExportControl::new()).expect("pdf export");
    let bytes = std::fs::read(&path).expect("pdf written");
    let _ = std::fs::remove_file(&path);
    bytes
}

fn contains(bytes: &[u8], needle: &str) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn svg_linear_gradient_is_a_real_gradient_in_document_space() {
    let (text, receipt) = svg(&doc_with(gradient(GradientKind::Linear, 1.0)), "linear");
    assert!(text.contains("<defs><linearGradient id=\"dtool-paint-7\" gradientUnits=\"userSpaceOnUse\""), "{text}");
    assert!(text.contains("x1=\"20\"") || text.contains("x1=\"20.0"), "{text}");
    assert!(text.contains("fill=\"url(#dtool-paint-7)\""), "{text}");
    assert!(text.contains("stop-color=\"#ff0000\""), "{text}");
    assert!(text.contains("stop-color=\"#0000ff\" stop-opacity=\"0.50\""), "{text}");
    assert!(receipt.warnings.iter().all(|w| !w.contains("paint_flattened")), "{:?}", receipt.warnings);
    let defs = text.find("<defs>").unwrap();
    let path = text.find("<path").unwrap();
    assert!(defs < path);
}

#[test]
fn svg_radial_and_elliptical_gradients_use_radial_gradient() {
    let (radial, _) = svg(&doc_with(gradient(GradientKind::Radial, 1.0)), "radial");
    assert!(radial.contains("<radialGradient id=\"dtool-paint-7\""), "{radial}");
    assert!(!radial.contains("gradientTransform"), "{radial}");
    let (elliptical, _) = svg(&doc_with(gradient(GradientKind::Elliptical, 0.5)), "elliptical");
    assert!(elliptical.contains("<radialGradient id=\"dtool-paint-7\""), "{elliptical}");
    assert!(elliptical.contains("gradientTransform=\""), "{elliptical}");
}

#[test]
fn svg_conical_paint_uses_a_pattern_and_unsupported_hatch_warns() {
    let (text, receipt) = svg(&doc_with(gradient(GradientKind::Conical, 1.0)), "conical");
    assert!(text.contains("<pattern id=\"dtool-appearance-7-0\""), "{text}");
    assert!(text.contains("fill=\"url(#dtool-appearance-7-0)\""), "{text}");
    assert!(receipt.warnings.is_empty(), "{:?}", receipt.warnings);
    let (text, receipt) = svg(&doc_with(hatch()), "hatch");
    assert!(text.contains("fill=\"#008000\""), "{text}");
    assert!(receipt.warnings.iter().any(|w| w == "export.svg.paint_flattened:7"), "{:?}", receipt.warnings);
    let (_, receipt) = svg(&doc_with(Paint::Solid { colour: Colour::new(1, 2, 3, 255) }), "solid");
    assert!(receipt.warnings.is_empty(), "{:?}", receipt.warnings);
}

#[test]
fn pdf_gradient_fill_writes_a_shading_not_a_flat_colour() {
    let gradient_pdf = pdf(&doc_with(gradient(GradientKind::Linear, 1.0)), "linear");
    let solid_pdf = pdf(&doc_with(Paint::Solid { colour: Colour::new(255, 0, 0, 255) }), "solid");
    assert!(gradient_pdf.starts_with(b"%PDF"));
    assert!(contains(&gradient_pdf, "/Shading") || contains(&gradient_pdf, "/Pattern"), "gradient PDF has no shading/pattern resource");
    assert!(!contains(&solid_pdf, "/Shading"), "solid PDF should not need a shading");
}
