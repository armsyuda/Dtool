use dtool_core::document::{Document, Units};
use dtool_core::paint::BitmapPaint;
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_core::scene::{bitmap_source, rasterize_bitmap};
use dtool_core::Object;

fn square(side: f32) -> Vec<Vec<[f32; 2]>> {
    vec![vec![[0.0, 0.0], [side, 0.0], [side, side], [0.0, side]]]
}

fn centre(doc: &Document, id: &str) -> Option<[u8; 4]> {
    let bitmap = BitmapPaint { resource_id: id.into(), matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], seed: None };
    let rgba = rasterize_bitmap(doc, &bitmap, &square(10.0), [0.0, 0.0], [0.0, 0.0, 10.0, 10.0], [10, 10])?;
    let o = (5 * 10 + 5) * 4;
    Some([rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]])
}

#[test]
fn c07_bitmap_fill_reads_embedded_image_resource_by_id_and_name() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    let mut resource = ImageResource::new_embedded(ImageFormat::Png, [2, 2], 72.0, [255, 0, 0, 255].repeat(4));
    resource.name = "red.png".into();
    let id = doc.add_image_resource(resource);
    assert_eq!(centre(&doc, &id.0.to_string()), Some([255, 0, 0, 255]));
    assert_eq!(centre(&doc, "red.png"), Some([255, 0, 0, 255]));
    assert!(bitmap_source(&doc, "999").is_none());
}

#[test]
fn c07_bitmap_fill_prefers_placed_pixels_of_the_resource() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    let id = doc.add_image_resource(ImageResource::new_embedded(ImageFormat::Png, [2, 2], 72.0, vec![1, 2, 3]));
    let object = doc.create_object(Object { name: "photo".into(), raster_data: Some([0, 0, 255, 255].repeat(4)), raster_size: Some((2, 2)), ..Object::default() });
    doc.place_image(object, id).unwrap();
    assert_eq!(centre(&doc, &id.0.to_string()), Some([0, 0, 255, 255]));
}

#[test]
fn c07_bitmap_fill_ignores_encoded_resource_bytes_without_pixels() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    let id = doc.add_image_resource(ImageResource::new_embedded(ImageFormat::Png, [2, 2], 72.0, vec![137, 80, 78, 71]));
    assert!(centre(&doc, &id.0.to_string()).is_none());
}
