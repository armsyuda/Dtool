use dtool_core::colour::{ColourModel, ConvertCtx, ProfileStore, RenderingIntent};
use dtool_core::paint::{display_paint, icc_display_cache_len, Colour, DisplayColourCtx, GradientGeom, GradientKind, GradientStop, Hatch, MeshPaint, Paint};
use dtool_core::scene::Prim;
use dtool_core::{CanvasFlags, Document, Object, ObjectId};
use dtool_render::scene::build_scene;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Colour {
    Colour::from_managed(dtool_core::colour::Colour::cmyk(c, m, y, k, 1.0))
}

fn expected(colour: &Colour, profile: &str) -> (u8, u8, u8) {
    let services = dtool_core::colour::services().read().unwrap();
    let ctx = ConvertCtx { from: profile.to_string(), to: ProfileStore::SRGB.to_string(), intent: services.preferences.intent, bpc: services.preferences.bpc };
    let out = dtool_core::colour::convert::convert(&colour.managed(), ColourModel::Rgb, &ctx, &services.profiles).unwrap();
    let to_u8 = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    (to_u8(out.values[0]), to_u8(out.values[1]), to_u8(out.values[2]))
}

fn rgb(colour: Colour) -> (u8, u8, u8) {
    (colour.r, colour.g, colour.b)
}

fn doc_with(objects: Vec<Object>) -> (Document, Vec<ObjectId>) {
    let mut doc = Document::new([200.0, 200.0], dtool_core::document::Units::Px, 72.0, 1);
    let ids = objects.into_iter().map(|object| doc.create_object(object)).collect();
    (doc, ids)
}

fn fill_of(doc: &Document, id: ObjectId) -> Paint {
    build_scene(doc, &CanvasFlags::default()).prims.into_iter().find_map(|prim| match prim {
        Prim::Fill { object, paint, .. } if object == id => Some(paint),
        _ => None,
    }).unwrap_or_else(|| panic!("no fill prim for {id:?}"))
}

fn stroke_of(doc: &Document, id: ObjectId) -> Paint {
    build_scene(doc, &CanvasFlags::default()).prims.into_iter().find_map(|prim| match prim {
        Prim::Stroke { object, paint, .. } if object == id => Some(paint),
        _ => None,
    }).unwrap_or_else(|| panic!("no stroke prim for {id:?}"))
}

fn solid(paint: Paint) -> Colour {
    match paint {
        Paint::Solid { colour } => colour,
        other => panic!("{other:?}"),
    }
}

fn boxed(fill: Paint) -> Object {
    Object { name: "box".into(), size: [20.0, 20.0], fill, ..Object::default() }
}

#[test]
fn c07_b10_no_profile_cmyk_uses_document_then_app_default_profile() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let colour = cmyk(10.0, 70.0, 30.0, 5.0);
    let (rgb_doc, ids) = doc_with(vec![boxed(Paint::Solid { colour })]);
    let app_default = dtool_core::colour::services().read().unwrap().preferences.cmyk.clone();
    assert_eq!(rgb(solid(fill_of(&rgb_doc, ids[0]))), expected(&colour, &app_default));
    assert_ne!(rgb(solid(fill_of(&rgb_doc, ids[0]))), rgb(colour), "naive CMYK preview must not reach the canvas");
    let mut cmyk_doc = rgb_doc.clone();
    cmyk_doc.colour.format.model = ColourModel::Cmyk;
    let has_swop = dtool_core::colour::services().read().unwrap().profiles.find(ProfileStore::SWOP_V2).is_some();
    let doc_profile = if has_swop { ProfileStore::SWOP_V2 } else { ProfileStore::FOGRA39L };
    cmyk_doc.colour.profile = doc_profile.to_string();
    assert_eq!(rgb(solid(fill_of(&cmyk_doc, ids[0]))), expected(&colour, doc_profile));
    if has_swop {
        assert_ne!(rgb(solid(fill_of(&cmyk_doc, ids[0]))), rgb(solid(fill_of(&rgb_doc, ids[0]))), "document CMYK profile wins over app default");
    }
    assert_eq!(DisplayColourCtx::for_document(&rgb_doc).cmyk, dtool_core::colour::ColourFormat { model: ColourModel::Cmyk, depth: dtool_core::colour::BitDepth::U8 }.default_profile(&dtool_core::colour::services().read().unwrap().preferences));
    assert_eq!(DisplayColourCtx::for_document(&cmyk_doc).cmyk, doc_profile);
}

#[test]
fn c07_b10_stroke_gradient_hatch_mesh_are_converted_rgb_unchanged() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let a = cmyk(0.0, 90.0, 80.0, 0.0);
    let b = cmyk(80.0, 0.0, 20.0, 10.0);
    let fogra = dtool_core::colour::services().read().unwrap().preferences.cmyk.clone();
    let mut stroked = boxed(Paint::Solid { colour: Colour::new(10, 200, 30, 255) });
    stroked.stroke.paint = Paint::Solid { colour: a };
    stroked.stroke.width = 3.0;
    let gradient = Paint::Gradient { kind: GradientKind::Linear, stops: vec![GradientStop { offset: 0.0, colour: a }, GradientStop { offset: 1.0, colour: Colour::new(0, 0, 255, 255) }], geom: GradientGeom { start: [0.0, 0.0], end: [20.0, 0.0], ratio: 1.0 } };
    let (doc, ids) = doc_with(vec![stroked, boxed(gradient)]);
    assert_eq!(rgb(solid(fill_of(&doc, ids[0]))), (10, 200, 30), "RGB fill unchanged");
    assert_eq!(rgb(solid(stroke_of(&doc, ids[0]))), expected(&a, &fogra));
    match fill_of(&doc, ids[1]) {
        Paint::Gradient { stops, .. } => {
            assert_eq!(rgb(stops[0].colour), expected(&a, &fogra));
            assert_eq!(rgb(stops[1].colour), (0, 0, 255));
        }
        other => panic!("{other:?}"),
    }
    let ctx = DisplayColourCtx::for_document(&doc);
    let hatch = Hatch { lines: Vec::new(), colour: a, background: Some(b), weight: 1.0, units: dtool_core::paint::HatchUnits::Document, matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };
    match display_paint(&Paint::Hatch(hatch), None, &ctx) {
        Paint::Hatch(out) => {
            assert_eq!(rgb(out.colour), expected(&a, &fogra));
            assert_eq!(rgb(out.background.unwrap()), expected(&b, &fogra));
        }
        other => panic!("{other:?}"),
    }
    let mut mesh = MeshPaint::new();
    mesh.colours = vec![a, Colour::new(1, 2, 3, 255), b];
    match display_paint(&Paint::Mesh(mesh), None, &ctx) {
        Paint::Mesh(out) => assert_eq!(out.colours.into_iter().map(rgb).collect::<Vec<_>>(), vec![expected(&a, &fogra), (1, 2, 3), expected(&b, &fogra)]),
        other => panic!("{other:?}"),
    }
    let icc = dtool_core::colour::model::ObjectIcc { profile: None, intent: Some(RenderingIntent::Perceptual) };
    assert_eq!(display_paint(&Paint::Solid { colour: Colour::new(9, 8, 7, 128) }, Some(&icc), &ctx), Paint::Solid { colour: Colour::new(9, 8, 7, 128) });
    assert_eq!(doc.get_object(ids[0]).unwrap().stroke.paint, Paint::Solid { colour: a }, "stored paint untouched");
}

#[test]
fn c07_b10_display_conversion_is_cached() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let colour = cmyk(33.0, 11.0, 77.0, 3.0);
    let objects = (0..50).map(|_| boxed(Paint::Solid { colour })).collect();
    let (doc, _) = doc_with(objects);
    build_scene(&doc, &CanvasFlags::default());
    let after_first = icc_display_cache_len();
    let started = std::time::Instant::now();
    for _ in 0..100 {
        build_scene(&doc, &CanvasFlags::default());
    }
    let elapsed = started.elapsed();
    assert_eq!(icc_display_cache_len(), after_first, "repeat builds hit the cache");
    eprintln!("100 scene builds x 50 CMYK objects: {elapsed:?}");
}