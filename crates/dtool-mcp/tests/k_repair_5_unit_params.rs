use dtool_core::document::{Document, Units};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn server(units: Units) -> (Server, u64) {
    let mut workspace = Workspace::default();
    let id = workspace.insert_context("k_repair_5", None, Document::new([400.0, 300.0], units, 96.0, 1), 0);
    workspace.activate(id).unwrap();
    (Server::new(Config::default(), workspace), id.0)
}

fn run(server: &mut Server, id: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": id, "params": params } }))
}

fn guides(server: &Server, id: u64) -> Vec<f32> {
    server.workspace().lock().unwrap().get(ContextId(id)).unwrap().session.guides.list().iter().map(|g| g.position).collect()
}

fn object(server: &Server, id: u64) -> ([f32; 2], [f32; 2]) {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    let obj = ws.get(ContextId(id)).unwrap().session.document.objects.last().unwrap().clone();
    (obj.pos, obj.size)
}

fn last_id(server: &Server, id: u64) -> u64 {
    server.workspace().lock().unwrap().get(ContextId(id)).unwrap().session.document.objects.last().unwrap().id.0
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn ok(reply: &Value) {
    assert!(reply.get("error").is_none(), "{reply}");
}

fn invalid(reply: &Value) {
    assert_eq!(reply["error"]["code"], "INVALID_PARAMS", "{reply}");
}

#[test]
fn mcp_guides_add_and_update_take_unit_strings_in_mm_doc() {
    let (mut s, id) = server(Units::Mm);
    ok(&run(&mut s, id, "guides.add", json!({ "kind": "Horizontal", "position": "10mm" })));
    ok(&run(&mut s, id, "guides.add", json!({ "kind": "Vertical", "position": "1in" })));
    ok(&run(&mut s, id, "guides.add", json!({ "kind": "Vertical", "position": "96px" })));
    ok(&run(&mut s, id, "guides.add", json!({ "kind": "Vertical", "position": 5.0 })));
    let g = guides(&s, id);
    assert!(near(g[0], 10.0) && near(g[1], 25.4) && near(g[2], 25.4) && near(g[3], 5.0), "{g:?}");
    let gid = s.workspace().lock().unwrap().get(ContextId(id)).unwrap().session.guides.list()[0].id.0;
    ok(&run(&mut s, id, "guides.update", json!({ "id": gid, "position": "2cm" })));
    assert!(near(guides(&s, id)[0], 20.0), "{:?}", guides(&s, id));
    ok(&run(&mut s, id, "edit.undo", json!({})));
    assert!(near(guides(&s, id)[0], 10.0), "{:?}", guides(&s, id));
}

#[test]
fn mcp_guides_add_unit_strings_in_px_doc() {
    let (mut s, id) = server(Units::Px);
    ok(&run(&mut s, id, "guides.add", json!({ "kind": "Horizontal", "position": "150px" })));
    ok(&run(&mut s, id, "guides.add", json!({ "kind": "Horizontal", "position": "1in" })));
    ok(&run(&mut s, id, "guides.add", json!({ "kind": "Horizontal", "position": 42.0 })));
    let g = guides(&s, id);
    assert!(near(g[0], 150.0) && near(g[1], 96.0) && near(g[2], 42.0), "{g:?}");
}

#[test]
fn mcp_transform_set_takes_unit_strings() {
    let (mut s, id) = server(Units::Mm);
    ok(&run(&mut s, id, "object.create", json!({ "pos": [0.0, 0.0], "size": [4.0, 4.0] })));
    let obj = last_id(&s, id);
    ok(&run(&mut s, id, "transform.set", json!({ "ids": [obj], "x": "10mm", "y": "1in", "w": "2cm", "h": 30.0 })));
    let (pos, size) = object(&s, id);
    assert!(near(pos[0], 10.0) && near(pos[1], 25.4), "{pos:?}");
    assert!(near(size[0], 20.0) && near(size[1], 30.0), "{size:?}");
    ok(&run(&mut s, id, "transform.set", json!({ "ids": [obj], "x": 7.0 })));
    assert!(near(object(&s, id).0[0], 7.0));
}

#[test]
fn mcp_bad_unit_strings_are_invalid_params() {
    let (mut s, id) = server(Units::Px);
    invalid(&run(&mut s, id, "guides.add", json!({ "kind": "Horizontal", "position": "10furlong" })));
    invalid(&run(&mut s, id, "guides.add", json!({ "kind": "Horizontal", "position": "abc" })));
    assert!(guides(&s, id).is_empty());
    ok(&run(&mut s, id, "object.create", json!({ "pos": [1.0, 2.0], "size": [4.0, 4.0] })));
    let obj = last_id(&s, id);
    invalid(&run(&mut s, id, "transform.set", json!({ "ids": [obj], "x": "ten mm" })));
    assert!(near(object(&s, id).0[0], 1.0));
}

#[test]
fn session_normalize_is_idempotent_on_numbers() {
    let mut session = dtool_core::Session::new(Document::new([400.0, 300.0], Units::Mm, 96.0, 1));
    session.has_document = true;
    let command = dtool_core::Command::with_params("guides.add", json!({ "kind": "Horizontal", "position": "10mm" }));
    let once = dtool_core::commands::ruler_units::normalize(&session, &command).unwrap();
    let twice = dtool_core::commands::ruler_units::normalize(&session, &once).unwrap();
    assert_eq!(once.params, twice.params);
    assert!(once.params["position"].is_number());
    session.run(&once).unwrap();
    assert!(near(session.guides.list()[0].position, 10.0));
}