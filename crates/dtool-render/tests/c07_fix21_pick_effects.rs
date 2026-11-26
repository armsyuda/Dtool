use dtool_core::blend::BlendMode;
use dtool_core::command::Session;
use dtool_core::commands::colour_pick::{sample, PickMode};
use dtool_core::commands::layer_style::StagedEffect;
use dtool_core::document::Units;
use dtool_core::layer_style::{default_effect, EffectKind, EffectParams, LayerEffect, OutlineAlign};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::Document;
use dtool_render::pick_sampler::{live_effects, pick, pick_live, rasterize_with_effects};
use serde_json::json;

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: Colour::new(r, g, b, 255) }
}

fn red_doc() -> (Document, ObjectId) {
    assert!(dtool_render::scene::install());
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    let id = doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill: solid(255, 0, 0), ..Object::default() });
    (doc, id)
}

fn shadow(offset: f32, radius: f32, colour: Colour, blend: BlendMode) -> LayerEffect {
    let mut effect = default_effect(EffectKind::OuterShadow);
    effect.opacity = 1.0;
    effect.blend = blend;
    effect.params = EffectParams::Shadow { radius, radius_y: None, blur_angle: None, offset, angle: 135.0, intensity: 0.0, colour, knock_out: true };
    effect
}

fn blue_shadow() -> LayerEffect {
    shadow(10.0, 0.0, Colour::new(0, 0, 255, 255), BlendMode::Normal)
}

fn overlay(r: u8, g: u8, b: u8) -> LayerEffect {
    let mut effect = default_effect(EffectKind::ColourOverlay);
    effect.params = EffectParams::Overlay { fill: solid(r, g, b) };
    effect
}

fn outline(width: f32, align: OutlineAlign) -> LayerEffect {
    let mut effect = default_effect(EffectKind::Outline);
    effect.params = EffectParams::Outline { width, align, fill: solid(255, 200, 0) };
    effect
}

fn staged(object: ObjectId, effect: LayerEffect) -> Vec<StagedEffect> {
    vec![StagedEffect { object, index: None, effect }]
}

fn rgba(c: Colour) -> [u8; 4] {
    [c.r, c.g, c.b, c.a]
}

fn at(doc: &Document, x: f32, y: f32, live: &[StagedEffect]) -> [u8; 4] {
    rgba(pick_live(doc, &Default::default(), x, y, PickMode::Point, None, live).unwrap())
}

#[test]
fn staged_outer_shadow_colour_is_picked_where_only_the_shadow_is_visible() {
    let (doc, id) = red_doc();
    let before = serde_json::to_string(&doc).unwrap();
    let live = staged(id, blue_shadow());
    assert_ne!(at(&doc, 55.0, 55.0, &[])[..3], [0, 0, 255]);
    assert_eq!(at(&doc, 55.0, 55.0, &live), [0, 0, 255, 255]);
    assert_eq!(at(&doc, 30.0, 30.0, &live), [255, 0, 0, 255]);
    let averaged = pick_live(&doc, &Default::default(), 55.0, 55.0, PickMode::Average3, None, &live).unwrap();
    assert_eq!([averaged.r, averaged.g, averaged.b], [0, 0, 255]);
    assert_eq!(serde_json::to_string(&doc).unwrap(), before);
}

#[test]
fn staged_overlay_and_outline_change_the_picked_colour() {
    let (doc, id) = red_doc();
    assert_eq!(at(&doc, 30.0, 30.0, &staged(id, overlay(0, 160, 0))), [0, 160, 0, 255]);
    assert_eq!(at(&doc, 52.0, 30.0, &staged(id, outline(4.0, OutlineAlign::Outside))), [255, 200, 0, 255]);
    assert_eq!(at(&doc, 30.0, 30.0, &staged(id, outline(4.0, OutlineAlign::Outside))), [255, 0, 0, 255]);
    assert_eq!(at(&doc, 49.0, 30.0, &staged(id, outline(4.0, OutlineAlign::Centre))), [255, 200, 0, 255]);
}

#[test]
fn outline_inside_stays_within_the_shape() {
    let (doc, id) = red_doc();
    let live = staged(id, outline(4.0, OutlineAlign::Inside));
    assert_eq!(at(&doc, 48.0, 30.0, &live), [255, 200, 0, 255]);
    assert_eq!(at(&doc, 12.0, 30.0, &live), [255, 200, 0, 255]);
    assert_eq!(at(&doc, 30.0, 30.0, &live), [255, 0, 0, 255]);
    assert_eq!(at(&doc, 52.0, 30.0, &live)[3], 0);
}

#[test]
fn multiply_shadow_blends_with_what_is_underneath() {
    assert!(dtool_render::scene::install());
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    doc.create_object(Object { pos: [0.0, 0.0], size: [100.0, 100.0], fill: solid(255, 255, 0), ..Object::default() });
    let id = doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill: solid(255, 0, 0), ..Object::default() });
    let cyan = Colour::new(0, 255, 255, 255);
    assert_eq!(at(&doc, 55.0, 55.0, &staged(id, shadow(10.0, 0.0, cyan.clone(), BlendMode::Multiply))), [0, 255, 0, 255]);
    assert_eq!(at(&doc, 55.0, 55.0, &staged(id, shadow(10.0, 0.0, cyan, BlendMode::Normal))), [0, 255, 255, 255]);
    assert_eq!(at(&doc, 80.0, 80.0, &staged(id, blue_shadow())), [255, 255, 0, 255]);
}

#[test]
fn shadow_blur_softens_the_edge() {
    let (doc, id) = red_doc();
    let sharp = staged(id, blue_shadow());
    let soft = staged(id, shadow(10.0, 8.0, Colour::new(0, 0, 255, 255), BlendMode::Normal));
    assert_eq!(at(&doc, 59.0, 40.0, &sharp)[3], 0);
    let outside = at(&doc, 59.0, 40.0, &soft);
    assert!(outside[3] > 0 && outside[3] < 128, "{outside:?}");
    assert_eq!(outside[2], 255);
    let edge = at(&doc, 56.5, 40.0, &soft);
    assert!(edge[3] > outside[3] && edge[3] < 255, "{edge:?}");
    assert_eq!(at(&doc, 35.0, 52.0, &sharp), [0, 0, 255, 255]);
    let core = at(&doc, 35.0, 52.0, &soft);
    assert!(core[3] > 200 && core[2] == 255, "{core:?}");
}

#[test]
fn pick_matches_the_effect_raster_pixel_for_pixel() {
    let (doc, id) = red_doc();
    let mut live = staged(id, shadow(10.0, 6.0, Colour::new(0, 0, 255, 255), BlendMode::Multiply));
    live.push(StagedEffect { object: id, index: None, effect: outline(3.0, OutlineAlign::Inside) });
    let raster = rasterize_with_effects(&doc, &Default::default(), &live, [40.0, 40.0, 24.0, 24.0], 24, 24);
    for py in (0..24).step_by(3) {
        for px in (0..24).step_by(3) {
            let picked = at(&doc, 40.5 + px as f32, 40.5 + py as f32, &live);
            let o = ((py * 24 + px) * 4) as usize;
            let drawn = &raster[o..o + 4];
            for c in 0..4 {
                assert!((i16::from(picked[c]) - i16::from(drawn[c])).abs() <= 1, "{px},{py} {picked:?} {drawn:?}");
            }
        }
    }
}

#[test]
fn disabled_and_foreign_staged_effects_are_ignored() {
    let (doc, id) = red_doc();
    let plain = at(&doc, 55.0, 55.0, &[]);
    let mut off = blue_shadow();
    off.enabled = false;
    assert_eq!(at(&doc, 55.0, 55.0, &staged(id, off)), plain);
    assert_eq!(at(&doc, 55.0, 55.0, &staged(ObjectId(id.0 + 99), blue_shadow())), plain);
    assert_eq!(at(&doc, 55.0, 55.0, &[StagedEffect { object: id, index: Some(0), effect: blue_shadow() }]), plain);
    assert!(live_effects(&doc, id, &[]).is_empty());
}

#[test]
fn document_without_effects_matches_the_existing_sampler() {
    let (doc, _) = red_doc();
    for mode in [PickMode::Point, PickMode::Average3, PickMode::Average5] {
        for (x, y) in [(30.0, 30.0), (50.0, 30.0), (150.0, 150.0)] {
            let live = pick_live(&doc, &Default::default(), x, y, mode, None, &[]).unwrap();
            let ours = pick(&doc, x, y, mode, None).unwrap();
            let core = sample(&doc, x, y, mode, None, 1.0).unwrap();
            assert_eq!(rgba(live), rgba(ours));
            assert_eq!(rgba(ours), rgba(core));
        }
    }
    assert!(pick_live(&doc, &Default::default(), f32::NAN, 1.0, PickMode::Point, None, &[]).is_none());
}

#[test]
fn pick_with_apply_false_does_not_bump_document_version() {
    let path = std::env::temp_dir().join(format!("c07_fix21_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    dtool_core::palette::set_app_path(Some(path));
    let (doc, id) = red_doc();
    let mut session = Session::new(doc);
    let (version, generation) = (session.document.version, session.generation);
    let reply = session.run_named("color.pick", json!({ "x": 30.0, "y": 30.0, "apply": false })).unwrap();
    assert_eq!(reply["applied"], json!(false));
    session.run_named("effect.preview", json!({ "id": id.0, "effect": serde_json::to_value(blue_shadow()).unwrap() })).unwrap();
    let live: Vec<StagedEffect> = dtool_core::commands::layer_style::staged(&session, id).into_iter().collect();
    assert_eq!(live.len(), 1);
    assert_eq!(at(&session.document, 55.0, 55.0, &live), [0, 0, 255, 255]);
    assert_eq!((session.document.version, session.generation), (version, generation));
}

#[test]
fn applied_document_effect_is_picked() {
    let (mut doc, id) = red_doc();
    doc.objects.iter_mut().find(|o| o.id == id).unwrap().effects = vec![blue_shadow()];
    assert_eq!(live_effects(&doc, id, &[]).len(), 1);
    assert_eq!(rgba(pick(&doc, 55.0, 55.0, PickMode::Point, None).unwrap()), [0, 0, 255, 255]);
    let mut green = blue_shadow();
    green.params = EffectParams::Shadow { radius_y: None, blur_angle: None, radius: 0.0, offset: 10.0, angle: 135.0, intensity: 0.0, colour: Colour::new(0, 120, 0, 255), knock_out: true };
    assert_eq!(at(&doc, 55.0, 55.0, &[StagedEffect { object: id, index: Some(0), effect: green }]), [0, 120, 0, 255]);
}

#[test]
fn pick_matches_shared_render_for_new_effect_kinds() {
    let (mut doc, id) = red_doc();
    let mut inner = shadow(5.0, 2.0, Colour::new(0, 0, 255, 255), BlendMode::Multiply);
    inner.kind = EffectKind::InnerShadow;
    let mut glow = default_effect(EffectKind::InnerGlow);
    glow.params = EffectParams::Glow { radius: 3.0, intensity: 0.3, colour: Colour::new(0, 255, 0, 255), from_centre: false };
    let bevel = default_effect(EffectKind::BevelEmboss);
    let mut blur = default_effect(EffectKind::GaussianBlur);
    blur.params = EffectParams::Blur { radius: 2.0, radius_y: None, angle: None, preserve_alpha: false };
    doc.objects.iter_mut().find(|o| o.id == id).unwrap().effects = vec![shadow(8.0, 3.0, Colour::new(0, 0, 0, 255), BlendMode::Multiply), inner, glow, bevel, blur];
    let flags = Default::default();
    for (x, y) in [(11.0f32, 30.0f32), (30.0, 11.5), (49.0, 49.0), (30.0, 30.0), (52.0, 52.0), (56.0, 40.0)] {
        let picked = at(&doc, x, y, &[]);
        let drawn = dtool_core::layer_style::raster::render_region(&doc, &flags, &[], [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1);
        for c in 0..4 {
            assert!((i16::from(picked[c]) - i16::from(drawn[c])).abs() <= 1, "{x},{y} {picked:?} {drawn:?}");
        }
    }
}
