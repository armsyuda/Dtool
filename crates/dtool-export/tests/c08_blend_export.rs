//! B29-T007/T008: PNG export composites object blend modes and blend ranges (scene rasterizer hook).
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};

fn rect(id: u64, pos: [f32; 2], size: [f32; 2], c: [u8; 3]) -> Object {
    Object { id: ObjectId(id), name: format!("o{id}"), pos, size, fill: Paint::Solid { colour: Colour::new(c[0], c[1], c[2], 255) }, ..Object::default() }
}

fn px(doc: &Document, x: f32, y: f32) -> [u8; 3] {
    let img = image::load_from_memory(&dtool_export::export_png(doc, None).expect("png")).unwrap().to_rgba8();
    let p = img.get_pixel((x * img.width() as f32 / 80.0) as u32, (y * img.height() as f32 / 80.0) as u32);
    [p[0], p[1], p[2]]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= 8)
}

#[test]
fn png_export_honours_blend_mode_and_blend_ranges() {
    let mut doc = Document::new([80.0, 80.0], Units::Pt, 72.0, 1);
    doc.objects.push(rect(1, [0.0, 0.0], [40.0, 80.0], [0, 200, 0]));
    doc.objects.push(rect(2, [40.0, 0.0], [40.0, 80.0], [240, 240, 240]));
    doc.objects.push(rect(3, [0.0, 0.0], [80.0, 40.0], [255, 0, 0]));
    doc.next_object_id = 10;
    assert!(near(px(&doc, 20.0, 20.0), [255, 0, 0]), "normal");
    doc.objects[2].blend_mode = dtool_core::blend::BlendMode::Multiply;
    assert!(near(px(&doc, 20.0, 20.0), [0, 0, 0]), "multiply red x green: {:?}", px(&doc, 20.0, 20.0));
    assert!(near(px(&doc, 60.0, 20.0), [240, 0, 0]), "multiply red x light grey: {:?}", px(&doc, 60.0, 20.0));
    assert!(near(px(&doc, 20.0, 60.0), [0, 200, 0]), "outside untouched");
    doc.objects[2].blend_mode = dtool_core::blend::BlendMode::Normal;
    doc.objects[2].blend_ranges = Some(dtool_core::layer_style::BlendRanges { source: vec![[0.0, 1.0], [1.0, 1.0]], underlying: vec![[0.0, 0.0], [0.6, 0.0], [0.7, 1.0], [1.0, 1.0]], gamma: 1.0, antialias: false });
    assert!(near(px(&doc, 20.0, 20.0), [0, 200, 0]), "hidden over the darker green (lum ~0.56): {:?}", px(&doc, 20.0, 20.0));
    assert!(near(px(&doc, 60.0, 20.0), [255, 0, 0]), "shown over light grey: {:?}", px(&doc, 60.0, 20.0));
}