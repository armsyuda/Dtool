use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use dtool_core::{Document, ObjectId, Session};
use dtool_core::document::Units;
use dtool_core::resources::{ResourceId, ResourceStatus};
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde_json::json;

struct Fixture(PathBuf);
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
fn fixture(width: u32) -> Fixture {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_raw(width,1,vec![200;width as usize*4]).unwrap()).write_to(&mut bytes,ImageFormat::Png).unwrap();
    let sequence = NEXT_FILE.fetch_add(1,Ordering::Relaxed);
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c09_{}_{sequence}.png",std::process::id()));
    std::fs::write(&path,bytes.into_inner()).unwrap();
    Fixture(path)
}
fn session() -> Session {
    dtool_io::image_import::install();
    Session::new(Document::new([100.0,100.0],Units::Px,72.0,1))
}

#[test]
fn linked_place_keeps_path_and_reports_missing_when_file_is_gone() {
    let file = fixture(2);
    let mut s = session();
    let id = ObjectId(s.run_named("image.place",json!({"path":file.0,"pos":[0,0],"link":true})).unwrap()["id"].as_u64().unwrap());
    let rid = s.document.placed_resource(id).unwrap();
    let resource = s.document.image_resource(rid).unwrap().clone();
    assert!(!resource.embedded);
    assert!(resource.data.is_none());
    assert_eq!(resource.path.as_ref(),Some(&file.0));
    assert_eq!(resource.status,ResourceStatus::Linked);
    assert!(s.document.objects[0].raster_data.is_some());
    let path = file.0.clone();
    drop(file);
    assert_eq!(resource.check_link(None),ResourceStatus::Missing);
    assert!(s.run_named("image.place",json!({"rgba":[0,0,0,0],"pixel_size":[1,1],"pos":[0,0],"link":true})).is_err());
    assert!(!path.exists());
}

#[test]
fn replace_retargets_only_one_placement_and_undo_restores_it() {
    let first = fixture(2);
    let second = fixture(3);
    let mut s = session();
    let a = ObjectId(s.run_named("image.place",json!({"path":first.0,"pos":[0,0]})).unwrap()["id"].as_u64().unwrap());
    let old = s.document.placed_resource(a).unwrap();
    let b = ObjectId(s.run_named("image.place",json!({"resource":old,"pixel_size":[2,1],"rgba":vec![200u8;8],"pos":[30,0]})).unwrap()["id"].as_u64().unwrap());
    assert_eq!(s.document.resource_uses(old),vec![a,b]);
    let before = s.document.objects.iter().find(|o| o.id==a).map(|o| (o.pos,o.size)).unwrap();
    let result = s.run_named("resource.replace",json!({"object_id":a.0,"path":second.0})).unwrap();
    let new = ResourceId(result["resource"].as_u64().unwrap());
    assert_ne!(new,old);
    assert_eq!(s.document.placed_resource(a),Some(new));
    assert_eq!(s.document.placed_resource(b),Some(old));
    assert_eq!(s.document.resource_uses(old),vec![b]);
    assert_eq!(s.document.resource_uses(new),vec![a]);
    assert!(s.document.image_resource(old).is_some());
    let object = s.document.objects.iter().find(|o| o.id==a).unwrap();
    assert_eq!((object.pos,object.size),before);
    assert_eq!(object.raster_size,Some((3,1)));
    s.run_named("edit.undo",json!({})).unwrap();
    assert_eq!(s.document.placed_resource(a),Some(old));
    assert_eq!(s.document.placed_resource(b),Some(old));
    assert!(s.document.image_resource(new).is_none());
    assert_eq!(s.document.objects.len(),2);
    assert_eq!(s.document.objects.iter().find(|o| o.id==a).unwrap().raster_size,Some((2,1)));
    s.run_named("edit.undo",json!({})).unwrap();
    assert_eq!(s.document.objects.len(),1);
}

#[test]
fn replace_keeps_link_mode_and_rejects_unplaced_objects() {
    let first = fixture(2);
    let second = fixture(4);
    let mut s = session();
    let a = ObjectId(s.run_named("image.place",json!({"path":first.0,"pos":[0,0],"link":true})).unwrap()["id"].as_u64().unwrap());
    s.run_named("resource.replace",json!({"object_id":a.0,"path":second.0})).unwrap();
    let resource = s.document.image_resource(s.document.placed_resource(a).unwrap()).unwrap();
    assert!(resource.is_linked());
    assert_eq!(resource.path.as_ref(),Some(&second.0));
    s.run_named("resource.replace",json!({"object_id":a.0,"path":first.0,"link":false})).unwrap();
    let resource = s.document.image_resource(s.document.placed_resource(a).unwrap()).unwrap();
    assert!(!resource.is_linked());
    assert!(resource.embedded);
    s.run_named("resource.replace",json!({"object_id":a.0,"path":second.0})).unwrap();
    assert!(!s.document.image_resource(s.document.placed_resource(a).unwrap()).unwrap().is_linked());
    assert!(s.run_named("resource.replace",json!({"object_id":a.0,"path":second.0,"link":"yes"})).is_err());
    assert!(s.run_named("resource.replace",json!({"object_id":999,"path":second.0})).is_err());
}
