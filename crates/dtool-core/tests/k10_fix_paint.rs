use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::paint::{collect_document_resources, paint_from_param, transform_paint_matrix, BitmapPaint, Colour, GradientGeom, GradientKind, GradientStop, Hatch, HatchLine, HatchUnits, Paint, PaintResourceKind, PatternPaint};
use dtool_core::{Object, ObjectKind, Session};
use serde_json::json;

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn hatch(matrix: [f64; 6]) -> Hatch {
    Hatch { lines: vec![HatchLine { angle_rad: 0.5, origin: [0.0, 0.0], shift: 0.0, spacing: 4.0, dash: vec![] }], colour: Colour::new(1, 2, 3, 255), background: None, weight: 1.0, units: HatchUnits::Document, matrix }
}

fn bitmap(id: &str) -> Paint {
    Paint::Bitmap(BitmapPaint { resource_id: id.to_string(), matrix: IDENTITY, seed: None, single_image: false })
}

#[test]
fn paint_from_param_accepts_hex_names_null_and_typed() {
    assert_eq!(paint_from_param(&json!("#FF0000")).unwrap(), Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
    assert_eq!(paint_from_param(&json!("#f00")).unwrap(), Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
    assert_eq!(paint_from_param(&json!("red")).unwrap(), Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
    assert_eq!(paint_from_param(&json!(null)).unwrap(), Paint::None);
    assert_eq!(paint_from_param(&json!("none")).unwrap(), Paint::None);
    let typed = serde_json::to_value(bitmap("img")).unwrap();
    assert_eq!(paint_from_param(&typed).unwrap(), bitmap("img"));
}

#[test]
fn paint_from_param_rejects_unknown_kind_and_bad_values() {
    assert_eq!(paint_from_param(&json!({ "type": "bogus" })).unwrap_err().code, "INVALID_PARAMS");
    assert!(paint_from_param(&json!({ "colour": "#fff" })).is_ok());
    assert!(paint_from_param(&json!("#12")).is_err());
    assert!(paint_from_param(&json!("notacolour")).is_err());
    assert!(paint_from_param(&json!(42)).is_err());
    assert!(paint_from_param(&serde_json::to_value(bitmap("")).unwrap()).is_err());
}

#[test]
fn hatch_and_gradient_compose_transform_after_paint_matrix() {
    let translate = [1.0, 0.0, 0.0, 1.0, 5.0, 7.0];
    let scale = [2.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut hatched = Paint::Hatch(hatch(translate));
    transform_paint_matrix(&mut hatched, scale);
    let mut gradient = Paint::Gradient { kind: GradientKind::Linear, stops: vec![GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 255) }], geom: GradientGeom::from_legacy_matrix(translate) };
    transform_paint_matrix(&mut gradient, scale);
    let Paint::Hatch(h) = &hatched else { panic!("hatch variant changed") };
    let Paint::Gradient { geom, .. } = &gradient else { panic!("gradient variant changed") };
    assert_eq!(h.matrix, [2.0, 0.0, 0.0, 1.0, 10.0, 7.0]);
    assert_eq!([h.matrix[4], h.matrix[5]], geom.start);
    let mut bitmapped = Paint::Bitmap(BitmapPaint { resource_id: "img".into(), matrix: translate, seed: None, single_image: false });
    transform_paint_matrix(&mut bitmapped, scale);
    let Paint::Bitmap(b) = &bitmapped else { panic!("bitmap variant changed") };
    assert_eq!(b.matrix, h.matrix);
}

#[test]
fn resource_collector_visits_tables_and_story_ranges() {
    let mut s = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
    s.run_named("table.create", json!({ "rows": 2, "cols": 2, "bounds": [40.0, 20.0], "pos": [0.0, 0.0] })).unwrap();
    if let ObjectKind::Table(table) = &mut s.document.objects[0].kind {
        table.cells[0].local.fill = Some(bitmap("cell-img"));
    } else {
        panic!("table.create made no table");
    }
    let mut rect = Object { name: "rect".into(), size: [10.0, 10.0], fill: bitmap("fill-img"), ..Object::default() };
    rect.stroke.paint = Paint::Hatch(hatch(IDENTITY));
    s.document.create_object(rect);
    let pattern = Paint::Pattern(PatternPaint { resource_id: "story-pat".into(), matrix: IDENTITY });
    s.document.stories.push(Story { id: StoryId(77), text: "ab".into(), ranges: vec![json!({ "start": 0, "end": 1, "fill": serde_json::to_value(&pattern).unwrap() })], specials: vec![], paras: vec![] });
    let found = collect_document_resources(&s.document);
    let bitmaps: Vec<&str> = found.iter().filter(|r| r.kind == PaintResourceKind::Bitmap).map(|r| r.id.as_str()).collect();
    assert_eq!(bitmaps.len(), 2);
    assert!(bitmaps.contains(&"cell-img") && bitmaps.contains(&"fill-img"));
    assert_eq!(found.iter().filter(|r| r.kind == PaintResourceKind::HatchSwatch).count(), 1);
    assert!(found.iter().any(|r| r.kind == PaintResourceKind::Pattern && r.id == "story-pat"));
    assert_eq!(found.len(), 4);
    assert_eq!(collect_document_resources(&s.document), found);
}

#[test]
fn hatch_resource_id_depends_on_content_not_only_matrix() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    let mut other = hatch(IDENTITY);
    other.lines[0].spacing = 9.0;
    doc.create_object(Object { fill: Paint::Hatch(hatch(IDENTITY)), ..Object::default() });
    doc.create_object(Object { fill: Paint::Hatch(other), ..Object::default() });
    assert_eq!(collect_document_resources(&doc).len(), 2);
}

#[test]
fn profile_convert_reports_skipped_and_unsupported_intent() {
    let mut s = Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1));
    let out = s.run_named("color.profile.convert", json!({ "format": dtool_core::colour::ColourFormat::LAB_16.to_string(), "profile": "LAB D50" })).unwrap();
    assert!(out["skipped"].is_u64());
    let err = s.run_named("color.profile.convert", json!({ "format": dtool_core::colour::ColourFormat::LAB_16.to_string(), "profile": "LAB D50", "intent": "sideways" })).unwrap_err();
    assert_eq!(err.code, "UNSUPPORTED_INTENT");
}