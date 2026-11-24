use dtool_core::document::{Document, Units};
use dtool_core::object::{ClipRole, Object, ObjectId};
use dtool_core::paint::{Colour, Paint};

fn rect(id: u64, pos: [f32; 2], size: [f32; 2], c: [u8; 3]) -> Object {
    Object { id: ObjectId(id), name: format!("o{id}"), pos, size, fill: Paint::Solid { colour: Colour::new(c[0], c[1], c[2], 255) }, ..Object::default() }
}

fn png(doc: &Document) -> image::RgbaImage {
    let bytes = dtool_export::export_png(doc, None).expect("png export");
    image::load_from_memory(&bytes).unwrap().to_rgba8()
}

fn px(img: &image::RgbaImage, x: f32, y: f32) -> image::Rgba<u8> {
    let sx = img.width() as f32 / 80.0;
    let sy = img.height() as f32 / 80.0;
    *img.get_pixel((x * sx) as u32, (y * sy) as u32)
}

fn is(p: image::Rgba<u8>, c: [u8; 3]) -> bool {
    p[3] > 200 && (0..3).all(|i| (i32::from(p[i]) - i32::from(c[i])).abs() < 40)
}

fn doc() -> Document {
    let mut doc = Document::new([80.0, 80.0], Units::Pt, 72.0, 1);
    doc.objects.push(rect(1, [0.0, 0.0], [40.0, 40.0], [255, 0, 0]));
    doc.objects.push(rect(2, [20.0, 20.0], [40.0, 40.0], [0, 0, 255]));
    doc.next_object_id = 10;
    doc
}

#[test]
fn png_export_honours_clip_child_and_vector_mask() {
    let mut d = doc();
    let plain = png(&d);
    assert!(is(px(&plain, 50.0, 50.0), [0, 0, 255]));
    d.objects[1].parent_id = Some(ObjectId(1));
    d.objects[1].clip_role = ClipRole::Clip;
    let clipped = png(&d);
    assert!(is(px(&clipped, 30.0, 30.0), [0, 0, 255]));
    assert!(!is(px(&clipped, 50.0, 50.0), [0, 0, 255]), "clip child outside parent not exported");
    assert!(is(px(&clipped, 10.0, 10.0), [255, 0, 0]));
    let mut mask = rect(3, [0.0, 0.0], [20.0, 80.0], [0, 255, 0]);
    mask.parent_id = Some(ObjectId(1));
    mask.clip_role = ClipRole::Mask;
    d.objects.push(mask);
    let masked = png(&d);
    assert!(is(px(&masked, 10.0, 10.0), [255, 0, 0]));
    assert!(!is(px(&masked, 30.0, 10.0), [255, 0, 0]), "parent outside vector mask not exported");
    assert!(!is(px(&masked, 30.0, 30.0), [0, 0, 255]));
    assert!(!masked.pixels().any(|p| is(*p, [0, 255, 0])), "mask path not exported");
}