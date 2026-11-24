use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportRequest};
use dtool_protocol::Command;
use serde_json::json;

#[test]
fn c06_fix5_export_while_isolated_keeps_other_visible_objects() {
    let mut session = Session::new(Document::new([100.0, 100.0], Units::Px, 72.0, 1));
    session.has_document = true;
    let inside = session.document.create_object(Object { pos: [0.0, 0.0], size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    let outside = session.document.create_object(Object { pos: [50.0, 50.0], size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, ..Object::default() });
    session.run(&Command::with_params("node.isolate", json!({ "nodes": [inside.0] }))).unwrap();
    assert_eq!(session.view.isolate_target(), Some(inside));
    let path = std::env::temp_dir().join(format!("dtool_c06_fix5_{}.svg", std::process::id()));
    let request = ExportRequest { format: ExportFormat::Svg, area: ExportArea::Rect([0.0, 0.0, 100.0, 100.0]), pages: None, path: path.clone(), preset: None, options: json!({}) };
    export_svg(&session.document, &request, &SvgOptions::default(), &ExportControl::new()).expect("svg");
    let svg = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let lower = svg.to_lowercase();
    assert!(lower.contains("#ff0000") || lower.contains("rgb(255,0,0)") || lower.contains("rgb(255, 0, 0)"), "{svg}");
    assert!(lower.contains("#0000ff") || lower.contains("rgb(0,0,255)") || lower.contains("rgb(0, 0, 255)"), "{svg}");
    let _ = ObjectId(outside.0);
}