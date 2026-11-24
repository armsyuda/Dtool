use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use dtool_core::{Command, Document, ObjectId, Session};
use dtool_core::document::Units;
use dtool_core::resources::ResourceStatus;
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde_json::json;

struct Fixture(PathBuf);
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }

fn fixture_color(format: ImageFormat, rgba: Vec<u8>) -> (Fixture, Vec<u8>) {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_raw(2, 1, rgba).unwrap()).write_to(&mut bytes, format).unwrap();
    let bytes = bytes.into_inner();
    let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let path = PathBuf::from("C:/Users/armsy/AppData/Local/Temp/opencode").join(format!("dtool_c09_{}_{sequence}_{format:?}.bin", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    (Fixture(path), bytes)
}

fn fixture(format: ImageFormat) -> (Fixture, Vec<u8>) {
    fixture_color(format, vec![255, 0, 0, 255, 0, 255, 0, 128])
}

#[test]
fn linked_place_keeps_path_and_reports_missing_without_embedding() {
    let (file, _) = fixture(ImageFormat::Png);
    dtool_io::image_import::install();
    let mut session = Session::new(Document::new([100.0, 100.0], Units::Px, 72.0, 1));
    session.run_named("image.place", json!({"path": file.0, "pos": [10, 20], "link": true})).unwrap();
    assert!(session.document.resources[0].is_linked());
    assert_eq!(session.document.resources[0].embedded, false);
    assert!(session.document.resources[0].data.is_none());
    assert_eq!(session.document.resources[0].path.as_ref(), Some(&file.0));
    assert_eq!(session.document.resources[0].check_link(None), ResourceStatus::Linked);
    std::fs::remove_file(&file.0).unwrap();
    assert_eq!(session.document.resources[0].check_link(None), ResourceStatus::Missing);
    assert_eq!(session.document.resources[0].embedded, false);
    assert!(session.document.resources[0].data.is_none());
}

#[test]
fn default_place_still_embeds() {
    let (file, bytes) = fixture(ImageFormat::Png);
    dtool_io::image_import::install();
    let mut session = Session::new(Document::new([100.0, 100.0], Units::Px, 72.0, 1));
    session.run_named("image.place", json!({"path": file.0, "pos": [0, 0]})).unwrap();
    assert!(!session.document.resources[0].is_linked());
    assert_eq!(session.document.resources[0].data, Some(bytes));
}

#[test]
fn replace_one_shared_resource_leaves_the_other_and_undo_restores_only_it() {
    let (first, _) = fixture_color(ImageFormat::Png, vec![255, 0, 0, 255, 0, 255, 0, 128]);
    let (second, _) = fixture_color(ImageFormat::Png, vec![0, 0, 255, 255, 0, 0, 0, 255]);
    dtool_io::image_import::install();
    let mut session = Session::new(Document::new([100.0, 100.0], Units::Px, 72.0, 1));
    let a = ObjectId(session.run_named("image.place", json!({"path": first.0, "pos": [0, 0]})).unwrap()["id"].as_u64().unwrap());
    let resource = session.document.placed_resource(a).unwrap();
    let rgba = session.document.get_object(a).unwrap().raster_data.clone().unwrap();
    let b = ObjectId(session.run_named("image.place", json!({"resource": resource.0, "pixel_size": [2, 1], "rgba": rgba, "pos": [10, 0]})).unwrap()["id"].as_u64().unwrap());
    assert_eq!(session.document.placed_resource(b), Some(resource));
    assert_eq!(session.document.resource_uses(resource).len(), 2);
    let reply = session.run(&Command::with_params("resource.replace", json!({"object_id": a.0, "path": second.0}))).unwrap();
    let new_resource = dtool_core::resources::ResourceId(reply["resource"].as_u64().unwrap());
    assert_ne!(new_resource, resource);
    assert_eq!(session.document.placed_resource(a), Some(new_resource));
    assert_eq!(session.document.placed_resource(b), Some(resource));
    assert_eq!(session.document.resource_uses(resource), vec![b]);
    assert!(session.document.image_resource(resource).is_some());
    assert_eq!(session.document.get_object(a).unwrap().raster_data.as_ref().unwrap()[..4], [0, 0, 255, 255]);
    assert_eq!(session.document.get_object(b).unwrap().raster_data.as_ref().unwrap()[..4], [255, 0, 0, 255]);
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.document.placed_resource(a), Some(resource));
    assert_eq!(session.document.placed_resource(b), Some(resource));
    assert_eq!(session.document.get_object(a).unwrap().raster_data.as_ref().unwrap()[..4], [255, 0, 0, 255]);
    assert!(session.document.image_resource(new_resource).is_none() || session.document.resource_uses(new_resource).is_empty());
}
