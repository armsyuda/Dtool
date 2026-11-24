use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1))
}

fn frame_with_image(s: &mut Session) -> u64 {
    let rid = s.document.add_image_resource(ImageResource::new_embedded(ImageFormat::Jpeg, [50, 100], 72.0, vec![0; 50 * 100 * 4]));
    s.run(&Command::with_params("frame.image.add", json!({ "pos": [0.0, 0.0], "size": [100.0, 100.0], "resource": rid.0 }))).unwrap()["id"].as_u64().unwrap()
}

#[test]
fn content_scale_must_be_positive_and_bad_scale_changes_nothing() {
    let mut s = session();
    let id = frame_with_image(&mut s);
    let before = s.run(&Command::with_params("frame.image.query", json!({ "id": id }))).unwrap();
    for scale in [[0.0, 1.0], [-1.0, 1.0], [1.0, 0.0]] {
        assert!(s.run(&Command::with_params("frame.image.content.set", json!({ "id": id, "scale": scale }))).is_err());
    }
    assert_eq!(s.run(&Command::with_params("frame.image.query", json!({ "id": id }))).unwrap(), before);
    let ok = s.run(&Command::with_params("frame.image.content.set", json!({ "id": id, "scale": [2.0, 2.0] }))).unwrap();
    assert_eq!(ok["content"]["scale"], json!([2.0, 2.0]));
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.run(&Command::with_params("frame.image.query", json!({ "id": id }))).unwrap(), before);
}

#[test]
fn deleted_frame_object_is_not_found_and_undo_brings_it_back() {
    let mut s = session();
    let id = frame_with_image(&mut s);
    s.run(&Command::with_params("node.delete", json!({ "nodes": [id] }))).unwrap();
    let error = s.run(&Command::with_params("frame.image.query", json!({ "id": id }))).unwrap_err();
    assert_eq!(error.code, "NOT_FOUND");
    assert!(s.run(&Command::with_params("frame.image.fit.set", json!({ "id": id, "fit": "max" }))).is_err());
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.run(&Command::with_params("frame.image.query", json!({ "id": id }))).unwrap()["id"], json!(id));
}

#[test]
fn resource_query_reports_type_and_placed_size() {
    let mut s = session();
    let id = frame_with_image(&mut s);
    let reply = s.run(&Command::new("resource.query")).unwrap();
    let row = &reply["images"][0];
    assert_eq!(row["format"], json!("Jpeg"));
    assert_eq!(row["uses"], json!([id]));
    let size = row["placed_size"].as_array().unwrap();
    assert!((size[0].as_f64().unwrap() - 100.0).abs() < 0.01, "{row}");
    assert!((size[1].as_f64().unwrap() - 100.0).abs() < 0.01, "{row}");
    s.run(&Command::with_params("object.update", json!({ "id": id, "size": [200.0, 50.0] }))).ok();
    let row = s.run(&Command::new("resource.query")).unwrap()["images"][0].clone();
    let w = row["placed_size"][0].as_f64().unwrap();
    let object_w = s.document.get_object(dtool_core::object::ObjectId(id)).unwrap().size[0] as f64;
    assert!((w - object_w).abs() < 0.01);
}