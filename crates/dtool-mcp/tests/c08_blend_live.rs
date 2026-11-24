//! B29-T007 blend modes + B29-T008 blend ranges composited by dtool_core::blend_composite on a live MCP doc
//! (doc A edited while doc B is active).
use dtool_core::paint::{Colour, Paint};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{CanvasFlags, Document};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn ok(reply: Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply["result"].clone()
}

fn doc(server: &Server, scope: u64) -> Document {
    server.workspace().lock().unwrap().get(ContextId(scope)).unwrap().session.document.clone()
}

fn create(server: &mut Server, scope: u64, pos: [f32; 2], size: [f32; 2], c: [u8; 3]) -> u64 {
    let fill = serde_json::to_value(Paint::Solid { colour: Colour::new(c[0], c[1], c[2], 255) }).unwrap();
    ok(run(server, scope, "object.create", json!({ "pos": pos, "size": size, "fill": fill })));
    doc(server, scope).objects.last().unwrap().id.0
}

/// What the scene rasterizer produces once scene.rs routes through blend_composite (PR to bot03).
fn probe(server: &Server, scope: u64) -> Vec<u8> {
    let d = doc(server, scope);
    let scene = dtool_core::scene::build_scene(&d, &CanvasFlags::default());
    dtool_core::blend_composite::rasterize(&d, &scene, [0.0, 0.0, 80.0, 80.0], 80, 80, None, dtool_core::scene::rasterize_scene)
        .unwrap_or_else(|| dtool_core::scene::rasterize_scene(&d, &scene, [0.0, 0.0, 80.0, 80.0], 80, 80, None))
}

fn at(px: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * 80 + x) * 4;
    [px[i], px[i + 1], px[i + 2]]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= 6)
}

fn setup() -> (Server, u64) {
    assert!(dtool_render::scene::install() || dtool_core::scene::scene_ready());
    let mut workspace = Workspace::default();
    let a = workspace.new_document("blend_a");
    let b = workspace.new_document("blend_b");
    workspace.activate(b).unwrap();
    (Server::new(Config::default(), workspace), a.0)
}

#[test]
fn blend_modes_composite_with_what_is_underneath_on_live_doc_and_undo() {
    let (mut s, a) = setup();
    create(&mut s, a, [0.0, 0.0], [40.0, 80.0], [0, 200, 0]);
    create(&mut s, a, [40.0, 0.0], [40.0, 80.0], [200, 200, 200]);
    let top = create(&mut s, a, [20.0, 20.0], [40.0, 40.0], [255, 0, 0]);
    let normal = probe(&s, a);
    assert!(near(at(&normal, 30, 30), [255, 0, 0]) && near(at(&normal, 50, 30), [255, 0, 0]));
    assert!(dtool_core::blend_composite::rasterize(&doc(&s, a), &dtool_core::scene::build_scene(&doc(&s, a), &CanvasFlags::default()), [0.0, 0.0, 80.0, 80.0], 80, 80, None, dtool_core::scene::rasterize_scene).is_none(), "all-Normal scene keeps the plain path");

    ok(run(&mut s, a, "layer.blend.set", json!({ "id": top, "mode": "Multiply" })));
    let m = probe(&s, a);
    assert!(near(at(&m, 30, 30), [0, 0, 0]), "red x green = black: {:?}", at(&m, 30, 30));
    assert!(near(at(&m, 50, 30), [200, 0, 0]), "red x grey: {:?}", at(&m, 50, 30));
    assert!(near(at(&m, 10, 10), [0, 200, 0]) && near(at(&m, 70, 70), [200, 200, 200]), "outside untouched");

    ok(run(&mut s, a, "layer.blend.set", json!({ "id": top, "mode": "Screen" })));
    let sc = probe(&s, a);
    assert!(near(at(&sc, 30, 30), [255, 200, 0]), "screen red/green: {:?}", at(&sc, 30, 30));

    ok(run(&mut s, a, "edit.undo", json!({})));
    assert!(near(at(&probe(&s, a), 30, 30), [0, 0, 0]), "undo back to Multiply");
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert!(near(at(&probe(&s, a), 30, 30), [255, 0, 0]), "undo back to Normal");
}

#[test]
fn object_above_a_blended_object_still_draws_normally_in_order() {
    let (mut s, a) = setup();
    create(&mut s, a, [0.0, 0.0], [80.0, 80.0], [0, 200, 0]);
    let mid = create(&mut s, a, [0.0, 0.0], [60.0, 60.0], [255, 0, 0]);
    create(&mut s, a, [40.0, 40.0], [40.0, 40.0], [0, 0, 255]);
    ok(run(&mut s, a, "layer.blend.set", json!({ "id": mid, "mode": "Multiply" })));
    let px = probe(&s, a);
    assert!(near(at(&px, 20, 20), [0, 0, 0]));
    assert!(near(at(&px, 50, 50), [0, 0, 255]), "normal object on top covers the blended one");
    assert!(near(at(&px, 70, 10), [0, 200, 0]));
}

#[test]
fn blend_ranges_gate_the_object_by_underlying_luminance_on_live_doc() {
    let (mut s, a) = setup();
    create(&mut s, a, [0.0, 0.0], [40.0, 80.0], [20, 20, 20]);
    create(&mut s, a, [40.0, 0.0], [40.0, 80.0], [240, 240, 240]);
    let top = create(&mut s, a, [0.0, 0.0], [80.0, 40.0], [255, 0, 0]);
    let v = doc(&s, a).version;
    // underlying: hide over dark (lum < 0.4), show over bright; source: always.
    let ranges = json!({ "source": [[0.0, 1.0], [1.0, 1.0]], "underlying": [[0.0, 0.0], [0.4, 0.0], [0.5, 1.0], [1.0, 1.0]] });
    ok(run(&mut s, a, "layer.blend_ranges.set", json!({ "id": top, "ranges": ranges })));
    assert_eq!(doc(&s, a).version, v + 1);
    let px = probe(&s, a);
    assert!(near(at(&px, 20, 20), [20, 20, 20]), "hidden over dark: {:?}", at(&px, 20, 20));
    assert!(near(at(&px, 60, 20), [255, 0, 0]), "shown over bright: {:?}", at(&px, 60, 20));
    assert!(near(at(&px, 20, 60), [20, 20, 20]) && near(at(&px, 60, 60), [240, 240, 240]));
    // source gate: red luminance ~0.21 -> hide dark sources everywhere
    let ranges = json!({ "source": [[0.0, 0.0], [0.3, 0.0], [0.31, 1.0], [1.0, 1.0]], "underlying": [[0.0, 1.0], [1.0, 1.0]] });
    ok(run(&mut s, a, "layer.blend_ranges.set", json!({ "id": top, "ranges": ranges })));
    let px = probe(&s, a);
    assert!(near(at(&px, 60, 20), [240, 240, 240]), "dark source hidden: {:?}", at(&px, 60, 20));
    ok(run(&mut s, a, "layer.blend_ranges.set", json!({ "id": top, "ranges": null })));
    let px = probe(&s, a);
    assert!(near(at(&px, 20, 20), [255, 0, 0]) && near(at(&px, 60, 20), [255, 0, 0]), "cleared ranges draw normally");
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert!(near(at(&probe(&s, a), 60, 20), [240, 240, 240]));
}

#[test]
fn curve_and_range_factor_units() {
    use dtool_core::blend_composite::{curve, range_factor};
    let c = [[0.0, 0.0], [0.5, 1.0], [1.0, 1.0]];
    assert_eq!(curve(&c, -1.0), 0.0);
    assert!((curve(&c, 0.25) - 0.5).abs() < 1e-5);
    assert_eq!(curve(&c, 2.0), 1.0);
    let r = dtool_core::layer_style::BlendRanges { source: vec![[0.0, 1.0], [1.0, 1.0]], underlying: vec![[0.0, 0.0], [1.0, 1.0]], gamma: 1.0, antialias: false };
    assert!(range_factor(&r, &[255, 0, 0, 255], &[0, 0, 0, 0]) == 1.0, "transparent underlying does not gate");
    assert!(range_factor(&r, &[255, 0, 0, 255], &[0, 0, 0, 255]) < 0.01);
    assert!(range_factor(&r, &[255, 0, 0, 255], &[255, 255, 255, 255]) > 0.99);
}