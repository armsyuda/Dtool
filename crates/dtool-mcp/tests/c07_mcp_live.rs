use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Cap, Colour, Paint, StrokeStyle};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: Colour::new(r, g, b, 255) }
}

fn setup() -> (Server, ContextId, ObjectId) {
    let mut workspace = Workspace::default();
    let doc = workspace.new_document("c07");
    workspace.activate(doc).unwrap();
    let id = workspace.get_mut(doc).unwrap().session.document.create_object(Object { name: "a".into(), size: [20.0, 20.0], fill: solid(255, 0, 0), stroke: StrokeStyle { paint: solid(0, 0, 255), width: 1.0, ..StrokeStyle::default() }, ..Object::default() });
    workspace.get_mut(doc).unwrap().session.selection.set(vec![id], dtool_core::selection::SelectOp::Replace);
    (Server::new(Config::default(), workspace), doc, id)
}

fn run(server: &mut Server, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply
}

fn undo(server: &mut Server) {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.undo", "params": {} }));
    assert!(reply.get("error").is_none(), "{reply}");
}

fn object(server: &Server, doc: ContextId, id: ObjectId) -> Object {
    server.workspace().lock().unwrap().get(doc).unwrap().session.document.get_object(id).unwrap().clone()
}

fn version(server: &Server, doc: ContextId) -> u64 {
    server.workspace().lock().unwrap().get(doc).unwrap().session.document.version
}

#[test]
fn c07_mcp_paint_wells_edit_the_live_document_and_undo() {
    let (mut server, doc, id) = setup();
    run(&mut server, "paint.swap", json!({ "ids": [id.0] }));
    let swapped = object(&server, doc, id);
    assert_eq!((swapped.fill, swapped.stroke.paint), (solid(0, 0, 255), solid(255, 0, 0)));
    undo(&mut server);
    assert_eq!(object(&server, doc, id).fill, solid(255, 0, 0));
    run(&mut server, "paint.none", json!({ "ids": [id.0], "role": "fill" }));
    assert_eq!(object(&server, doc, id).fill, Paint::None);
    assert_eq!(object(&server, doc, id).stroke.paint, solid(0, 0, 255));
    undo(&mut server);
    assert_eq!(object(&server, doc, id).fill, solid(255, 0, 0));
}

#[test]
fn c07_mcp_stroke_set_full_style_edits_the_live_document() {
    let (mut server, doc, id) = setup();
    run(&mut server, "stroke.set", json!({ "ids": [id.0], "width": 3.0, "cap": "Round" }));
    let stroke = object(&server, doc, id).stroke;
    assert_eq!((stroke.width, stroke.cap), (3.0, Cap::Round));
    undo(&mut server);
    let stroke = object(&server, doc, id).stroke;
    assert_eq!((stroke.width, stroke.cap), (1.0, StrokeStyle::default().cap));
}

#[test]
fn c07_mcp_color_pick_reads_the_live_document() {
    let (mut server, doc, id) = setup();
    let before = version(&server, doc);
    let reply = run(&mut server, "color.pick", json!({ "x": 10.0, "y": 10.0, "apply": false }));
    assert_eq!(reply["result"]["colour"]["rgba"], json!([255, 0, 0, 255]), "{reply}");
    assert_eq!(version(&server, doc), before);
    run(&mut server, "ui.color.target.set", json!({ "target": "stroke" }));
    run(&mut server, "color.pick", json!({ "x": 10.0, "y": 10.0 }));
    assert_eq!(object(&server, doc, id).stroke.paint, solid(255, 0, 0));
    undo(&mut server);
    assert_eq!(object(&server, doc, id).stroke.paint, solid(0, 0, 255));
}