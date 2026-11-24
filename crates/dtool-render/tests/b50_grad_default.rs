use std::sync::Arc;
use dtool_core::document::Units;
use dtool_core::layer_style::raster::render_region;
use dtool_core::layer_style::{EffectKind, EffectParams};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, GradientKind, Paint};
use dtool_core::{CanvasFlags, Command, ContextId, Document, Scope, SharedServices, Workspace};
use serde_json::{json, Value};

fn live(size: [f32; 2]) -> (Workspace, ContextId, ObjectId) {
    assert!(dtool_render::scene::install());
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let ctx = ws.new_document("grad");
    let mut doc = Document::new([300.0, 300.0], Units::Px, 72.0, 1);
    let id = doc.create_object(Object { pos: [40.0, 40.0], size, fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    ws.get_mut(ctx).unwrap().session = dtool_core::Session::new(doc);
    (ws, ctx, id)
}

fn run(ws: &mut Workspace, ctx: ContextId, name: &str, params: Value) -> Value {
    ws.run(Scope::of(ctx), Command::with_params(name, params)).1.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn px(ws: &Workspace, ctx: ContextId, x: f32, y: f32) -> [u8; 4] {
    let p = render_region(&ws.get(ctx).unwrap().session.document, &CanvasFlags::default(), &[], [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1);
    [p[0], p[1], p[2], p[3]]
}

fn grey(c: [u8; 4], v: i16) -> bool {
    c[3] == 255 && c[..3].iter().all(|ch| (i16::from(*ch) - v).abs() <= 6)
}

#[test]
fn default_gradient_overlay_spans_object_bounds_at_90_degrees() {
    for size in [[120.0, 60.0], [40.0, 200.0]] {
        let (mut ws, ctx, id) = live(size);
        run(&mut ws, ctx, "effect.add", json!({"id": id.0, "kind": "GradientOverlay"}));
        let effect = ws.get(ctx).unwrap().session.document.get_object(id).unwrap().effects[0].clone();
        assert_eq!(effect.kind, EffectKind::GradientOverlay);
        let EffectParams::Overlay { fill: Paint::Gradient { kind, stops, geom } } = &effect.params else { panic!("{:?}", effect.params) };
        assert_eq!(*kind, GradientKind::Linear);
        assert_eq!((stops[0].colour, stops[stops.len() - 1].colour), (Colour::new(0, 0, 0, 255), Colour::new(255, 255, 255, 255)));
        assert_eq!((geom.start, geom.end), ([f64::from(size[0]) * 0.5, f64::from(size[1])], [f64::from(size[0]) * 0.5, 0.0]));
        let [x0, y0] = [40.0f32, 40.0];
        let [w, h] = size;
        let bottom = px(&ws, ctx, x0 + w * 0.5, y0 + h - 0.5);
        let top = px(&ws, ctx, x0 + w * 0.5, y0 + 0.5);
        let middle = px(&ws, ctx, x0 + w * 0.5, y0 + h * 0.5);
        assert!(grey(bottom, 0), "{size:?} bottom {bottom:?}");
        assert!(grey(top, 255), "{size:?} top {top:?}");
        assert!(grey(middle, 128), "{size:?} middle {middle:?}");
        for x in [x0 + 0.5, x0 + w - 0.5] {
            assert_eq!(px(&ws, ctx, x, y0 + h * 0.5), middle, "{size:?} level across x={x}");
        }
        run(&mut ws, ctx, "edit.undo", json!({}));
        assert_eq!(px(&ws, ctx, x0 + w * 0.5, y0 + h * 0.5), [255, 0, 0, 255]);
    }
}

#[test]
fn default_gradient_overlay_preview_spans_bounds_too() {
    let (mut ws, ctx, id) = live([100.0, 50.0]);
    run(&mut ws, ctx, "effect.preview", json!({"id": id.0, "kind": "GradientOverlay"}));
    let session = &ws.get(ctx).unwrap().session;
    let staged: Vec<_> = dtool_core::commands::layer_style::staged(session, id).into_iter().collect();
    let p = render_region(&session.document, &CanvasFlags::default(), &staged, [89.5, 64.5, 1.0, 1.0], 1, 1);
    assert!(grey([p[0], p[1], p[2], p[3]], 128), "{p:?}");
    run(&mut ws, ctx, "effect.cancel", json!({"id": id.0}));
}
