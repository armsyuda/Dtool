use dtool_core::document::Units;
use dtool_core::{Command, Document, Session};
use dtool_export::pdf::{export_pdf, PdfOptions};
use dtool_export::{ExportArea, ExportControl, ExportRequest};
use dtool_io::import_pdf::{import_pdf, ImportOptions};
use dtool_io::import_report::LossKind;
use serde_json::json;

fn document() -> Document {
    let mut session = Session::new(Document::new([100.0, 80.0], Units::Px, 300.0, 1));
    let mut add = Command::new("shape.add");
    add.params = json!({ "kind": dtool_core::shape::ShapeKind::Rectangle, "size": [40.0, 20.0], "pos": [10.0, 10.0] });
    let added = session.run(&add).expect("shape added");
    let id = added.get("id").or_else(|| added.get("object_id")).or_else(|| added.pointer("/ids/0")).and_then(|v| v.as_u64()).expect("id");
    let mut fill = Command::new("object.update");
    fill.params = json!({ "id": id, "fill": dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(200, 30, 40, 255) } });
    session.run(&fill).expect("filled");
    session.document
}

fn export(doc: &Document, name: &str, opts: &PdfOptions) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("pdf_oi_{}_{name}.pdf", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let req = ExportRequest { format: dtool_export::raster::parse_format("pdf").unwrap(), area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) };
    export_pdf(doc, &req, opts, &ExportControl::new()).expect("pdf export");
    path
}

#[test]
fn pdf_oi_own_print_pdf_with_output_intent_imports_back_with_a_warning() {
    let doc = document();
    let opts = PdfOptions { embed_icc: true, ..PdfOptions::default() };
    let path = export(&doc, "print", &opts);
    let bytes = std::fs::read(&path).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/OutputIntents"), "print export carries an OutputIntent");
    let imported = import_pdf(&path, &ImportOptions::default());
    let _ = std::fs::remove_file(&path);
    let (back, report) = imported.expect("own print PDF imports without error");
    assert!(report.objects >= 1 && !back.objects.is_empty(), "{report:?}");
    let intent: Vec<_> = report.losses.iter().filter(|l| l.kind == LossKind::Icc && l.detail.contains("output intent")).collect();
    assert_eq!(intent.len(), 1, "output intent stays listed as a warning: {:?}", report.losses);
    assert!(!intent[0].required, "output intent is a warning, not a required loss");
    let value = dtool_io::import_commands::report_value(&report);
    let listed = value["losses"].as_array().unwrap();
    assert!(listed.iter().any(|l| l["required"] == false && l["detail"].as_str().is_some_and(|d| d.contains("output intent"))), "{value}");
    assert!(report.losses.iter().filter(|l| l.required).all(|l| !l.detail.contains("output intent")), "{:?}", report.losses);
}

#[test]
fn pdf_oi_plain_pdf_has_no_output_intent_warning() {
    let doc = document();
    let path = export(&doc, "plain", &PdfOptions::default());
    let imported = import_pdf(&path, &ImportOptions::default());
    let _ = std::fs::remove_file(&path);
    let (_, report) = imported.expect("plain PDF imports");
    assert!(!report.losses.iter().any(|l| l.detail.contains("output intent")), "{:?}", report.losses);
}