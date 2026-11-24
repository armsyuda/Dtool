use dtool_core::paint::{Colour, Paint};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{CanvasFlags, Document};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const GREEN: [u8; 3] = [0, 255, 0];

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn ok(reply: Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply["result"].clone()
}

fn fill(c: [u8; 3]) -> Value {
    serde_json::to_value(Paint::Solid { colour: Colour::new(c[0], c[1], c[2], 255) }).unwrap()
}

fn doc(server: &Server, scope: u64) -> Document {
    server.workspace().lock().unwrap().get(ContextId(scope)).unwrap().session.document.clone()
}

fn flags(server: &Server, scope: u64) -> CanvasFlags {
    server.workspace().lock().unwrap().get(ContextId(scope)).unwrap().session.view.canvas_flags.clone()
}

fn create(server: &mut Server, scope: u64, pos: [f32; 2], size: [f32; 2], c: [u8; 3]) -> u64 {
    ok(run(server, scope, "object.create", json!({ "pos": pos, "size": size, "fill": fill(c) })));
    doc(server, scope).objects.last().unwrap().id.0
}

fn pixels(doc: &Document, flags: &CanvasFlags) -> Vec<u8> {
    let mut scene = dtool_core::scene::build_scene(doc, flags);
    dtool_core::clip_mask::apply(doc, flags, &mut scene);
    dtool_core::scene::rasterize_scene(doc, &scene, [0.0, 0.0, 80.0, 80.0], 80, 80, None)
}

fn at(px: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * 80 + x) * 4;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn is(p: [u8; 4], c: [u8; 3]) -> bool {
    p[3] > 200 && (0..3).all(|i| (i32::from(p[i]) - i32::from(c[i])).abs() < 40)
}

fn any(px: &[u8], c: [u8; 3]) -> bool {
    px.chunks(4).any(|p| is([p[0], p[1], p[2], p[3]], c))
}

fn probe(server: &Server, scope: u64) -> Vec<u8> {
    pixels(&doc(server, scope), &flags(server, scope))
}

fn setup() -> (Server, u64, u64, u64) {
    assert!(dtool_render::scene::install() || dtool_core::scene::scene_ready());
    let mut workspace = Workspace::default();
    let a = workspace.new_document("k08_a");
    let b = workspace.new_document("k08_b");
    workspace.activate(b).unwrap();
    let mut server = Server::new(Config::default(), workspace);
    let parent = create(&mut server, a.0, [0.0, 0.0], [40.0, 40.0], RED);
    let child = create(&mut server, a.0, [20.0, 20.0], [40.0, 40.0], BLUE);
    (server, a.0, parent, child)
}

#[test]
fn clip_child_is_clipped_to_parent_on_live_doc_with_undo_redo_and_release() {
    let (mut s, a, parent, child) = setup();
    let before = probe(&s, a);
    assert!(is(at(&before, 50, 50), BLUE) && is(at(&before, 10, 10), RED));
    let v = doc(&s, a).version;
    let reply = ok(run(&mut s, a, "layer.clip.set", json!({ "id": child, "parent": parent })));
    assert_eq!(reply["changes"][0]["name"], json!("layer.clip.set"), "{reply}");
    assert_eq!(doc(&s, a).version, v + 1);
    let object = doc(&s, a).get_object(dtool_core::ObjectId(child)).cloned().unwrap();
    assert_eq!(object.parent_id.map(|p| p.0), Some(parent));
    assert_eq!(object.clip_role, dtool_core::object::ClipRole::Clip);
    let clipped = probe(&s, a);
    assert!(is(at(&clipped, 30, 30), BLUE), "inside parent and child");
    assert_eq!(at(&clipped, 50, 50)[3], 0, "outside parent is clipped away");
    assert!(is(at(&clipped, 10, 10), RED));
    assert_eq!(ok(run(&mut s, a, "layer.clip.set", json!({ "id": child, "parent": parent })))["changes"], json!([]));
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert!(is(at(&probe(&s, a), 50, 50), BLUE));
    assert_eq!(doc(&s, a).get_object(dtool_core::ObjectId(child)).unwrap().parent_id, None);
    ok(run(&mut s, a, "edit.redo", json!({})));
    assert_eq!(at(&probe(&s, a), 50, 50)[3], 0);
    ok(run(&mut s, a, "layer.clip.release", json!({ "id": child })));
    let released = probe(&s, a);
    assert!(is(at(&released, 50, 50), BLUE));
    assert_eq!(doc(&s, a).get_object(dtool_core::ObjectId(child)).unwrap().parent_id, None);
}

#[test]
fn vector_mask_hides_parent_and_children_outside_the_path_and_is_not_drawn() {
    let (mut s, a, parent, child) = setup();
    ok(run(&mut s, a, "layer.clip.set", json!({ "id": child, "parent": parent })));
    let mask = create(&mut s, a, [0.0, 0.0], [20.0, 80.0], GREEN);
    ok(run(&mut s, a, "layer.vector_mask.set", json!({ "id": mask, "target": parent })));
    let px = probe(&s, a);
    assert!(is(at(&px, 10, 10), RED), "parent inside mask");
    assert_eq!(at(&px, 30, 10)[3], 0, "parent outside mask hidden");
    assert_eq!(at(&px, 30, 30)[3], 0, "clip child inherits the parent's vector mask");
    assert!(!any(&px, GREEN), "the mask path itself is not drawn");
    ok(run(&mut s, a, "edit.undo", json!({})));
    let px = probe(&s, a);
    assert!(is(at(&px, 30, 10), RED) && is(at(&px, 30, 30), BLUE));
    ok(run(&mut s, a, "edit.redo", json!({})));
    ok(run(&mut s, a, "layer.vector_mask.release", json!({ "id": mask })));
    let px = probe(&s, a);
    assert!(any(&px, GREEN) && is(at(&px, 30, 10), RED));
}

#[test]
fn alt_mask_view_shows_mask_only_and_never_touches_the_document() {
    let (mut s, a, parent, _child) = setup();
    assert!(run(&mut s, a, "view.mask_view.set", json!({ "id": parent })).get("error").is_some(), "no mask yet");
    let mask = create(&mut s, a, [0.0, 0.0], [20.0, 40.0], GREEN);
    ok(run(&mut s, a, "layer.vector_mask.set", json!({ "id": mask, "target": parent })));
    let v = doc(&s, a).version;
    let saved = serde_json::to_string(&doc(&s, a)).unwrap();
    let reply = ok(run(&mut s, a, "view.mask_view.set", json!({ "id": parent })));
    assert_eq!(reply["mask_view"], json!(parent));
    assert_eq!(flags(&s, a).mask_view.map(|id| id.0), Some(parent));
    let px = probe(&s, a);
    assert!(is(at(&px, 10, 10), [255, 255, 255]), "mask area white");
    assert!(is(at(&px, 30, 10), [0, 0, 0]), "outside mask black");
    assert!(!any(&px, RED) && !any(&px, BLUE));
    assert_eq!(doc(&s, a).version, v);
    assert_eq!(serde_json::to_string(&doc(&s, a)).unwrap(), saved);
    assert!(!serde_json::to_string(&flags(&s, a)).unwrap().contains("mask_view"));
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(doc(&s, a).get_object(dtool_core::ObjectId(mask)).unwrap().clip_role, dtool_core::object::ClipRole::None, "undo skips the view toggle");
    ok(run(&mut s, a, "edit.redo", json!({})));
    ok(run(&mut s, a, "view.mask_view.set", json!({ "id": null })));
    assert!(is(at(&probe(&s, a), 10, 10), RED));
}

#[test]
fn clip_and_vector_mask_survive_native_save_and_reopen() {
    let (mut s, a, parent, child) = setup();
    ok(run(&mut s, a, "layer.clip.set", json!({ "id": child, "parent": parent })));
    let mask = create(&mut s, a, [0.0, 0.0], [30.0, 80.0], GREEN);
    ok(run(&mut s, a, "layer.vector_mask.set", json!({ "id": mask, "target": parent })));
    ok(run(&mut s, a, "view.mask_view.set", json!({ "id": parent })));
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("c08_k08_roundtrip.dtool");
    dtool_io::native::save(&doc(&s, a), &path).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("\"clip_role\":\"clip\"") || body.contains("\"clip_role\": \"clip\""), "role saved");
    assert!(!body.contains("mask_view"), "Alt view state is never saved");
    let reopened = dtool_io::native::open(&path).unwrap();
    let normal = CanvasFlags::default();
    assert_eq!(pixels(&reopened, &normal), pixels(&doc(&s, a), &normal));
    let px = pixels(&reopened, &normal);
    assert!(is(at(&px, 25, 25), BLUE) && is(at(&px, 10, 10), RED));
    assert_eq!(at(&px, 35, 10)[3], 0);
    assert_eq!(at(&px, 50, 50)[3], 0);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn bad_clip_and_mask_requests_are_rejected_without_change() {
    let (mut s, a, parent, child) = setup();
    let v = doc(&s, a).version;
    for (name, params) in [
        ("layer.clip.set", json!({ "id": child })),
        ("layer.clip.set", json!({ "id": child, "parent": child })),
        ("layer.clip.set", json!({ "id": 9999, "parent": parent })),
        ("layer.clip.release", json!({ "id": child })),
        ("layer.vector_mask.release", json!({ "id": child })),
        ("view.mask_view.set", json!({ "id": child })),
    ] {
        assert!(run(&mut s, a, name, params.clone()).get("error").is_some(), "{name} {params}");
    }
    ok(run(&mut s, a, "layer.clip.set", json!({ "id": child, "parent": parent })));
    assert!(run(&mut s, a, "layer.clip.set", json!({ "id": parent, "parent": child })).get("error").is_some(), "cycle");
    assert_eq!(doc(&s, a).version, v + 1);
}