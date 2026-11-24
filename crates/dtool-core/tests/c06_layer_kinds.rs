use dtool_core::document::Units;
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_core::{Command, Document, Session};
use serde_json::{json, Value};

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1))
}

fn run(s: &mut Session, name: &str, params: Value) -> Result<Value, String> {
    s.run(&Command::with_params(name, params)).map_err(|e| e.code)
}

fn object(s: &Session, id: u64) -> &Object {
    s.document.get_object(ObjectId(id)).unwrap()
}

#[test]
fn fill_layer_covers_page_one_undo() {
    let mut s = session();
    let page = dtool_core::scene::page_rects(&s.document)[0];
    let id = run(&mut s, "layer.fill.add", json!({"colour": "#336699"})).unwrap()["id"].as_u64().unwrap();
    let layer = object(&s, id);
    assert_eq!(layer.pos, [page[0], page[1]]);
    assert_eq!(layer.size, [page[2], page[3]]);
    assert_eq!(layer.fill, Paint::Solid { colour: Colour::new(0x33, 0x66, 0x99, 255) });
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert!(s.document.get_object(ObjectId(id)).is_none());
    assert!(run(&mut s, "layer.fill.add", json!({"page": 9})).is_err());
    assert!(run(&mut s, "layer.fill.add", json!({"colour": "nope"})).is_err());
}

#[test]
fn pattern_layer_keeps_resource_id() {
    let mut s = session();
    let rid = s.document.add_image_resource(ImageResource::new_embedded(ImageFormat::Png, [2, 2], 72.0, vec![1, 2, 3]));
    let id = run(&mut s, "layer.pattern.add", json!({"resource": rid.0})).unwrap()["id"].as_u64().unwrap();
    let Paint::Pattern(pattern) = &object(&s, id).fill else { panic!("pattern") };
    assert_eq!(pattern.resource_id, rid.0.to_string());
    assert_eq!(s.document.resource_uses(rid), vec![ObjectId(id)]);
    assert!(!s.document.unused_resources().contains(&rid));
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert!(s.document.resource_uses(rid).is_empty());
    assert_eq!(run(&mut s, "layer.pattern.add", json!({"resource": 999})), Err("NOT_FOUND".into()));
}

#[test]
fn merge_keeps_documented_survivor_and_undo_restores_both() {
    let mut s = session();
    let back = s.document.create_object(Object { name: "back".into(), size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    let front = s.document.create_object(Object { name: "front".into(), pos: [5.0, 5.0], size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, ..Object::default() });
    let reply = run(&mut s, "node.merge", json!({"nodes": [back.0, front.0]})).unwrap();
    let survivor = reply["survivor"].as_u64().unwrap();
    assert!(survivor == back.0 || survivor == front.0);
    assert!(reply["map"].is_array() || reply["map"].is_object());
    assert_eq!(s.document.objects.len(), 1);
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert!(s.document.get_object(back).is_some() && s.document.get_object(front).is_some());
}

#[test]
fn pixel_layer_is_blank_document_sized_and_undoes_in_one_step() {
    let mut s = session();
    let page = dtool_core::scene::page_rects(&s.document)[0];
    let objects = s.document.objects.len();
    let reply = run(&mut s, "layer.pixel.add", json!({})).unwrap();
    let id = reply["id"].as_u64().unwrap();
    assert_eq!(reply["pixel_size"], json!([200, 100]));
    let layer = object(&s, id);
    assert_eq!(layer.pos, [page[0], page[1]]);
    assert_eq!(layer.size, [page[2], page[3]]);
    assert_eq!(layer.size, s.document.pages[0].size);
    assert_eq!(s.selection.ids, vec![ObjectId(id)]);
    let pixels = s.document.pixel_layers.get(ObjectId(id)).unwrap();
    assert_eq!((pixels.width, pixels.height), (200, 100));
    assert!(pixels.to_rgba().iter().all(|v| *v == 0));
    assert_eq!(dtool_core::raster_source::dimensions(&s.document, layer), Some([200, 100]));
    assert!(s.undo.can_undo());
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert!(s.document.get_object(ObjectId(id)).is_none());
    assert!(s.document.pixel_layers.get(ObjectId(id)).is_none());
    assert_eq!(s.document.objects.len(), objects);
    assert!(!s.undo.can_undo());
    run(&mut s, "edit.redo", json!({})).unwrap();
    assert_eq!(s.document.pixel_layers.get(ObjectId(id)).map(|p| (p.width, p.height)), Some((200, 100)));
    assert!(run(&mut s, "layer.pixel.add", json!({"page": 9})).is_err());
}

#[test]
fn pixel_layer_command_is_registered() {
    let s = session();
    assert!(s.catalog.assert_named("layer.pixel.add").is_ok());
    assert!(s.catalog.assert_named("layer.fill.add").is_ok());
    assert!(s.catalog.assert_named("layer.pattern.add").is_ok());
}