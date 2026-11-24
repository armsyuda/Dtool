use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint};
use dtool_core::scene::{Prim, Scene};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{CanvasFlags, ViewMode};
use dtool_mcp::{Config, Server};
use dtool_render::scene::{build_scene, pixel_step};
use serde_json::{json, Value};

fn setup() -> (Server, u64) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("c02_pixel");
    workspace.activate(a).unwrap();
    let mut server = Server::new(Config::default(), workspace);
    ok(&run(&mut server, a.0, "object.create", json!({ "pos": [10.3, 20.7], "size": [5.4, 3.2], "fill": serde_json::to_value(Paint::Solid { colour: Colour::new(200, 30, 30, 255) }).unwrap() })));
    (server, a.0)
}

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn ok(reply: &Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply.clone()
}

fn live_scene(server: &Server, scope: u64) -> (Scene, u64, bool) {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let session = &ws.get(ContextId(scope)).unwrap().session;
    (build_scene(&session.document, &session.view.canvas_flags), session.document.version, session.undo.can_undo())
}

fn coords(scene: &Scene) -> Vec<f32> {
    let mut out = Vec::new();
    for prim in &scene.prims {
        match prim {
            Prim::Fill { contours, .. } => contours.iter().flatten().for_each(|p| out.extend(p)),
            Prim::Stroke { points, .. } => points.iter().for_each(|p| out.extend(p)),
            _ => {}
        }
    }
    out
}

fn on_grid(values: &[f32], step: f32) -> bool {
    values.iter().all(|v| ((v / step) - (v / step).round()).abs() < 1e-3)
}

#[test]
fn mcp_view_mode_pixel_snaps_live_canvas_scene_without_touching_document() {
    let (mut s, a) = setup();
    let (normal, version, undo) = live_scene(&s, a);
    let normal = coords(&normal);
    assert!(!normal.is_empty());
    assert!(!on_grid(&normal, 0.5), "{normal:?}");

    ok(&run(&mut s, a, "view.flags.set", json!({ "view_mode": "Pixel" })));
    let (pixel, v, u) = live_scene(&s, a);
    let pixel = coords(&pixel);
    assert!(on_grid(&pixel, 1.0), "{pixel:?}");
    assert_eq!((v, u), (version, undo));

    ok(&run(&mut s, a, "view.flags.set", json!({ "view_mode": "Pixel2x" })));
    let (retina, v, u) = live_scene(&s, a);
    let retina = coords(&retina);
    assert!(on_grid(&retina, 0.5), "{retina:?}");
    assert!(!on_grid(&retina, 1.0), "{retina:?}");
    assert_eq!((v, u), (version, undo));

    let q = ok(&run(&mut s, a, "view.flags.query", json!({})));
    assert_eq!(q["result"]["flags"]["view_mode"], json!("Pixel2x"), "{q}");

    ok(&run(&mut s, a, "view.flags.set", json!({ "view_mode": "Normal" })));
    let (back, _, _) = live_scene(&s, a);
    assert_eq!(coords(&back), normal);
}

#[test]
fn pixel_step_follows_document_units_and_mode() {
    let px = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let mm = Document::new([100.0, 100.0], Units::Mm, 300.0, 1);
    let flags = |view_mode| CanvasFlags { view_mode, ..CanvasFlags::default() };
    assert_eq!(pixel_step(&px, &flags(ViewMode::Normal)), None);
    assert_eq!(pixel_step(&px, &flags(ViewMode::Outline)), None);
    assert_eq!(pixel_step(&px, &flags(ViewMode::Pixel)), Some(1.0));
    assert_eq!(pixel_step(&px, &flags(ViewMode::Pixel2x)), Some(0.5));
    let step = pixel_step(&mm, &flags(ViewMode::Pixel)).unwrap();
    assert!((step - 25.4 / 300.0).abs() < 1e-5, "{step}");
}