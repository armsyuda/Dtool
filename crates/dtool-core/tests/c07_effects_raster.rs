use dtool_core::blend::BlendMode;
use dtool_core::commands::layer_style::StagedEffect;
use dtool_core::document::Units;
use dtool_core::layer_style::raster::{render_object_region, render_region, EffectCache};
use dtool_core::layer_style::{default_effect, BevelType, EffectKind, EffectParams, LayerEffect};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{CanvasFlags, Document};

fn doc_with(opacity: f32, effects: Vec<LayerEffect>) -> (Document, ObjectId) {
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    let id = doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, opacity, effects, ..Object::default() });
    (doc, id)
}

fn px(doc: &Document, x: f32, y: f32) -> [u8; 4] {
    let p = render_region(doc, &CanvasFlags::default(), &[], [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1);
    [p[0], p[1], p[2], p[3]]
}

fn shadow(kind: EffectKind, offset: f32, radius: f32, intensity: f32, knock_out: bool) -> LayerEffect {
    let mut effect = default_effect(kind);
    effect.opacity = 1.0;
    effect.blend = BlendMode::Normal;
    effect.params = EffectParams::Shadow { radius, radius_y: None, blur_angle: None, offset, angle: 135.0, intensity, colour: Colour::new(0, 0, 255, 255), knock_out };
    effect
}

#[test]
fn inner_shadow_darkens_the_lit_edge_inside_only() {
    let (doc, _) = doc_with(1.0, vec![shadow(EffectKind::InnerShadow, 6.0, 0.0, 0.0, false)]);
    assert_eq!(px(&doc, 12.0, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&doc, 30.0, 12.0), [0, 0, 255, 255]);
    assert_eq!(px(&doc, 30.0, 30.0), [255, 0, 0, 255]);
    assert_eq!(px(&doc, 48.0, 30.0), [255, 0, 0, 255]);
    assert_eq!(px(&doc, 5.0, 30.0)[3], 0);
}

#[test]
fn inner_glow_tints_the_rim_and_leaves_the_centre() {
    let mut glow = default_effect(EffectKind::InnerGlow);
    glow.blend = BlendMode::Normal;
    glow.params = EffectParams::Glow { radius: 4.0, intensity: 0.0, colour: Colour::new(0, 255, 0, 255), from_centre: false };
    let (doc, _) = doc_with(1.0, vec![glow]);
    let rim = px(&doc, 10.5, 30.0);
    assert!(rim[1] > 100 && rim[0] < 200 && rim[3] == 255, "{rim:?}");
    assert_eq!(px(&doc, 30.0, 30.0), [255, 0, 0, 255]);
    assert_eq!(px(&doc, 8.0, 30.0)[3], 0);
}

#[test]
fn bevel_lights_the_azimuth_side_and_shades_the_other() {
    let mut bevel = default_effect(EffectKind::BevelEmboss);
    bevel.params = EffectParams::Light { radius: 4.0, depth: 1.0, soften: 0.0, azimuth: 135.0, elevation: 45.0, bevel: BevelType::Inner, highlight: Colour::new(255, 255, 255, 255), shadow: Colour::new(0, 0, 0, 255) };
    let (doc, _) = doc_with(1.0, vec![bevel]);
    assert_eq!(px(&doc, 11.0, 30.0), [255, 255, 255, 255]);
    assert_eq!(px(&doc, 49.0, 30.0), [0, 0, 0, 255]);
    assert_eq!(px(&doc, 30.0, 30.0), [255, 0, 0, 255]);
}

#[test]
fn object_gaussian_blur_spreads_past_the_edge_unless_alpha_is_preserved() {
    let mut blur = default_effect(EffectKind::GaussianBlur);
    blur.params = EffectParams::Blur { radius: 4.0, radius_y: None, angle: None, preserve_alpha: false };
    let (doc, _) = doc_with(1.0, vec![blur.clone()]);
    let outside = px(&doc, 51.0, 30.0);
    assert!(outside[3] > 0 && outside[3] < 255 && outside[0] == 255, "{outside:?}");
    assert_eq!(px(&doc, 30.0, 30.0), [255, 0, 0, 255]);
    let edge = px(&doc, 49.5, 30.0);
    assert!(edge[3] < 255, "{edge:?}");
    blur.params = EffectParams::Blur { radius: 4.0, radius_y: None, angle: None, preserve_alpha: true };
    let (kept, _) = doc_with(1.0, vec![blur]);
    assert_eq!(px(&kept, 51.0, 30.0)[3], 0);
}

#[test]
fn shadow_intensity_spreads_and_knockout_hides_the_shadow_under_the_object() {
    let (soft, _) = doc_with(1.0, vec![shadow(EffectKind::OuterShadow, 10.0, 8.0, 0.0, true)]);
    let (dense, _) = doc_with(1.0, vec![shadow(EffectKind::OuterShadow, 10.0, 8.0, 0.6, true)]);
    let a = px(&soft, 59.0, 40.0)[3];
    let b = px(&dense, 59.0, 40.0)[3];
    assert!(a > 0 && b > a, "{a} {b}");
    let (knocked, _) = doc_with(0.5, vec![shadow(EffectKind::OuterShadow, 6.0, 0.0, 0.0, true)]);
    let (shown, _) = doc_with(0.5, vec![shadow(EffectKind::OuterShadow, 6.0, 0.0, 0.0, false)]);
    let k = px(&knocked, 40.0, 40.0);
    let s = px(&shown, 40.0, 40.0);
    assert_eq!(k[2], 0);
    assert!(s[2] > 50, "{s:?}");
    assert_eq!(px(&knocked, 53.0, 53.0), [0, 0, 255, 128]);
}

#[test]
fn cache_hits_until_version_or_staged_effects_change() {
    let (mut doc, id) = doc_with(1.0, vec![shadow(EffectKind::OuterShadow, 6.0, 2.0, 0.0, true)]);
    let flags = CanvasFlags::default();
    let mut cache = EffectCache::default();
    let bounds = [0.0, 0.0, 80.0, 80.0];
    let first = cache.object_region(&doc, &flags, &[], id, bounds, [40, 40]).to_vec();
    let again = cache.object_region(&doc, &flags, &[], id, bounds, [40, 40]).to_vec();
    assert_eq!((cache.hits, cache.misses), (1, 1));
    assert_eq!(first, again);
    assert_eq!(first, render_object_region(&doc, &flags, &[], id, bounds, 40, 40));
    let live = vec![StagedEffect { object: id, index: Some(0), effect: shadow(EffectKind::OuterShadow, 12.0, 0.0, 0.0, true) }];
    let staged = cache.object_region(&doc, &flags, &live, id, bounds, [40, 40]).to_vec();
    assert_eq!((cache.hits, cache.misses), (1, 2));
    assert_ne!(staged, first);
    doc.version += 1;
    cache.object_region(&doc, &flags, &[], id, bounds, [40, 40]);
    assert_eq!((cache.hits, cache.misses), (1, 3));
    assert_eq!(cache.len(), 1);
}

#[test]
fn single_object_region_matches_full_render() {
    let (doc, id) = doc_with(1.0, vec![shadow(EffectKind::OuterShadow, 6.0, 3.0, 0.2, true), shadow(EffectKind::InnerShadow, 3.0, 1.0, 0.0, false)]);
    let flags = CanvasFlags::default();
    let bounds = [0.0, 0.0, 70.0, 70.0];
    assert_eq!(render_object_region(&doc, &flags, &[], id, bounds, 35, 35), render_region(&doc, &flags, &[], bounds, 35, 35));
}
