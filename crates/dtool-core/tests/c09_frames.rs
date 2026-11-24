use dtool_core::command::Session;
use dtool_core::commands::picture_frame::{content_rect, frames, Anchor9, ImageFit, PictureFrame, ContentTransform};
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::resources::{ImageFormat, ImageResource, ResourceId};
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1))
}

#[test]
fn empty_frame_create_and_undo() {
    let mut session = session();
    let reply = session.run(&Command::with_params("frame.image.add", json!({ "shape": "ellipse", "pos": [10.0, 20.0], "size": [80.0, 40.0], "name": "Hole" }))).unwrap();
    let id = ObjectId(reply["id"].as_u64().unwrap());
    assert_eq!(reply["shape"], "ellipse");
    let object = session.document.get_object(id).unwrap();
    assert_eq!(object.pos, [10.0, 20.0]);
    assert_eq!(object.size, [80.0, 40.0]);
    assert!(frames(&session.document).contains_key(&id));
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(session.document.get_object(id).is_none());
    assert!(!frames(&session.document).contains_key(&id));
}

#[test]
fn minfit_and_maxfit_content_rects_differ() {
    let rect_min = content_rect([0.0, 0.0], [100.0, 100.0], [50.0, 100.0], ImageFit::MinFit, Anchor9::Center, ContentTransform::default());
    let rect_max = content_rect([0.0, 0.0], [100.0, 100.0], [50.0, 100.0], ImageFit::MaxFit, Anchor9::Center, ContentTransform::default());
    assert_eq!(rect_min, [25.0, 0.0, 50.0, 100.0]);
    assert_eq!(rect_max, [0.0, -50.0, 100.0, 200.0]);
    let top_left = content_rect([0.0, 0.0], [100.0, 100.0], [40.0, 20.0], ImageFit::Native, Anchor9::TopLeft, ContentTransform { offset: [5.0, 7.0], scale: [1.0, 1.0], rotation: 0.0 });
    assert_eq!(top_left, [5.0, 7.0, 40.0, 20.0]);
}

#[test]
fn place_into_frame_fit_content_and_does_not_steal_sibling() {
    let mut session = session();
    let mut shared = ImageResource::new_embedded(ImageFormat::Png, [50, 100], 72.0, vec![0; 50 * 100 * 4]);
    shared.id = ResourceId(3);
    session.document.resources.push(shared);
    session.document.next_resource_id = session.document.next_resource_id.max(4);
    let rgba = vec![0u8; 50 * 100 * 4];
    let first = session.document.create_object(Object { name: "A".into(), pos: [0.0, 0.0], size: [100.0, 100.0], raster_data: Some(rgba.clone()), raster_size: Some((50, 100)), ..Object::default() });
    let second = session.document.create_object(Object { name: "B".into(), pos: [200.0, 0.0], size: [100.0, 100.0], raster_data: Some(rgba), raster_size: Some((50, 100)), ..Object::default() });
    session.document.place_image(first, ResourceId(3)).unwrap();
    session.document.place_image(second, ResourceId(3)).unwrap();
    session.run(&Command::with_params("frame.convert", json!({ "nodes": [first.0, second.0] }))).unwrap();
    session.run(&Command::with_params("frame.image.fit.set", json!({ "node": first.0, "fit": "min" }))).unwrap();
    session.run(&Command::with_params("frame.image.fit.set", json!({ "node": second.0, "fit": "max" }))).unwrap();
    let min = session.run(&Command::with_params("frame.image.query", json!({ "node": first.0 }))).unwrap();
    let max = session.run(&Command::with_params("frame.image.query", json!({ "node": second.0 }))).unwrap();
    assert_eq!(min["content_rect"], json!([25.0, 0.0, 50.0, 100.0]));
    assert_eq!(max["content_rect"], json!([200.0, -50.0, 100.0, 200.0]));
    assert_eq!(session.document.placed_resource(first), Some(ResourceId(3)));
    assert_eq!(session.document.placed_resource(second), Some(ResourceId(3)));
    session.run(&Command::with_params("frame.image.content.set", json!({ "node": first.0, "offset": [2.0, 3.0], "scale": [1.0, 1.0], "rotation": 15.0 }))).unwrap();
    let moved = session.run(&Command::with_params("frame.image.query", json!({ "node": first.0 }))).unwrap();
    assert_eq!(moved["content_rect"], json!([27.0, 3.0, 50.0, 100.0]));
    assert_eq!(moved["content"]["rotation"], 15.0);
    assert_eq!(session.document.placed_resource(second), Some(ResourceId(3)));
    session.run(&Command::new("edit.undo")).unwrap();
    let restored = session.run(&Command::with_params("frame.image.query", json!({ "node": first.0 }))).unwrap();
    assert_eq!(restored["content_rect"], json!([25.0, 0.0, 50.0, 100.0]));
    let _ = PictureFrame::new(first, dtool_core::commands::picture_frame::FrameShape::Rect);
}
