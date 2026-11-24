use std::path::PathBuf;
use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathFlow, PathTextData};
use dtool_core::paint::{Colour, Paint};
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{export_object, ExportArea, ExportControl, ExportFormat, ExportRequest};
use serde_json::json;
fn fixture() -> Document {
    let mut doc = Document::new([72.0, 72.0], Units::Pt, 72.0, 1);
    doc.stories.push(Story {
        id: StoryId(1),
        text: "Hello".to_owned(),
        ranges: vec![json!({"start":0,"end":5,"family":"Arial","style":"Regular","size_pt":12.0})],
        ..Story::default()
    });
    doc.objects.push(Object {
        id: ObjectId(1),
        name: "path".to_owned(),
        pos: [0.0, 0.0],
        size: [72.0, 72.0],
        kind: ObjectKind::Curve(Curve {
            contour: Contour { points: vec![PathPoint::sharp(PointId(1), [8.0, 36.0]), PathPoint::sharp(PointId(2), [64.0, 36.0])], closed: false },
            holes: Vec::new(),
            source_object: None,
        }),
        ..Object::default()
    });
    doc.objects.push(Object {
        id: ObjectId(2),
        name: "path_text".to_owned(),
        pos: [8.0, 24.0],
        size: [56.0, 12.0],
        fill: Paint::Solid { colour: Colour::new(0, 0, 0, 255) },
        kind: ObjectKind::PathText(PathTextData {
            path: ObjectId(1),
            story: StoryId(1),
            flows: vec![PathFlow::default()],
            source_paint: None,
        }),
        ..Object::default()
    });
    doc.next_object_id = 3;
    doc
}

fn temp_path(name: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dtool_export_{name}_{}.{extension}", std::process::id()))
}

fn request(format: ExportFormat, path: &PathBuf) -> ExportRequest {
    ExportRequest {
        format,
        area: ExportArea::WholeDocument,
        pages: None,
        path: path.clone(),
        preset: None,
        options: json!({}),
    }
}
#[test]
fn test_path_text_svg_output_parity() {
    let doc = fixture();
    let before = serde_json::to_value(&doc).unwrap();
    let path = temp_path("svg_parity", "svg");
    let receipt = export_svg(&doc, &request(ExportFormat::Svg, &path), &SvgOptions::default(), &ExportControl::new()).expect("SVG export failed");
    let svg_output = std::fs::read_to_string(&path).expect("SVG output missing");
    let _ = std::fs::remove_file(&path);
    assert_eq!(receipt.bytes as usize, svg_output.len());
    assert!(svg_output.contains("<text"));
    assert!(svg_output.contains("Hello"));
    assert_eq!(doc.stories[0].text, "Hello");
    assert_eq!(doc.objects.len(), 2);
    assert_eq!(serde_json::to_value(&doc).unwrap(), before);
    assert!(export_object(&doc, &doc.objects[1].kind).is_ok());
}