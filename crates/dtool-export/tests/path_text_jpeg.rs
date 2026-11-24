use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathFlow, PathTextData};
use dtool_core::paint::{Colour, Paint};
use dtool_core::Session;
use dtool_export::jpeg::export_jpeg;
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

fn encoded(doc: &Document) -> Vec<u8> {
    let encoded = export_jpeg(doc, None).expect("JPEG export failed");
    assert_eq!(image::guess_format(&encoded).unwrap(), image::ImageFormat::Jpeg);
    encoded
}

#[test]
fn test_path_text_jpeg_output_parity() {
    let doc = fixture();
    let bytes = encoded(&doc);
    let decoded = image::load_from_memory(&bytes).expect("JPEG cannot be decoded").to_rgb8();
    assert_eq!(decoded.dimensions(), (72, 72));
    assert_eq!(doc.stories.len(), 1);
    assert_eq!(doc.objects.len(), 2);
}

#[test]
fn test_path_text_glyph_positions_follow_path() {
    let original = fixture();
    let before = encoded(&original);
    let mut moved = original.clone();
    let path = moved.objects.iter_mut().find(|object| object.id == ObjectId(1)).unwrap();
    let ObjectKind::Curve(curve) = &mut path.kind else { panic!("fixture path kind changed"); };
    for point in &mut curve.contour.points { point.pos[1] += 12.0; }
    let after = encoded(&moved);
    assert_ne!(before, after, "moving only the referenced path must move glyphs");
}

#[test]
fn test_source_document_unchanged_after_export() {
    let session = Session::new(fixture());
    let document_before = serde_json::to_value(&session.document).unwrap();
    let history_before = format!("{:?}", session.undo);
    let generation_before = session.generation;
    let _ = encoded(&session.document);
    assert_eq!(serde_json::to_value(&session.document).unwrap(), document_before);
    assert_eq!(format!("{:?}", session.undo), history_before);
    assert_eq!(session.generation, generation_before);
}