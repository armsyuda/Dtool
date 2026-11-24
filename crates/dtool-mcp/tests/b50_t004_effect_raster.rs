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

fn setup() -> (Server, u64) {
    assert!(dtool_render::scene::install() || dtool_core::scene::scene_ready());
    let mut workspace = Workspace::default();
    let a = workspace.new_document("b50_a");
    let b = workspace.new_document("b50_b");
    workspace.activate(b).unwrap();
    let mut s = Server::new(Config::default(), workspace);
    let fill = serde_json::to_value(Paint::Solid { colour: Colour::new(255, 0, 0, 255) }).unwrap();
    ok(run(&mut s, a.0, "object.create", json!({ "pos": [20.0, 20.0], "size": [30.0, 30.0], "fill": fill })));
    let id = doc(&s, a.0).objects.last().unwrap().id.0;
    ok(run(&mut s, a.0, "effect.add", json!({ "id": id, "kind": "OuterShadow" })));
    (s, a.0)
}

fn render(doc: &Document, px_per_unit: f32) -> Vec<u8> {
    let (w, h) = ((80.0 * px_per_unit) as u32, (80.0 * px_per_unit) as u32);
    let scale = dtool_core::effect_raster::effect_scale(doc, px_per_unit);
    dtool_core::layer_style::raster::render_region_scaled(doc, &CanvasFlags::default(), &[], [0.0, 0.0, 80.0, 80.0], w, h, scale)
}

#[test]
fn effect_raster_dpi_set_query_undo_on_live_doc() {
    let (mut s, a) = setup();
    let q = ok(run(&mut s, a, "doc.effect_raster.query", json!({})));
    assert_eq!(q["dpi"], Value::Null);
    assert_eq!(q["pdf_dpi"], json!(300.0));
    let v = doc(&s, a).version;
    let reply = ok(run(&mut s, a, "doc.effect_raster.set", json!({ "dpi": 72 })));
    assert_eq!(reply["changes"][0]["name"], json!("doc.effect_raster.set"), "{reply}");
    assert_eq!(doc(&s, a).effect_raster_dpi, Some(72.0));
    assert_eq!(doc(&s, a).version, v + 1);
    assert_eq!(ok(run(&mut s, a, "doc.effect_raster.set", json!({ "dpi": 72 })))["changes"], json!([]));
    for bad in [json!({ "dpi": 10 }), json!({ "dpi": 5000 }), json!({ "dpi": "x" }), json!({})] {
        assert!(run(&mut s, a, "doc.effect_raster.set", bad.clone()).get("error").is_some(), "{bad}");
    }
    assert_eq!(doc(&s, a).version, v + 1);
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(doc(&s, a).effect_raster_dpi, None);
    ok(run(&mut s, a, "edit.redo", json!({})));
    assert_eq!(doc(&s, a).effect_raster_dpi, Some(72.0));
    ok(run(&mut s, a, "doc.effect_raster.set", json!({ "dpi": null })));
    assert_eq!(doc(&s, a).effect_raster_dpi, None);
}

#[test]
fn effect_raster_dpi_controls_effect_resolution_and_round_trips() {
    let (mut s, a) = setup();
    let full = render(&doc(&s, a), 4.0);
    assert_eq!(full, dtool_core::layer_style::raster::render_region(&doc(&s, a), &CanvasFlags::default(), &[], [0.0, 0.0, 80.0, 80.0], 320, 320));
    let plain_doc = { let mut d = doc(&s, a); d.objects.last_mut().unwrap().effects.clear(); d };
    let plain = render(&plain_doc, 4.0);
    assert_ne!(full, plain, "effects drawn");
    ok(run(&mut s, a, "doc.effect_raster.set", json!({ "dpi": 36 })));
    let coarse = render(&doc(&s, a), 4.0);
    assert_ne!(coarse, full, "effect layer rendered at 36 dpi then upsampled");
    let centre = ((140 * 320 + 140) * 4) as usize;
    assert_eq!(&coarse[centre..centre + 4], &full[centre..centre + 4], "object body stays at full resolution");
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("b50_t004_roundtrip.dtool");
    dtool_io::native::save(&doc(&s, a), &path).unwrap();
    let reopened = dtool_io::native::open(&path).unwrap();
    assert_eq!(reopened.effect_raster_dpi, Some(36.0));
    assert_eq!(render(&reopened, 4.0), coarse);
    let _ = std::fs::remove_file(&path);
}