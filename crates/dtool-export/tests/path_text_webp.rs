use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathFlow, PathTextData};
use dtool_core::paint::{Colour, Paint};
use dtool_core::Session;
use image::RgbaImage;
use serde_json::json;

fn fixture() -> Document {
    let mut doc = Document::new([72.0, 72.0], Units::Points, 72.0, 1);
    doc.stories.push(Story {
        id: StoryId(1),
        text: "OO".to_owned(),
        ranges: vec![json!({"start":0,"end":2,"family":"Pretendard","style":"Regular","size_pt":12.0})],
        ..Story::default()
    });
    doc.objects.push(Object {
        id: ObjectId(1),
        pos: [0.0, 0.0],
        size: [72.0, 72.0],
        kind: ObjectKind::Curve(Curve {
            contour: Contour { points: vec![PathPoint::sharp(PointId(1), [8.0,36.0]), PathPoint::sharp(PointId(2), [64.0,36.0])], closed: false },
            holes: Vec::new(),
            source_object: None,
        }),
        ..Object::default()
    });
    doc.objects.push(Object {
        id: ObjectId(2),
        pos: [8.0, 24.0],
        size: [24.0, 12.0],
        fill: Paint::Solid { colour: Colour::new(0,0,0,255) },
        kind: ObjectKind::PathText(PathTextData {
            path: ObjectId(1), story: StoryId(1), flows: vec![PathFlow::default()], source_paint: None,
        }),
        ..Object::default()
    });
    doc.next_object_id = 3;
    doc
}

fn decoded(doc: &Document) -> RgbaImage {
    let encoded = dtool_export::export_webp(doc, None).expect("canonical WebP export failed");
    assert_eq!(image::guess_format(&encoded).unwrap(), image::ImageFormat::WebP);
    image::load_from_memory(&encoded).expect("exported WebP cannot be decoded").to_rgba8()
}

#[test]
fn webp_fixture_72pt_at_72dpi_contains_path_text_ink() {
    let doc = fixture();
    let rendered = decoded(&doc);
    assert_eq!(rendered.dimensions(), (72,72));
    assert!(rendered.pixels().any(|pixel| pixel[3] > 0), "live Story on the open path produced no glyph ink");
    assert!(rendered.pixels().any(|pixel| pixel[3] == 0), "text export unexpectedly filled the entire document");
}

#[test]
fn webp_glyph_positions_follow_referenced_path_translation() {
    let original = fixture();
    let before = decoded(&original);
    assert!(before.pixels().any(|pixel| pixel[3] > 0), "cannot test glyph movement with an empty raster");
    let mut translated = original.clone();
    let path = translated.objects.iter_mut().find(|object| object.id == ObjectId(1)).unwrap();
    let ObjectKind::Curve(curve) = &mut path.kind else { panic!("fixture path kind changed"); };
    for point in &mut curve.contour.points { point.pos[1] += 12.0; }
    let after = decoded(&translated);
    assert_eq!(after.dimensions(), before.dimensions());
    for y in 0..72 {
        for x in 0..72 {
            let expected = if y >= 12 { before.get_pixel(x,y-12)[3] } else { 0 };
            assert_eq!(after.get_pixel(x,y)[3], expected, "glyph alpha did not follow path at ({x},{y})");
        }
    }
    assert_ne!(before.as_raw(), after.as_raw(), "moving only the referenced path must move glyphs");
}

#[test]
fn webp_export_preserves_canonical_document_and_history() {
    let session = Session::new(fixture());
    let document_before = serde_json::to_value(&session.document).unwrap();
    let history_before = format!("{:?}", session.undo);
    let generation_before = session.generation;
    let _ = decoded(&session.document);
    assert_eq!(serde_json::to_value(&session.document).unwrap(), document_before);
    assert_eq!(format!("{:?}", session.undo), history_before);
    assert_eq!(session.generation, generation_before);
}
