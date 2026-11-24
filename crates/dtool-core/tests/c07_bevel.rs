use dtool_core::document::Units;
use dtool_core::layer_style::raster::{effect_margin, render_object_region, render_region};
use dtool_core::layer_style::{default_effect, BevelType, EffectKind, EffectParams, LayerEffect};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{CanvasFlags, Document};

fn bevel(kind: BevelType, elevation: f32) -> LayerEffect {
    let mut effect = default_effect(EffectKind::BevelEmboss);
    effect.params = EffectParams::Light { radius: 4.0, depth: 1.0, soften: 0.0, azimuth: 135.0, elevation, bevel: kind, highlight: Colour::new(255, 255, 255, 255), shadow: Colour::new(0, 0, 0, 255) };
    effect
}

fn doc_with(effects: Vec<LayerEffect>) -> (Document, ObjectId) {
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    let id = doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, effects, ..Object::default() });
    (doc, id)
}

fn px(doc: &Document, x: f32, y: f32) -> [u8; 4] {
    let p = render_region(doc, &CanvasFlags::default(), &[], [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1);
    [p[0], p[1], p[2], p[3]]
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

fn row(kind: BevelType) -> Vec<[u8; 4]> {
    let (doc, _) = doc_with(vec![bevel(kind, 45.0)]);
    [8.5f32, 9.5, 10.5, 30.0, 49.5, 50.5, 51.5].iter().map(|x| px(&doc, *x, 30.0)).collect()
}

#[test]
fn inner_bevel_stays_inside_the_shape() {
    assert_eq!(row(BevelType::Inner), vec![CLEAR, CLEAR, WHITE, RED, BLACK, CLEAR, CLEAR]);
}

#[test]
fn outer_bevel_lights_and_shades_outside_only() {
    assert_eq!(row(BevelType::Outer), vec![CLEAR, WHITE, RED, RED, RED, BLACK, CLEAR]);
}

#[test]
fn emboss_splits_the_bevel_across_the_edge() {
    assert_eq!(row(BevelType::Emboss), vec![CLEAR, WHITE, WHITE, RED, BLACK, BLACK, CLEAR]);
}

#[test]
fn pillow_emboss_reverses_the_inner_edge() {
    assert_eq!(row(BevelType::Pillow), vec![CLEAR, WHITE, BLACK, RED, WHITE, BLACK, CLEAR]);
}

#[test]
fn elevation_widens_low_light_and_fades_overhead_light() {
    let at = |elevation: f32, x: f32| {
        let (doc, _) = doc_with(vec![bevel(BevelType::Inner, elevation)]);
        px(&doc, x, 30.0)
    };
    assert_eq!(at(45.0, 11.5), RED);
    assert_eq!(at(0.0, 11.5), WHITE);
    let faded = at(60.0, 49.5);
    assert!(faded[0] > 0 && faded[0] < 255, "{faded:?}");
    assert_eq!(at(90.0, 10.5), RED);
    assert_eq!(at(90.0, 49.5), RED);
}

#[test]
fn outer_types_reserve_margin_and_object_region_matches_full_render() {
    let flags = CanvasFlags::default();
    let bounds = [0.0, 0.0, 64.0, 64.0];
    for kind in [BevelType::Inner, BevelType::Outer, BevelType::Emboss, BevelType::Pillow] {
        let (doc, id) = doc_with(vec![bevel(kind, 45.0)]);
        let margin = effect_margin(&doc, id, &[]);
        assert_eq!(margin > 0.0, kind != BevelType::Inner, "{kind:?} {margin}");
        assert_eq!(render_object_region(&doc, &flags, &[], id, bounds, 64, 64), render_region(&doc, &flags, &[], bounds, 64, 64), "{kind:?}");
    }
}
