use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use dtool_core::{Command, Document, ObjectId, Scope, Session, Workspace};
use dtool_core::document::Units;
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde_json::json;

struct Fixture(PathBuf);
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
fn fixture(format: ImageFormat) -> (Fixture,Vec<u8>) {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_raw(2,1,vec![255,0,0,255,0,255,0,128]).unwrap()).write_to(&mut bytes,format).unwrap();
    let bytes = bytes.into_inner();
    let sequence = NEXT_FILE.fetch_add(1,Ordering::Relaxed);
    let path = PathBuf::from("C:/Users/armsy/AppData/Local/Temp/opencode").join(format!("dtool_image_{}_{sequence}_{format:?}.bin",std::process::id()));
    std::fs::write(&path,&bytes).unwrap();
    (Fixture(path),bytes)
}

#[test]
fn supported_file_formats_really_decode_not_just_register_extensions() {
    for format in [ImageFormat::Png,ImageFormat::Gif,ImageFormat::WebP,ImageFormat::Tiff] {
        let (file,bytes) = fixture(format);
        let image = dtool_io::image_import::decode_image(&file.0).unwrap();
        assert_eq!(image.resource.pixel_size,[2,1]);
        assert_eq!(image.resource.data,Some(bytes));
        assert_eq!(&image.rgba[..4],&[255,0,0,255]);
        assert_eq!(image.rgba.len(),8);
    }
}

#[test]
fn path_placement_preserves_source_pixels_and_one_undo() {
    let (file,bytes) = fixture(ImageFormat::Png);
    dtool_io::image_import::install();
    let mut s = Session::new(Document::new([100.0,100.0],Units::Px,72.0,1));
    let id = ObjectId(s.run_named("image.place",json!({"path":file.0,"pos":[10,20]})).unwrap()["id"].as_u64().unwrap());
    assert_eq!(s.document.resources[0].data,Some(bytes));
    assert_eq!(s.document.objects[0].size,[2.0,1.0]);
    assert_eq!(s.document.pixel_layers.get(id).unwrap().get(1,0),[0,255,0,128]);
    s.run(&Command::new("edit.undo")).unwrap();
    assert!(s.document.objects.is_empty());
    assert!(s.document.resources.is_empty());
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(s.document.objects[0].id,id);
}

#[test]
fn file_placement_honours_explicit_document_scope() {
    let (file,_) = fixture(ImageFormat::Png);
    dtool_io::install_import_commands();
    let mut workspace = Workspace::default();
    let first = workspace.new_document("first");
    let second = workspace.new_document("second");
    let (target,result) = workspace.run(Scope::of(first),Command::with_params("image.place",json!({"path":file.0,"pos":[0,0]})));
    result.unwrap();
    assert_eq!(target,Some(first));
    assert_eq!(workspace.get(first).unwrap().session.document.objects.len(),1);
    assert!(workspace.get(second).unwrap().session.document.objects.is_empty());
}

#[test]
fn invalid_image_and_conflicting_input_do_not_mutate_the_document() {
    dtool_io::image_import::install();
    let mut s = Session::new(Document::default());
    assert!(s.run_named("image.place",json!({"path":"not-an-existing-image.png","pos":[0,0]})).is_err());
    let (file,_) = fixture(ImageFormat::Png);
    assert!(s.run_named("image.place",json!({"path":file.0,"pos":[0,0],"rgba":[0,0,0,0]})).is_err());
    assert!(s.document.objects.is_empty());
    assert!(!s.undo.can_undo());
}
