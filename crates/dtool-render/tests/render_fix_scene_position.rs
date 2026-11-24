use dtool_core::commands::colour_pick::{sample, PickMode};
use dtool_core::document::Units;
use dtool_core::object::Object;
use dtool_core::paint::{Colour, Paint};
use dtool_core::scene::rasterize_scene;
use dtool_core::view_state::ViewState;
use dtool_core::{CanvasFlags, Document};

fn red_doc() -> Document {
    assert!(dtool_render::scene::install());
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    doc
}

fn alpha_at(pixels: &[u8], width: u32, x: u32, y: u32) -> u8 {
    pixels[((y * width + x) * 4 + 3) as usize]
}

#[test]
fn one_pixel_windows_only_hit_the_shape() {
    let doc = red_doc();
    let scene = dtool_core::scene::build_scene(&doc, &CanvasFlags::default());
    let inside = rasterize_scene(&doc, &scene, [29.5, 29.5, 1.0, 1.0], 1, 1, None);
    assert_eq!(inside, vec![255, 0, 0, 255]);
    for (x, y) in [(55.0, 55.0), (150.0, 150.0), (5.0, 30.0)] {
        assert_eq!(rasterize_scene(&doc, &scene, [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1, None)[3], 0, "{x},{y}");
    }
    let core = sample(&doc, 55.0, 55.0, PickMode::Point, None, 1.0).unwrap();
    assert_eq!(core.a, 0);
    let core = sample(&doc, 30.0, 30.0, PickMode::Average3, None, 1.0).unwrap();
    assert_eq!([core.r, core.g, core.b, core.a], [255, 0, 0, 255]);
}

#[test]
fn full_page_raster_keeps_shape_position_and_size() {
    let doc = red_doc();
    let scene = dtool_core::scene::build_scene(&doc, &CanvasFlags::default());
    let pixels = rasterize_scene(&doc, &scene, [0.0, 0.0, 100.0, 100.0], 100, 100, None);
    let covered = pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
    assert_eq!(covered, 40 * 40);
    assert_eq!(alpha_at(&pixels, 100, 10, 10), 255);
    assert_eq!(alpha_at(&pixels, 100, 49, 49), 255);
    assert_eq!(alpha_at(&pixels, 100, 50, 50), 0);
    assert_eq!(alpha_at(&pixels, 100, 9, 30), 0);
}

#[test]
fn zoomed_view_maps_shape_through_the_view() {
    let doc = red_doc();
    let scene = dtool_core::scene::build_scene(&doc, &CanvasFlags::default());
    let view = ViewState { zoom: 2.0, ..ViewState::default() };
    let pixels = rasterize_scene(&doc, &scene, [0.0, 0.0, 120.0, 120.0], 120, 120, Some(&view));
    let covered = pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
    assert_eq!(covered, 80 * 80);
    assert_eq!(&pixels[((30 * 120 + 30) * 4) as usize..][..4], &[255, 0, 0, 255]);
    assert_eq!(alpha_at(&pixels, 120, 19, 50), 0);
    assert_eq!(alpha_at(&pixels, 120, 101, 50), 0);
}
