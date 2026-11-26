use dtool_core::layer_style::raster::render_region;
use dtool_core::layer_style::{BevelType, EffectParams, OutlineAlign};
use dtool_core::object::ObjectId;
use dtool_core::paint::{Colour, Paint};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::CanvasFlags;
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn setup() -> (Server, u64, u64) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("b50").0;
    let mut server = Server::new(Config::default(), workspace);
    run(&mut server, a, "object.create", json!({ "pos": [10.0, 10.0], "size": [40.0, 40.0] }));
    let id = {
        let ws = server.workspace();
        let mut ws = ws.lock().unwrap();
        let doc = &mut ws.get_mut(ContextId(a)).unwrap().session.document;
        let object = doc.objects.last_mut().unwrap();
        object.fill = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
        object.id.0
    };
    (server, a, id)
}

fn px(server: &Server, scope: u64, x: f32, y: f32) -> [u8; 4] {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let doc = &ws.get(ContextId(scope)).unwrap().session.document;
    let p = render_region(doc, &CanvasFlags::default(), &[], [x - 0.5, y - 0.5, 1.0, 1.0], 1, 1);
    [p[0], p[1], p[2], p[3]]
}

fn effects(server: &Server, scope: u64, id: u64) -> usize {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.get_object(ObjectId(id)).map(|o| o.effects.len()).unwrap_or(0)
}

fn round_trip(kind: &str, params: EffectParams) -> (Server, u64, u64, Value) {
    let (mut s, a, id) = setup();
    run(&mut s, a, "effect.add", json!({ "id": id, "kind": kind }));
    let params = serde_json::to_value(params).unwrap();
    run(&mut s, a, "effect.update", json!({ "id": id, "index": 0, "opacity": 1.0, "blend": "Normal", "params": params }));
    let listed = run(&mut s, a, "effect.list", json!({ "id": id }));
    assert!(listed.to_string().contains(kind), "{listed}");
    assert_eq!(effects(&s, a, id), 1);
    (s, a, id, listed)
}

fn blue() -> Colour {
    Colour::new(0, 0, 255, 255)
}

fn green() -> Colour {
    Colour::new(0, 255, 0, 255)
}

#[test]
fn mcp_outer_shadow_offset_and_angle_are_stored_and_rendered() {
    let (s, a, _, listed) = round_trip("OuterShadow", EffectParams::Shadow { radius_y: None, blur_angle: None, radius: 0.0, offset: 10.0, angle: 180.0, intensity: 0.0, colour: blue(), knock_out: true });
    let text = listed.to_string();
    assert!(text.contains("\"offset\":10.0") && text.contains("\"angle\":180.0"), "{text}");
    assert_eq!(px(&s, a, 55.0, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&s, a, 30.0, 30.0), [255, 0, 0, 255]);
}

#[test]
fn mcp_inner_shadow_renders_inside_the_edge_only() {
    let (s, a, _, _) = round_trip("InnerShadow", EffectParams::Shadow { radius_y: None, blur_angle: None, radius: 0.0, offset: 6.0, angle: 0.0, intensity: 0.0, colour: blue(), knock_out: false });
    assert_eq!(px(&s, a, 48.0, 30.0), [0, 0, 255, 255]);
    assert_eq!(px(&s, a, 52.0, 30.0)[3], 0);
}

#[test]
fn mcp_outer_glow_colour_and_radius() {
    let (s, a, _, listed) = round_trip("OuterGlow", EffectParams::Glow { radius: 6.0, intensity: 0.0, colour: green(), from_centre: false });
    assert!(listed.to_string().contains("\"radius\":6.0"), "{listed}");
    let near = px(&s, a, 50.5, 30.0);
    assert!(near[1] == 255 && near[0] == 0 && near[3] > 0, "{near:?}");
    assert_eq!(px(&s, a, 70.0, 30.0)[3], 0);
}

#[test]
fn mcp_outline_alignments_render_their_width() {
    for (align, inside_px, outside_px) in [(OutlineAlign::Outside, 48.0, 53.5), (OutlineAlign::Centre, 48.5, 51.5), (OutlineAlign::Inside, 46.5, 49.5)] {
        let (s, a, _, listed) = round_trip("Outline", EffectParams::Outline { width: 4.0, align, fill: Paint::Solid { colour: blue() } });
        assert!(listed.to_string().contains(&format!("{align:?}")), "{listed}");
        let probe = if align == OutlineAlign::Outside { outside_px } else { inside_px };
        assert_eq!(px(&s, a, probe, 30.0), [0, 0, 255, 255], "{align:?}");
        assert_eq!(px(&s, a, 56.0, 30.0)[3], 0, "{align:?}");
    }
}

#[test]
fn mcp_three_d_light_azimuth_and_elevation_are_stored() {
    let (s, a, _, listed) = round_trip("ThreeD", EffectParams::Light { radius: 4.0, depth: 1.0, soften: 0.0, azimuth: 315.0, elevation: 30.0, bevel: BevelType::Inner, highlight: Colour::new(255, 255, 255, 255), shadow: Colour::new(0, 0, 0, 255) });
    let text = listed.to_string();
    assert!(text.contains("\"azimuth\":315.0") && text.contains("\"elevation\":30.0"), "{text}");
    assert_eq!(px(&s, a, 49.0, 30.0)[0..3], [255, 255, 255]);
    assert_eq!(px(&s, a, 11.0, 30.0)[0..3], [0, 0, 0]);
}

#[test]
fn mcp_colour_overlay_recolours_visible_pixels_and_undoes() {
    let (mut s, a, id, _) = round_trip("ColourOverlay", EffectParams::Overlay { fill: Paint::Solid { colour: green() } });
    for (x, y) in [(12.0, 12.0), (30.0, 30.0), (48.0, 48.0)] {
        assert_eq!(px(&s, a, x, y), [0, 255, 0, 255]);
    }
    assert_eq!(px(&s, a, 5.0, 5.0)[3], 0);
    run(&mut s, a, "edit.undo", json!({}));
    run(&mut s, a, "edit.undo", json!({}));
    assert_eq!(effects(&s, a, id), 0);
    assert_eq!(px(&s, a, 30.0, 30.0), [255, 0, 0, 255]);
}