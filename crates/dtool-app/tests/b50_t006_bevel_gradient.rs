use std::sync::Arc;
use dtool_core::commands::colour_pick::PickMode;
use dtool_core::document::Units;
use dtool_core::layer_style::raster::{render_region, render_region_scaled};
use dtool_core::layer_style::{default_effect, BevelType, EffectKind, EffectParams, LayerEffect};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint};
use dtool_core::serde_json::{self, json, Value};
use dtool_core::{CanvasFlags, Command, ContextId, Document, Scope, SharedServices, Workspace};
use dtool_ui::app_canvas::AppCanvas;

const W: [u8; 4] = [255, 255, 255, 255];
const K: [u8; 4] = [0, 0, 0, 255];
const G: [u8; 4] = [128, 128, 128, 255];
const C: [u8; 4] = [0, 0, 0, 0];

fn live() -> (Workspace, ContextId, ObjectId) {
    assert!(dtool_render::scene::install());
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let ctx = ws.new_document("fx");
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    let id = doc.create_object(Object { pos: [40.0, 40.0], size: [80.0, 80.0], fill: Paint::Solid { colour: Colour::new(128, 128, 128, 255) }, ..Object::default() });
    ws.get_mut(ctx).unwrap().session = dtool_core::Session::new(doc);
    (ws, ctx, id)
}

fn run(ws: &mut Workspace, ctx: ContextId, name: &str, params: Value) -> Value {
    ws.run(Scope::of(ctx), Command::with_params(name, params)).1.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn add(ws: &mut Workspace, ctx: ContextId, id: ObjectId, effect: &LayerEffect) {
    run(ws, ctx, "effect.add", json!({"id": id.0, "effect": serde_json::to_value(effect).unwrap()}));
}

fn doc(ws: &Workspace, ctx: ContextId) -> &Document {
    &ws.get(ctx).unwrap().session.document
}

fn px(doc: &Document, x: f32, y: f32) -> [u8; 4] {
    let p = render_region(doc, &CanvasFlags::default(), &[], [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1);
    let picked = dtool_render::pick_sampler::pick(doc, x, y, PickMode::Point, None).unwrap();
    assert_eq!([picked.r, picked.g, picked.b, picked.a], [p[0], p[1], p[2], p[3]], "pick vs raster at {x},{y}");
    [p[0], p[1], p[2], p[3]]
}

fn canvas_equals_export(ws: &Workspace, ctx: ContextId, id: ObjectId) {
    let session = &ws.get(ctx).unwrap().session;
    let mut canvas = AppCanvas::new();
    let (b, pixels) = canvas.effect_pixels(session, id, [64, 64]).unwrap();
    let scale = dtool_core::effect_raster::effect_scale(&session.document, 64.0 / b[2]);
    let export = render_region_scaled(&session.document, &CanvasFlags::default(), &[], b, 64, 64, scale);
    assert_eq!(pixels.len(), export.len());
    for (i, (a, e)) in pixels.iter().zip(&export).enumerate() {
        assert!((i16::from(*a) - i16::from(*e)).abs() <= 1, "byte {i}: canvas {a} export {e}");
    }
}

fn near(got: [u8; 4], want: [u8; 4], tol: i16) -> bool {
    got.iter().zip(want).all(|(g, w)| (i16::from(*g) - i16::from(w)).abs() <= tol)
}

fn bevel(kind: BevelType) -> LayerEffect {
    let mut effect = default_effect(EffectKind::BevelEmboss);
    effect.params = EffectParams::Light { radius: 6.0, depth: 1.0, soften: 0.0, azimuth: 135.0, elevation: 45.0, bevel: kind, highlight: Colour::new(255, 255, 255, 255), shadow: Colour::new(0, 0, 0, 255) };
    effect
}

fn gradient(kind: GradientKind, start: [f64; 2], end: [f64; 2]) -> LayerEffect {
    let mut effect = default_effect(EffectKind::GradientOverlay);
    let stops = vec![GradientStop { offset: 0.0, colour: Colour::new(255, 0, 0, 255) }, GradientStop { offset: 1.0, colour: Colour::new(0, 0, 255, 255) }];
    effect.params = EffectParams::Overlay { fill: Paint::Gradient { kind, stops, geom: GradientGeom { start, end, ratio: 1.0, ..GradientGeom::default() } } };
    effect
}

#[test]
fn bevel_emboss_types_light_top_left_and_shade_bottom_right_live() {
    let cases: [(BevelType, &[(f32, [u8; 4])]); 4] = [
        (BevelType::Inner, &[(38.5, C), (41.5, W), (44.5, G), (80.0, G), (118.5, K), (121.5, C)]),
        (BevelType::Outer, &[(35.5, C), (38.5, W), (41.5, G), (118.5, G), (121.5, K), (124.5, C)]),
        (BevelType::Emboss, &[(38.5, C), (39.5, W), (40.5, W), (41.5, G), (80.0, G), (118.5, G), (119.5, K), (120.5, K), (121.5, C)]),
        (BevelType::Pillow, &[(38.5, C), (39.5, W), (40.5, K), (41.5, G), (80.0, G), (118.5, G), (119.5, W), (120.5, K), (121.5, C)]),
    ];
    for (kind, samples) in cases {
        let (mut ws, ctx, id) = live();
        add(&mut ws, ctx, id, &bevel(kind));
        for (at, want) in samples {
            assert_eq!(px(doc(&ws, ctx), *at, 80.0), *want, "{kind:?} row x={at}");
            assert_eq!(px(doc(&ws, ctx), 80.0, *at), *want, "{kind:?} col y={at}");
        }
        canvas_equals_export(&ws, ctx, id);
        run(&mut ws, ctx, "edit.undo", json!({}));
        assert!(doc(&ws, ctx).get_object(id).unwrap().effects.is_empty());
        assert_eq!(px(doc(&ws, ctx), 80.0, 80.0), G);
        assert_eq!(px(doc(&ws, ctx), 41.5, 80.0), G, "{kind:?} undo");
    }
}

#[test]
fn gradient_overlay_kind_and_angle_colours_live() {
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let mid = [128, 0, 128, 255];
    let cases: [(&str, LayerEffect, &[([f32; 2], [u8; 4])]); 4] = [
        ("linear 0deg", gradient(GradientKind::Linear, [0.0, 40.0], [80.0, 40.0]), &[([40.5, 80.0], red), ([119.5, 80.0], blue), ([80.0, 80.0], mid), ([80.0, 40.5], mid), ([80.0, 119.5], mid)]),
        ("linear 90deg", gradient(GradientKind::Linear, [40.0, 0.0], [40.0, 80.0]), &[([80.0, 40.5], red), ([80.0, 119.5], blue), ([80.0, 80.0], mid), ([40.5, 80.0], mid), ([119.5, 80.0], mid)]),
        ("linear 45deg", gradient(GradientKind::Linear, [0.0, 0.0], [80.0, 80.0]), &[([40.5, 40.5], red), ([119.5, 119.5], blue), ([119.5, 40.5], mid), ([40.5, 119.5], mid), ([80.0, 80.0], mid)]),
        ("radial", gradient(GradientKind::Radial, [40.0, 40.0], [80.0, 40.0]), &[([80.0, 80.0], red), ([100.0, 80.0], mid), ([80.0, 60.0], mid), ([119.5, 80.0], blue), ([80.0, 40.5], blue)]),
    ];
    for (name, effect, samples) in cases {
        let (mut ws, ctx, id) = live();
        add(&mut ws, ctx, id, &effect);
        for (at, want) in samples {
            let got = px(doc(&ws, ctx), at[0], at[1]);
            assert!(near(got, *want, 8), "{name} at {at:?}: {got:?} want {want:?}");
        }
        assert_eq!(px(doc(&ws, ctx), 38.5, 80.0), C, "{name} stays inside the shape");
        canvas_equals_export(&ws, ctx, id);
        run(&mut ws, ctx, "effect.update", json!({"id": id.0, "index": 0, "opacity": 0.5}));
        let (at, want) = samples[0];
        let half = px(doc(&ws, ctx), at[0], at[1]);
        let blended = [0, 1, 2].map(|i| ((u16::from(want[i]) + 128) / 2) as u8);
        assert!(near(half, [blended[0], blended[1], blended[2], 255], 8), "{name} half opacity {half:?} want {blended:?}");
        canvas_equals_export(&ws, ctx, id);
    }
}
