use dtool_core::blend::BlendMode;
use dtool_core::document::Units;
use dtool_core::layer_style::raster::render_region;
use dtool_core::layer_style::{default_effect, BevelType, EffectKind, EffectParams, LayerEffect, OutlineAlign};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{CanvasFlags, Document};

const RED: [u8; 4] = [255, 0, 0, 255];
fn blue() -> Colour {
    Colour::new(0, 0, 255, 255)
}

fn green() -> Colour {
    Colour::new(0, 255, 0, 255)
}

fn doc_with(effects: Vec<LayerEffect>) -> (Document, ObjectId) {
    doc_fill(Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, effects)
}

fn doc_fill(fill: Paint, effects: Vec<LayerEffect>) -> (Document, ObjectId) {
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    let id = doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill, effects, ..Object::default() });
    (doc, id)
}

fn px(doc: &Document, x: f32, y: f32) -> [u8; 4] {
    let p = render_region(doc, &CanvasFlags::default(), &[], [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1);
    [p[0], p[1], p[2], p[3]]
}

fn region(doc: &Document) -> Vec<u8> {
    render_region(doc, &CanvasFlags::default(), &[], [0.0, 0.0, 80.0, 80.0], 80, 80)
}

fn normal(kind: EffectKind, params: EffectParams) -> LayerEffect {
    let mut effect = default_effect(kind);
    effect.opacity = 1.0;
    effect.blend = BlendMode::Normal;
    effect.params = params;
    effect
}

fn shadow(kind: EffectKind, offset: f32, angle: f32, radius: f32) -> LayerEffect {
    normal(kind, EffectParams::Shadow { radius, offset, angle, intensity: 0.0, colour: blue(), knock_out: true })
}

fn outline(width: f32, align: OutlineAlign) -> LayerEffect {
    normal(EffectKind::Outline, EffectParams::Outline { width, align, fill: Paint::Solid { colour: blue() } })
}

fn glow(kind: EffectKind, radius: f32, from_centre: bool) -> LayerEffect {
    normal(kind, EffectParams::Glow { radius, intensity: 0.0, colour: green(), from_centre })
}

fn overlay(kind: EffectKind, fill: Paint) -> LayerEffect {
    normal(kind, EffectParams::Overlay { fill })
}

fn light(kind: EffectKind, azimuth: f32, elevation: f32) -> LayerEffect {
    normal(kind, EffectParams::Light { radius: 4.0, depth: 1.0, soften: 0.0, azimuth, elevation, bevel: BevelType::Inner, highlight: Colour::new(255, 255, 255, 255), shadow: Colour::new(0, 0, 0, 255) })
}

#[test]
fn outer_shadow_offset_moves_the_shadow_by_the_offset_along_the_angle() {
    let (doc, _) = doc_with(vec![shadow(EffectKind::OuterShadow, 10.0, 180.0, 0.0)]);
    assert_eq!(px(&doc, 30.0, 30.0), RED);
    assert_eq!(px(&doc, 55.0, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&doc, 59.5, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&doc, 60.5, 30.0)[3], 0);
    assert_eq!(px(&doc, 15.0, 30.0), RED);
    assert_eq!(px(&doc, 5.0, 30.0)[3], 0);
    let (down, _) = doc_with(vec![shadow(EffectKind::OuterShadow, 10.0, 90.0, 0.0)]);
    assert_eq!(px(&down, 30.0, 59.5), [0, 0, 255, 255]);
    assert_eq!(px(&down, 30.0, 60.5)[3], 0);
    assert_eq!(px(&down, 55.0, 30.0)[3], 0);
}

#[test]
fn outer_shadow_without_knock_out_shows_under_a_transparent_fill() {
    let mut effect = shadow(EffectKind::OuterShadow, 10.0, 180.0, 0.0);
    effect.params = EffectParams::Shadow { radius: 0.0, offset: 10.0, angle: 180.0, intensity: 0.0, colour: blue(), knock_out: false };
    let (doc, _) = doc_fill(Paint::Solid { colour: Colour::new(255, 0, 0, 0) }, vec![effect.clone()]);
    assert_eq!(px(&doc, 30.0, 30.0), [0, 0, 255, 255]);
    effect.params = EffectParams::Shadow { radius: 0.0, offset: 10.0, angle: 180.0, intensity: 0.0, colour: blue(), knock_out: true };
    let (knocked, _) = doc_fill(Paint::Solid { colour: Colour::new(255, 0, 0, 0) }, vec![effect]);
    assert_eq!(px(&knocked, 30.0, 30.0)[3], 0);
    assert_eq!(px(&knocked, 55.0, 30.0), [0, 0, 255, 255]);
}

#[test]
fn inner_shadow_stays_inside_the_edge() {
    let (doc, _) = doc_with(vec![shadow(EffectKind::InnerShadow, 6.0, 0.0, 0.0)]);
    assert_eq!(px(&doc, 48.0, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&doc, 30.0, 30.0), RED);
    assert_eq!(px(&doc, 12.0, 30.0), RED);
    assert_eq!(px(&doc, 52.0, 30.0)[3], 0);
    assert_eq!(px(&doc, 8.0, 30.0)[3], 0);
}

#[test]
fn outer_glow_uses_its_colour_and_fades_over_its_radius() {
    let (doc, _) = doc_with(vec![glow(EffectKind::OuterGlow, 6.0, false)]);
    let near = px(&doc, 50.5, 30.0);
    let mid = px(&doc, 53.0, 30.0);
    let far = px(&doc, 70.0, 30.0);
    assert!(near[1] == 255 && near[0] == 0 && near[2] == 0, "{near:?}");
    assert!(near[3] > mid[3] && mid[3] > 0, "{near:?} {mid:?}");
    assert_eq!(far[3], 0);
    assert_eq!(px(&doc, 30.0, 30.0), RED);
    let (wide, _) = doc_with(vec![glow(EffectKind::OuterGlow, 12.0, false)]);
    assert!(px(&wide, 56.0, 30.0)[3] > px(&doc, 56.0, 30.0)[3]);
}

#[test]
fn inner_glow_from_centre_is_brightest_in_the_middle() {
    let (doc, _) = doc_with(vec![glow(EffectKind::InnerGlow, 8.0, true)]);
    let centre = px(&doc, 30.0, 30.0);
    let edge = px(&doc, 11.0, 30.0);
    assert!(centre[1] > edge[1], "{centre:?} {edge:?}");
    assert_eq!(centre[3], 255);
    assert_eq!(px(&doc, 8.0, 30.0)[3], 0);
    let (rim, _) = doc_with(vec![glow(EffectKind::InnerGlow, 8.0, false)]);
    assert!(px(&rim, 11.0, 30.0)[1] > px(&rim, 30.0, 30.0)[1]);
}

#[test]
fn outline_outside_centre_and_inside_cover_their_widths() {
    let (outside, _) = doc_with(vec![outline(4.0, OutlineAlign::Outside)]);
    assert_eq!(px(&outside, 51.0, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&outside, 53.5, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&outside, 55.0, 30.0)[3], 0);
    assert_eq!(px(&outside, 48.5, 30.0), RED);
    let (centre, _) = doc_with(vec![outline(4.0, OutlineAlign::Centre)]);
    assert_eq!(px(&centre, 48.5, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&centre, 51.5, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&centre, 53.0, 30.0)[3], 0);
    assert_eq!(px(&centre, 46.5, 30.0), RED);
    let (inside, _) = doc_with(vec![outline(4.0, OutlineAlign::Inside)]);
    assert_eq!(px(&inside, 46.5, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&inside, 49.5, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&inside, 45.0, 30.0), RED);
    assert_eq!(px(&inside, 51.0, 30.0)[3], 0);
}

#[test]
fn three_d_lights_the_azimuth_side_and_flat_elevation_does_nothing() {
    let (doc, _) = doc_with(vec![light(EffectKind::ThreeD, 135.0, 45.0)]);
    assert_eq!(px(&doc, 11.0, 30.0), [255, 255, 255, 255]);
    assert_eq!(px(&doc, 49.0, 30.0), [0, 0, 0, 255]);
    assert_eq!(px(&doc, 30.0, 30.0), RED);
    let (opposite, _) = doc_with(vec![light(EffectKind::ThreeD, 315.0, 45.0)]);
    assert_eq!(px(&opposite, 49.0, 30.0), [255, 255, 255, 255]);
    assert_eq!(px(&opposite, 11.0, 30.0), [0, 0, 0, 255]);
    let (flat, _) = doc_with(vec![light(EffectKind::ThreeD, 135.0, 90.0)]);
    let (plain, _) = doc_with(Vec::new());
    assert_eq!(region(&flat), region(&plain));
}

#[test]
fn colour_overlay_recolours_every_visible_pixel() {
    let (doc, _) = doc_with(vec![overlay(EffectKind::ColourOverlay, Paint::Solid { colour: green() })]);
    let pixels = region(&doc);
    let (plain, _) = doc_with(Vec::new());
    let base = region(&plain);
    let mut seen = 0;
    for (p, b) in pixels.chunks_exact(4).zip(base.chunks_exact(4)) {
        assert_eq!(p[3], b[3]);
        if b[3] == 255 {
            assert_eq!(p, &[0, 255, 0, 255]);
            seen += 1;
        }
    }
    assert!(seen >= 38 * 38, "{seen}");
}

#[test]
fn gaussian_blur_radius_zero_changes_nothing() {
    let mut blur = default_effect(EffectKind::GaussianBlur);
    blur.params = EffectParams::Blur { radius: 0.0, preserve_alpha: false };
    let (doc, _) = doc_with(vec![blur]);
    let (plain, _) = doc_with(Vec::new());
    assert_eq!(region(&doc), region(&plain));
}

#[test]
fn effects_composite_in_spec_order_regardless_of_list_order() {
    let edge = [outline(4.0, OutlineAlign::Centre), overlay(EffectKind::ColourOverlay, Paint::Solid { colour: green() })];
    let (outline_first, _) = doc_with(edge.to_vec());
    let (overlay_first, _) = doc_with(edge.iter().rev().cloned().collect());
    assert_eq!(px(&outline_first, 48.5, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&outline_first, 30.0, 30.0), [0, 255, 0, 255]);
    assert_eq!(region(&outline_first), region(&overlay_first));
    let mut shade = shadow(EffectKind::OuterShadow, 4.0, 180.0, 0.0);
    shade.params = EffectParams::Shadow { radius: 0.0, offset: 4.0, angle: 180.0, intensity: 0.0, colour: blue(), knock_out: true };
    let below = [glow(EffectKind::OuterGlow, 6.0, false), shade];
    let (glow_first, _) = doc_with(below.to_vec());
    let (shadow_first, _) = doc_with(below.iter().rev().cloned().collect());
    assert_eq!(region(&glow_first), region(&shadow_first));
    let spot = px(&glow_first, 51.0, 30.0);
    assert!(spot[1] > 0, "{spot:?}");
}