use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::raster_source::LayerMask;
use dtool_core::resources::ImageResource;
use dtool_core::selection::PixelSelection;
use dtool_protocol::Command;
use serde_json::json;

fn rgba(color: [u8; 4], width: u32, height: u32) -> Vec<u8> {
    color.repeat((width * height) as usize)
}

fn raster_object(name: &str, pos: [f32; 2], size: [f32; 2], width: u32, height: u32, color: [u8; 4]) -> Object {
    Object {
        name: name.into(),
        pos,
        size,
        raster_data: Some(rgba(color, width, height)),
        raster_size: Some((width, height)),
        ..Object::default()
    }
}

#[test]
fn merge_keeps_backmost_id_and_identity_map() {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let back = session.document.create_object(raster_object("A", [0.0, 0.0], [2.0, 2.0], 2, 2, [255, 0, 0, 255]));
    let front = session.document.create_object(raster_object("B", [2.0, 0.0], [2.0, 2.0], 2, 2, [0, 255, 0, 255]));
    let before_id = session.document.next_object_id;
    let reply = session.run(&Command::with_params("node.merge", json!({ "nodes": [front.0, back.0] }))).unwrap();
    assert_eq!(reply["survivor"], back.0);
    assert_eq!(reply["id"], back.0);
    let map = reply["map"].as_array().unwrap();
    assert!(map.iter().any(|row| row["from"] == back.0 && row["to"] == back.0));
    assert!(map.iter().any(|row| row["from"] == front.0 && row["to"] == back.0));
    assert!(session.document.get_object(back).is_some());
    assert!(session.document.get_object(front).is_none());
    assert_eq!(session.document.next_object_id, before_id);
    let survivor = session.document.get_object(back).unwrap();
    assert_eq!(survivor.pos, [0.0, 0.0]);
    assert_eq!(survivor.size, [4.0, 2.0]);
    let pixels = survivor.raster_data.as_ref().unwrap();
    assert_eq!(&pixels[0..4], &[255, 0, 0, 255]);
    let last = pixels.len() - 4;
    assert_eq!(&pixels[last..], &[0, 255, 0, 255]);
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(session.document.get_object(back).is_some());
    assert!(session.document.get_object(front).is_some());
    assert_eq!(session.document.get_object(front).unwrap().name, "B");
}

#[test]
fn merge_retargets_placed_image_and_mask_without_dropping_resource() {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let back = session.document.create_object(raster_object("A", [0.0, 0.0], [2.0, 2.0], 2, 2, [255, 0, 0, 255]));
    let front = session.document.create_object(raster_object("B", [2.0, 0.0], [2.0, 2.0], 2, 2, [0, 255, 0, 255]));
    let resource = session.document.add_image_resource(ImageResource { pixel_size: [2, 2], dpi: 72.0, ..ImageResource::default() });
    session.document.place_image(front, resource).unwrap();
    session.document.layer_masks.push(LayerMask::new(front, PixelSelection::all(2, 2)));
    let reply = session.run(&Command::with_params("node.merge", json!({ "nodes": [back.0, front.0] }))).unwrap();
    assert_eq!(reply["resource"], resource.0);
    assert!(session.document.image_resource(resource).is_some());
    assert_eq!(session.document.placed_resource(back), Some(resource));
    assert!(session.document.placed_images.iter().all(|placed| placed.object_id != front));
    assert!(session.document.layer_masks.iter().any(|mask| mask.object == back));
    assert!(session.document.layer_masks.iter().all(|mask| mask.object != front));
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.document.placed_resource(front), Some(resource));
    assert!(session.document.layer_masks.iter().any(|mask| mask.object == front));
    assert!(session.document.get_object(front).is_some());
}

#[test]
fn rasterize_keep_original_preserves_resource_on_source() {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let source = session.document.create_object(raster_object("Photo", [4.0, 8.0], [2.0, 2.0], 2, 2, [0, 0, 255, 255]));
    let resource = session.document.add_image_resource(ImageResource { pixel_size: [2, 2], dpi: 72.0, ..ImageResource::default() });
    session.document.place_image(source, resource).unwrap();
    let before = session.document.next_object_id;
    let reply = session.run(&Command::with_params("node.rasterize", json!({ "nodes": [source.0], "keep_original": true }))).unwrap();
    let created = ObjectId(reply["id"].as_u64().unwrap());
    assert_ne!(created, source);
    assert!(session.document.next_object_id > before);
    assert!(session.document.get_object(source).is_some());
    assert!(session.document.get_object(created).is_some());
    assert_eq!(session.document.placed_resource(source), Some(resource));
    assert!(session.document.placed_resource(created).is_none());
    assert_eq!(reply["resource"], resource.0);
    let raster = session.document.get_object(created).unwrap();
    assert_eq!(raster.pos, [4.0, 8.0]);
    assert_eq!(raster.raster_size, Some((2, 2)));
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(session.document.get_object(created).is_none());
    assert!(session.document.next_object_id > before);
    assert_eq!(session.document.placed_resource(source), Some(resource));
}

#[test]
fn rasterize_in_place_keeps_id_and_resource() {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let source = session.document.create_object(raster_object("Photo", [0.0, 0.0], [2.0, 2.0], 2, 2, [10, 20, 30, 255]));
    let resource = session.document.add_image_resource(ImageResource { pixel_size: [2, 2], dpi: 72.0, ..ImageResource::default() });
    session.document.place_image(source, resource).unwrap();
    let reply = session.run(&Command::with_params("node.rasterize", json!({ "nodes": [source.0] }))).unwrap();
    assert_eq!(reply["id"], source.0);
    assert_eq!(reply["resource"], resource.0);
    assert_eq!(session.document.placed_resource(source), Some(resource));
    assert!(session.document.get_object(source).unwrap().raster_data.is_some());
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.document.placed_resource(source), Some(resource));
}
