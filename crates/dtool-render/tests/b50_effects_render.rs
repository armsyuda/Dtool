use dtool_core::blend::BlendMode;
use dtool_core::document::Units;
use dtool_core::layer_style::raster::render_region;
use dtool_core::layer_style::{default_effect, EffectKind, EffectParams, LayerEffect};
use dtool_core::object::Object;
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint};
use dtool_core::{CanvasFlags, Document};

fn doc_with(effects: Vec<LayerEffect>) -> Document {
    assert!(dtool_render::scene::install());
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, effects, ..Object::default() });
    doc
}

fn px(doc: &Document, x: f32, y: f32) -> [u8; 4] {
    let p = render_region(doc, &CanvasFlags::default(), &[], [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1);
    [p[0], p[1], p[2], p[3]]
}

fn region(doc: &Document) -> Vec<u8> {
    render_region(doc, &CanvasFlags::default(), &[], [0.0, 0.0, 60.0, 60.0], 60, 60)
}

fn gradient(kind: GradientKind, start: [f64; 2], end: [f64; 2], ratio: f64) -> Paint {
    Paint::Gradient {
        kind,
        stops: vec![GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 255) }, GradientStop { offset: 1.0, colour: Colour::new(255, 255, 255, 255) }],
        geom: GradientGeom { start, end, ratio, ..GradientGeom::default() },
    }
}

fn overlay(fill: Paint, blend: BlendMode, opacity: f32) -> LayerEffect {
    let mut effect = default_effect(EffectKind::GradientOverlay);
    effect.opacity = opacity;
    effect.blend = blend;
    effect.params = EffectParams::Overlay { fill };
    effect
}

fn linear(start: [f64; 2], end: [f64; 2]) -> Document {
    doc_with(vec![overlay(gradient(GradientKind::Linear, start, end, 1.0), BlendMode::Normal, 1.0)])
}

#[test]
fn linear_gradient_overlay_runs_along_its_angle() {
    let doc = linear([0.0, 20.0], [40.0, 20.0]);
    let (left, mid, right) = (px(&doc, 11.0, 30.0), px(&doc, 30.0, 30.0), px(&doc, 49.0, 30.0));
    assert!(left[0] < 20 && right[0] > 235 && left[0] < mid[0] && mid[0] < right[0], "{left:?} {mid:?} {right:?}");
    assert!((120..=135).contains(&mid[0]), "{mid:?}");
    assert_eq!(px(&doc, 30.0, 12.0), px(&doc, 30.0, 48.0));
    assert_eq!(px(&doc, 5.0, 30.0)[3], 0);
    let down = linear([20.0, 0.0], [20.0, 40.0]);
    assert!(px(&down, 30.0, 11.0)[0] < 20 && px(&down, 30.0, 49.0)[0] > 235);
    assert_eq!(px(&down, 12.0, 30.0), px(&down, 48.0, 30.0));
}

#[test]
fn gradient_overlay_scale_and_offset_move_the_ramp() {
    let half = linear([0.0, 20.0], [20.0, 20.0]);
    assert!(px(&half, 31.0, 30.0)[0] > 245, "{:?}", px(&half, 31.0, 30.0));
    let shifted = linear([20.0, 20.0], [40.0, 20.0]);
    assert!(px(&shifted, 29.0, 30.0)[0] < 10, "{:?}", px(&shifted, 29.0, 30.0));
    assert!(px(&shifted, 40.0, 30.0)[0] > 100 && px(&shifted, 40.0, 30.0)[0] < 160);
}

#[test]
fn radial_elliptical_and_conical_overlays_differ_from_linear() {
    let images: Vec<Vec<u8>> = [GradientKind::Linear, GradientKind::Radial, GradientKind::Elliptical, GradientKind::Conical]
        .into_iter()
        .map(|kind| region(&doc_with(vec![overlay(gradient(kind, [20.0, 20.0], [40.0, 20.0], 0.5), BlendMode::Normal, 1.0)])))
        .collect();
    for (a, image) in images.iter().enumerate() {
        for other in &images[a + 1..] {
            assert_ne!(image, other);
        }
        assert!(image.chunks_exact(4).all(|p| p[3] == 0 || (p[0] == p[1] && p[1] == p[2])));
    }
    let radial = doc_with(vec![overlay(gradient(GradientKind::Radial, [20.0, 20.0], [40.0, 20.0], 1.0), BlendMode::Normal, 1.0)]);
    let centre = px(&radial, 30.0, 30.0);
    assert!(centre[0] < 20, "{centre:?}");
    assert_eq!(px(&radial, 40.0, 30.0)[0], px(&radial, 30.0, 40.0)[0]);
    assert!(px(&radial, 11.0, 11.0)[0] > 235);
}

#[test]
fn overlay_blend_mode_and_opacity_apply_inside_the_effect() {
    let blue = Paint::Solid { colour: Colour::new(0, 0, 255, 255) };
    let mut effect = default_effect(EffectKind::ColourOverlay);
    effect.params = EffectParams::Overlay { fill: blue.clone() };
    effect.blend = BlendMode::Normal;
    effect.opacity = 0.5;
    let half = px(&doc_with(vec![effect.clone()]), 30.0, 30.0);
    assert!((120..=135).contains(&half[0]) && (120..=135).contains(&half[2]) && half[3] == 255, "{half:?}");
    effect.opacity = 1.0;
    effect.blend = BlendMode::Multiply;
    assert_eq!(px(&doc_with(vec![effect]), 30.0, 30.0), [0, 0, 0, 255]);
}