use dtool_core::document::{Document, Units};
use dtool_core::paint::{BitmapPaint, Paint};
use dtool_core::scene::{bitmap_source, rasterize_bitmap, rasterize_scene, Prim, Scene};
use dtool_core::{Object, ObjectId};

fn doc_with_texture(pixels: Vec<u8>, size: (u32, u32)) -> Document {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    doc.create_object(Object { name: "tex".into(), raster_data: Some(pixels), raster_size: Some(size), ..Object::default() });
    doc
}

fn square(side: f32) -> Vec<Vec<[f32; 2]>> {
    vec![vec![[0.0, 0.0], [side, 0.0], [side, side], [0.0, side]]]
}

fn pixel(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let o = (y * width + x) * 4;
    [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]]
}

#[test]
fn c07_red_bitmap_fill_paints_red_centre() {
    let doc = doc_with_texture([255, 0, 0, 255].repeat(4), (2, 2));
    let bitmap = BitmapPaint { resource_id: "tex".into(), matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], seed: None };
    let rgba = rasterize_bitmap(&doc, &bitmap, &square(10.0), [0.0, 0.0], [0.0, 0.0, 10.0, 10.0], [10, 10]).expect("bitmap raster");
    assert_eq!(pixel(&rgba, 10, 5, 5), [255, 0, 0, 255]);
}

#[test]
fn c07_bitmap_fill_uses_matrix_and_tiles() {
    let texels = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255]].concat();
    let doc = doc_with_texture(texels, (2, 2));
    let bitmap = BitmapPaint { resource_id: "tex".into(), matrix: [5.0, 0.0, 0.0, 5.0, 0.0, 0.0], seed: None };
    let rgba = rasterize_bitmap(&doc, &bitmap, &square(20.0), [0.0, 0.0], [0.0, 0.0, 20.0, 20.0], [20, 20]).expect("bitmap raster");
    assert_eq!(pixel(&rgba, 20, 2, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&rgba, 20, 7, 2), [0, 255, 0, 255]);
    assert_eq!(pixel(&rgba, 20, 2, 7), [0, 0, 255, 255]);
    assert_eq!(pixel(&rgba, 20, 7, 7), [255, 255, 255, 255]);
    assert_eq!(pixel(&rgba, 20, 12, 2), [255, 0, 0, 255]);
}

#[test]
fn c07_bitmap_fill_reaches_scene_raster_and_missing_resource_is_none() {
    let doc = doc_with_texture([255, 0, 0, 255].repeat(4), (2, 2));
    let scene = Scene {
        prims: vec![Prim::Fill {
            object: ObjectId(99),
            contours: square(10.0),
            origin: [0.0, 0.0],
            paint: Paint::Bitmap(BitmapPaint { resource_id: "tex".into(), matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], seed: None }),
            opacity: 1.0,
        }],
        ..Scene::default()
    };
    let pixels = rasterize_scene(&doc, &scene, [0.0, 0.0, 10.0, 10.0], 10, 10, None);
    assert_eq!(pixel(&pixels, 10, 5, 5)[..3], [255, 0, 0]);
    assert!(bitmap_source(&doc, "missing").is_none());
    let bitmap = BitmapPaint { resource_id: "missing".into(), matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], seed: None };
    assert!(rasterize_bitmap(&doc, &bitmap, &square(10.0), [0.0, 0.0], [0.0, 0.0, 10.0, 10.0], [10, 10]).is_none());
}