use dtool_core::blend::BlendMode;
use dtool_core::command::Session;
use dtool_core::document::Units;
use dtool_core::layer_style::raster::{object_effect_bounds, render_region};
use dtool_core::layer_style::{default_effect, EffectKind, EffectParams, LayerEffect};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{CanvasFlags, Document};
use dtool_export::DocumentExportExt;
use dtool_ui::app_canvas::AppCanvas;
use serde_json::json;

fn shadow(offset: f32, radius: f32) -> LayerEffect {
    let mut effect = default_effect(EffectKind::OuterShadow);
    effect.opacity = 1.0;
    effect.blend = BlendMode::Multiply;
    effect.params = EffectParams::Shadow { radius, radius_y: None, blur_angle: None, offset, angle: 135.0, intensity: 0.0, colour: Colour::new(0, 0, 255, 255), knock_out: true };
    effect
}

fn session(effects: Vec<LayerEffect>) -> (Session, ObjectId) {
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    doc.create_object(Object { pos: [0.0, 0.0], size: [120.0, 120.0], fill: Paint::Solid { colour: Colour::new(255, 255, 0, 255) }, ..Object::default() });
    let id = doc.create_object(Object { pos: [20.0, 20.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, effects, ..Object::default() });
    (Session::new(doc), id)
}

fn close(a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!((i16::from(*x) - i16::from(*y)).abs() <= 1, "byte {i}: {x} vs {y}");
    }
}

#[test]
fn canvas_effect_pixels_equal_the_shared_export_raster() {
    let (session, id) = session(vec![shadow(8.0, 4.0)]);
    let mut canvas = AppCanvas::new();
    assert!(AppCanvas::has_effects(&session, id));
    let (bounds, pixels) = canvas.effect_pixels(&session, id, [64, 64]).unwrap();
    assert_eq!(bounds, object_effect_bounds(&session.document, &CanvasFlags::default(), &[], id).unwrap());
    close(&pixels, &render_region(&session.document, &CanvasFlags::default(), &[], bounds, 64, 64));
    let centre = ((32 * 64 + 32) * 4) as usize;
    assert_eq!(&pixels[centre..centre + 4], &[255, 0, 0, 255]);
}

#[test]
fn canvas_effect_pixels_equal_export_render() {
    let (session, id) = session(vec![shadow(8.0, 4.0)]);
    let mut canvas = AppCanvas::new();
    let (bounds, pixels) = canvas.effect_pixels(&session, id, [64, 64]).unwrap();
    let export = session.document.render_rgba_region((bounds[0], bounds[1], bounds[2], bounds[3]), 64, 64);
    close(&pixels, &export);
}

#[test]
fn canvas_cache_misses_on_version_and_staging_only() {
    let (mut session, id) = session(vec![shadow(8.0, 0.0)]);
    let mut canvas = AppCanvas::new();
    let first = canvas.effect_pixels(&session, id, [32, 32]).unwrap();
    let again = canvas.effect_pixels(&session, id, [32, 32]).unwrap();
    assert_eq!(first, again);
    assert_eq!((canvas.effect_cache().hits, canvas.effect_cache().misses), (1, 1));
    session.run_named("effect.preview", json!({ "id": id.0, "index": 0, "effect": serde_json::to_value(shadow(16.0, 0.0)).unwrap() })).unwrap();
    let staged = canvas.effect_pixels(&session, id, [32, 32]).unwrap();
    assert_eq!((canvas.effect_cache().hits, canvas.effect_cache().misses), (1, 2));
    assert_ne!(staged.1, first.1);
    session.run_named("effect.cancel", json!({ "id": id.0 })).unwrap();
    let version = session.document.version;
    session.run_named("effect.update", json!({ "id": id.0, "index": 0, "opacity": 0.5 })).unwrap();
    assert_ne!(session.document.version, version);
    canvas.effect_pixels(&session, id, [32, 32]).unwrap();
    assert_eq!(canvas.effect_cache().misses, 3);
}

#[test]
fn objects_without_effects_keep_the_plain_canvas_path() {
    let (session, id) = session(Vec::new());
    assert!(!AppCanvas::has_effects(&session, id));
}
