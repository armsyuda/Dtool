use dtool_core::command::Session;
use dtool_core::commands::picture_frame::get_frame;
use dtool_core::document::{Document, Units};
use dtool_core::native_io::{native_body, parse_native};
use dtool_core::object::ObjectId;
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_core::workspace::{Scope, SharedServices, Workspace};
use dtool_protocol::Command;
use serde_json::{json, Value};
use std::sync::Arc;

fn frame_doc(s: &mut Session) -> u64 {
    let rid = s.document.add_image_resource(ImageResource::new_embedded(ImageFormat::Png, [50, 100], 72.0, vec![0; 50 * 100 * 4]));
    s.run(&Command::with_params("frame.image.add", json!({ "shape": "ellipse", "pos": [0.0, 0.0], "size": [100.0, 100.0], "resource": rid.0 }))).unwrap()["id"].as_u64().unwrap()
}

fn edit_frame(s: &mut Session, id: u64) {
    s.run(&Command::with_params("frame.image.fit.set", json!({ "id": id, "fit": "max" }))).unwrap();
    s.run(&Command::with_params("frame.image.anchor.set", json!({ "id": id, "anchor": "top_left" }))).unwrap();
    s.run(&Command::with_params("frame.image.content.set", json!({ "id": id, "offset": [5.0, 7.0], "scale": [1.5, 1.5] }))).unwrap();
}

fn query(s: &mut Session, id: u64) -> Value {
    s.run(&Command::with_params("frame.image.query", json!({ "id": id }))).unwrap()
}

#[test]
fn frame_settings_live_on_their_own_document() {
    let mut a = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let mut b = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let ida = frame_doc(&mut a);
    let idb = frame_doc(&mut b);
    assert_eq!(ida, idb);
    let b_before = query(&mut b, idb);
    edit_frame(&mut a, ida);
    let qa = query(&mut a, ida);
    assert_eq!(qa["fit"], json!("max"));
    assert_eq!(qa["content"]["offset"], json!([5.0, 7.0]));
    assert_eq!(query(&mut b, idb), b_before);
    assert_eq!(a.document.picture_frames.len(), 1);
    assert_ne!(a.document.picture_frames, b.document.picture_frames);
}

#[test]
fn frame_settings_survive_native_save_and_open() {
    let mut a = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = frame_doc(&mut a);
    edit_frame(&mut a, id);
    let expected = query(&mut a, id);
    let body = native_body(&a.document, &json!({})).unwrap();
    let (loaded, _) = parse_native(&body).unwrap();
    assert_eq!(loaded.picture_frames, a.document.picture_frames);
    let mut reopened = Session::new(loaded);
    assert_eq!(query(&mut reopened, id), expected);
    let old: Value = {
        let mut value: Value = serde_json::from_str(&body).unwrap();
        value.as_object_mut().unwrap().remove("picture_frames");
        value
    };
    let (legacy, _) = parse_native(&old.to_string()).unwrap();
    assert!(legacy.picture_frames.is_empty());
}

#[test]
fn frame_edits_undo_and_redo_on_the_document() {
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = frame_doc(&mut s);
    let start = query(&mut s, id);
    edit_frame(&mut s, id);
    let edited = query(&mut s, id);
    for _ in 0..3 {
        s.run(&Command::new("edit.undo")).unwrap();
    }
    assert_eq!(query(&mut s, id), start);
    for _ in 0..3 {
        s.run(&Command::new("edit.redo")).unwrap();
    }
    assert_eq!(query(&mut s, id), edited);
    assert_eq!(get_frame(&s.document, ObjectId(id)).unwrap().content.offset, [5.0, 7.0]);
    s.run(&Command::new("edit.undo")).unwrap();
    s.run(&Command::new("edit.undo")).unwrap();
    s.run(&Command::new("edit.undo")).unwrap();
    s.run(&Command::new("edit.undo")).unwrap();
    assert!(s.document.picture_frames.is_empty());
    assert!(s.document.get_object(ObjectId(id)).is_none());
}

#[test]
fn workspace_tabs_do_not_share_frames_and_save_open_keeps_them() {
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let first = ws.new_document("first");
    let second = ws.new_document("second");
    let mut run = |ws: &mut Workspace, id, name: &str, params: Value| ws.run(Scope::of(id), Command::with_params(name, params)).1;
    let id = run(&mut ws, first, "frame.image.add", json!({ "pos": [0, 0], "size": [50, 50] })).unwrap()["id"].as_u64().unwrap();
    run(&mut ws, first, "frame.image.fit.set", json!({ "id": id, "fit": "stretch" })).unwrap();
    run(&mut ws, first, "frame.image.anchor.set", json!({ "id": id, "anchor": "bottom_right" })).unwrap();
    assert!(ws.get(second).unwrap().session.document.picture_frames.is_empty());
    assert!(run(&mut ws, second, "frame.image.query", json!({ "id": id })).is_err());
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c09b_frames_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("frames.dtool");
    run(&mut ws, first, "doc.save_as", json!({ "path": path })).unwrap();
    let expected = run(&mut ws, first, "frame.image.query", json!({ "id": id })).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("picture_frames"));
    let opened = ws.open_path(&path).unwrap();
    let reopened = run(&mut ws, opened, "frame.image.query", json!({ "id": id })).unwrap();
    assert_eq!(reopened["fit"], expected["fit"]);
    assert_eq!(reopened["anchor"], expected["anchor"]);
    assert_eq!(reopened["content"], expected["content"]);
}