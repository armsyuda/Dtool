use dtool_core::paint::{Colour, Paint};
use dtool_core::scene::build_scene;
use dtool_core::scene_hit::hit_test;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn ok(reply: Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply["result"].clone()
}

fn server() -> (Server, u64) {
    assert!(dtool_render::scene::install() || dtool_core::scene::scene_ready());
    let mut workspace = Workspace::default();
    let a = workspace.new_document("c09d");
    (Server::new(Config::default(), workspace), a.0)
}

fn pick(server: &Server, scope: u64, p: [f32; 2]) -> Option<u64> {
    let shared = server.workspace();
    let workspace = shared.lock().unwrap();
    let session = &workspace.get(ContextId(scope)).unwrap().session;
    let scene = build_scene(&session.document, &session.view.canvas_flags);
    hit_test(&scene, &session.document, p, 0.5).map(|id| id.0)
}

fn frame(server: &mut Server, scope: u64, shape: &str, pos: [f32; 2]) -> u64 {
    ok(run(server, scope, "frame.image.add", json!({ "shape": shape, "pos": pos, "size": [40.0, 40.0] })));
    let shared = server.workspace();
    let mut workspace = shared.lock().unwrap();
    let session = &mut workspace.get_mut(ContextId(scope)).unwrap().session;
    let object = session.document.objects.last_mut().unwrap();
    object.raster_size = Some((2, 2));
    object.raster_data = Some(vec![255; 16]);
    let id = object.id.0;
    drop(workspace);
    drop(shared);
    ok(run(server, scope, "frame.image.fit.set", json!({ "object": id, "fit": "stretch" })));
    id
}

fn create(server: &mut Server, scope: u64, pos: [f32; 2], size: [f32; 2]) -> u64 {
    let fill = serde_json::to_value(Paint::Solid { colour: Colour::new(0, 0, 255, 255) }).unwrap();
    ok(run(server, scope, "object.create", json!({ "name": "box", "pos": pos, "size": size, "fill": fill })));
    let shared = server.workspace();
    let workspace = shared.lock().unwrap();
    workspace.get(ContextId(scope)).unwrap().session.document.objects.last().unwrap().id.0
}

#[test]
fn c09d_ellipse_frame_pick_misses_the_corners_and_hits_inside() {
    let (mut server, scope) = server();
    let id = frame(&mut server, scope, "ellipse", [0.0, 0.0]);
    for corner in [[1.5, 1.5], [38.5, 1.5], [1.5, 38.5], [38.5, 38.5]] {
        assert_eq!(pick(&server, scope, corner), None, "corner {corner:?}");
    }
    for inner in [[20.0, 20.0], [20.0, 2.0], [3.0, 20.0], [10.0, 10.0]] {
        assert_eq!(pick(&server, scope, inner), Some(id), "inside {inner:?}");
    }
    let rect = frame(&mut server, scope, "rect", [50.0, 0.0]);
    assert_eq!(pick(&server, scope, [51.5, 1.5]), Some(rect));
    ok(run(&mut server, scope, "frame.image.content.set", json!({ "object": rect, "rotation": 45.0 })));
    assert_eq!(pick(&server, scope, [51.5, 1.5]), None, "rotated content leaves the rect corner empty");
    assert_eq!(pick(&server, scope, [70.0, 20.0]), Some(rect));
}

#[test]
fn c09d_clipped_child_is_not_pickable_outside_its_clip() {
    let (mut server, scope) = server();
    let parent = create(&mut server, scope, [0.0, 0.0], [40.0, 40.0]);
    let child = create(&mut server, scope, [20.0, 20.0], [40.0, 40.0]);
    assert_eq!(pick(&server, scope, [50.0, 50.0]), Some(child));
    ok(run(&mut server, scope, "layer.clip.set", json!({ "id": child, "parent": parent })));
    assert_eq!(pick(&server, scope, [50.0, 50.0]), None);
    assert_eq!(pick(&server, scope, [30.0, 30.0]), Some(child));
    assert_eq!(pick(&server, scope, [10.0, 10.0]), Some(parent));
    ok(run(&mut server, scope, "edit.undo", json!({})));
    assert_eq!(pick(&server, scope, [50.0, 50.0]), Some(child));
}