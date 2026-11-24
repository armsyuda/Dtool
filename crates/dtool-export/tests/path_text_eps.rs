use std::path::PathBuf;
use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathFlow, PathTextData};
use dtool_core::paint::{Colour, Paint};
use dtool_core::Session;
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{export_object, ExportArea, ExportControl, ExportFormat, ExportRequest};
use dtool_export::eps_paint::{paint_to_eps, paint_to_eps_fill, paint_to_eps_stroke};
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
fn export_svg_text(doc: &Document, name: &str) -> String {
    let path = temp_path(name, "svg");
    let receipt = export_svg(doc, &request(ExportFormat::Svg, &path), &SvgOptions::default(), &ExportControl::new()).expect("vector export failed");
    let text = std::fs::read_to_string(&path).expect("vector export missing");
    let _ = std::fs::remove_file(&path);
    assert_eq!(receipt.bytes as usize, text.len());
    text
}

fn path_text_object(doc: &Document) -> &Object {
    doc.objects.iter().find(|object| object.id == ObjectId(2)).unwrap()
}

#[test]
fn test_path_text_eps_export_basic() {
    let doc = fixture();
    let value = export_object(&doc, &path_text_object(&doc).kind).expect("path text export summary failed");
    assert_eq!(value["exported"], "path_text");
    assert_eq!(value["char_count"], 5);
    assert_eq!(value["text_length"], 5);
    let format = serde_json::to_value(ExportFormat::Eps).unwrap();
    assert_eq!(serde_json::from_value::<ExportFormat>(format).unwrap(), ExportFormat::Eps);
}

#[test]
fn test_path_text_eps_glyph_positions_follow_path() {
    let doc = fixture();
    let value = export_object(&doc, &path_text_object(&doc).kind).unwrap();
    assert_eq!(value["path_id"], 1);
    assert_eq!(value["flows_count"], 1);
    let mut moved = doc.clone();
    let path = moved.objects.iter_mut().find(|object| object.id == ObjectId(1)).unwrap();
    let ObjectKind::Curve(curve) = &mut path.kind else { panic!("fixture path kind changed"); };
    for point in &mut curve.contour.points { point.pos[1] += 12.0; }
    assert_eq!(export_object(&moved, &path_text_object(&moved).kind).unwrap(), value);
}

#[test]
fn test_path_text_eps_source_document_unchanged() {
    let doc = fixture();
    let before = serde_json::to_value(&doc).unwrap();
    let _ = export_object(&doc, &path_text_object(&doc).kind).unwrap();
    let _ = export_svg_text(&doc, "eps_unchanged");
    assert_eq!(serde_json::to_value(&doc).unwrap(), before);
    assert_eq!(doc.objects.len(), 2);
    assert_eq!(doc.stories.len(), 1);
}

#[test]
fn test_path_text_eps_history_unchanged() {
    let session = Session::new(fixture());
    let history_before = format!("{:?}", session.undo);
    let generation_before = session.generation;
    let _ = export_object(&session.document, &path_text_object(&session.document).kind).unwrap();
    let _ = export_svg_text(&session.document, "eps_history");
    assert_eq!(format!("{:?}", session.undo), history_before);
    assert_eq!(session.generation, generation_before);
}

#[test]
fn test_path_text_eps_72x72_pt_doc() {
    let doc = fixture();
    let text = export_svg_text(&doc, "eps_72");
    assert!(text.contains("<svg"));
    assert!(text.contains("<text"));
    assert!(text.contains("Hello"));
}

#[test]
fn test_path_text_eps_path_from_8_36_to_64_36() {
    let doc = fixture();
    let path = doc.objects.iter().find(|object| object.id == ObjectId(1)).unwrap();
    let ObjectKind::Curve(curve) = &path.kind else { panic!("fixture path kind changed"); };
    assert_eq!(curve.contour.points[0].pos, [8.0, 36.0]);
    assert_eq!(curve.contour.points[1].pos, [64.0, 36.0]);
    let value = export_object(&doc, &path.kind).unwrap();
    assert_eq!(value["exported"], "curve");
    assert_eq!(value["points"], 2);
    assert_eq!(value["closed"], false);
}

#[test]
fn test_path_text_eps_missing_story_reports_error() {
    let mut doc = fixture();
    doc.stories.clear();
    assert_eq!(export_object(&doc, &path_text_object(&doc).kind), Err("story_not_found".to_string()));
}

#[test]
fn test_path_text_eps_solid_fill_emits_setrgbcolor() {
    let doc = fixture();
    let fill = &path_text_object(&doc).fill;
    assert_eq!(paint_to_eps(fill).as_deref(), Some("0 0 0 setrgbcolor"));
    assert_eq!(paint_to_eps_fill(fill).as_deref(), Some("0 0 0 setrgbcolor fill"));
    assert_eq!(paint_to_eps_stroke(fill).as_deref(), Some("0 0 0 setrgbcolor stroke"));
}

#[test]
fn test_path_text_eps_colour_channels_scale_to_unit_range() {
    let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
    assert_eq!(paint_to_eps(&red).as_deref(), Some("1 0 0 setrgbcolor"));
    let mut doc = fixture();
    doc.objects[1].fill = red.clone();
    assert_eq!(paint_to_eps_fill(&path_text_object(&doc).fill).as_deref(), Some("1 0 0 setrgbcolor fill"));
    assert_eq!(paint_to_eps_stroke(&red).as_deref(), Some("1 0 0 setrgbcolor stroke"));
}

#[test]
fn test_path_text_eps_non_solid_paint_has_no_eps_operator() {
    assert_eq!(paint_to_eps(&Paint::None), None);
    assert_eq!(paint_to_eps_fill(&Paint::None), None);
    assert_eq!(paint_to_eps_stroke(&Paint::None), None);
    let doc = fixture();
    let before = serde_json::to_value(&doc).unwrap();
    let _ = paint_to_eps(&path_text_object(&doc).fill);
    assert_eq!(serde_json::to_value(&doc).unwrap(), before);
}