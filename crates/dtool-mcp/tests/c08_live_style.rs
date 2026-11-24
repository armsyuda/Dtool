use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn setup() -> (Server, u64, u64, u64) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    workspace.activate(b).unwrap();
    let mut server = Server::new(Config::default(), workspace);
    let reply = run(&mut server, a.0, "object.create", json!({ "pos": [0.0, 0.0], "size": [10.0, 10.0] }));
    assert!(reply.get("error").is_none(), "{reply}");
    let id = object(&server, a.0)["id"].as_u64().unwrap();
    (server, a.0, b.0, id)
}

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn object(server: &Server, scope: u64) -> Value {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    serde_json::to_value(ws.get(ContextId(scope)).unwrap().session.document.objects.last().unwrap()).unwrap()
}

fn version(server: &Server, scope: u64) -> u64 {
    server.workspace().lock().unwrap().get(ContextId(scope)).unwrap().session.document.version
}

fn ok(reply: &Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply.clone()
}

#[test]
fn mcp_blend_set_acts_on_scoped_live_document_with_one_undo() {
    let (mut s, a, b, id) = setup();
    let b_version = version(&s, b);
    ok(&run(&mut s, a, "layer.blend.set", json!({ "id": id, "mode": "Multiply" })));
    assert_eq!(object(&s, a)["blend_mode"], "Multiply");
    assert_eq!(version(&s, b), b_version);
    let v = version(&s, a);
    ok(&run(&mut s, a, "layer.blend.set", json!({ "id": id, "mode": "Multiply" })));
    assert_eq!(version(&s, a), v);
    ok(&run(&mut s, a, "edit.undo", json!({})));
    assert!(object(&s, a).get("blend_mode").is_none(), "{}", object(&s, a));
    assert_eq!(run(&mut s, a, "layer.blend.set", json!({ "id": id, "mode": "NoSuchMode" }))["error"]["code"], "INVALID_PARAMS");
}

#[test]
fn mcp_effect_add_list_preview_cancel_on_live_document() {
    let (mut s, a, _b, id) = setup();
    let v0 = version(&s, a);
    ok(&run(&mut s, a, "effect.add", json!({ "id": id, "kind": "OuterShadow" })));
    assert_eq!(object(&s, a)["effects"].as_array().map(Vec::len), Some(1));
    let listed = ok(&run(&mut s, a, "effect.list", json!({ "id": id })));
    assert!(listed.to_string().contains("OuterShadow"), "{listed}");
    let v1 = version(&s, a);
    assert!(v1 > v0);
    ok(&run(&mut s, a, "effect.preview", json!({ "id": id, "index": 0 })));
    ok(&run(&mut s, a, "effect.cancel", json!({ "id": id })));
    assert_eq!(version(&s, a), v1);
    ok(&run(&mut s, a, "effect.remove", json!({ "id": id, "index": 0 })));
    assert!(object(&s, a).get("effects").is_none());
    ok(&run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(object(&s, a)["effects"].as_array().map(Vec::len), Some(1));
}

fn mask(server: &Server, scope: u64, id: u64) -> Option<Vec<u8>> {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.layer_masks.iter().find(|m| m.object.0 == id).map(|m| m.coverage.coverage.clone())
}

#[test]
fn mcp_mask_commands_reach_live_document() {
    let (mut s, a, b, _) = setup();
    let placed = ok(&run(&mut s, a, "image.place", json!({ "pixel_size": [2, 1], "rgba": [255, 0, 0, 255, 0, 255, 0, 200], "pos": [10, 20] })));
    let id = placed["result"]["id"].as_u64().or_else(|| placed["id"].as_u64()).or_else(|| object(&s, a)["id"].as_u64()).unwrap();
    let raster = object(&s, a)["raster_data"].clone();
    ok(&run(&mut s, a, "layer.mask.set", json!({ "id": id, "mask": { "width": 2, "height": 1, "coverage": [0, 200] } })));
    ok(&run(&mut s, a, "layer.mask.invert", json!({ "id": id })));
    assert_eq!(mask(&s, a, id), Some(vec![255, 55]));
    assert_eq!(object(&s, a)["raster_data"], raster);
    ok(&run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(mask(&s, a, id), Some(vec![0, 200]));
    ok(&run(&mut s, a, "layer.mask.release", json!({ "id": id })));
    assert_eq!(mask(&s, a, id), None);
    assert_eq!(run(&mut s, a, "layer.mask.invert", json!({ "id": id }))["error"]["code"], "NOT_FOUND");
    assert_eq!(run(&mut s, b, "layer.mask.invert", json!({ "id": id }))["error"]["code"], "NOT_FOUND");
}