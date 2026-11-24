use dtool_core::document::Units;
use dtool_core::selection::PixelSelection;
use dtool_core::{Command, Document, ObjectId, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1))
}

fn place(s: &mut Session) -> u64 {
    s.run_named("image.place", json!({"pixel_size":[2,1],"rgba":[255,0,0,255,0,255,0,200],"pos":[10,20]})).unwrap()["id"].as_u64().unwrap()
}

fn select_right_half(s: &mut Session) {
    let mut selection = PixelSelection::empty(400, 300);
    for y in 0..300 {
        for x in 11..400 {
            selection.coverage[y * 400 + x] = 255;
        }
    }
    s.document.pixel_selection = Some(selection);
}

fn mask(s: &Session, id: u64) -> Option<Vec<u8>> {
    s.document.layer_masks.iter().find(|m| m.object == ObjectId(id)).map(|m| m.coverage.coverage.clone())
}

fn pixels(s: &Session, id: u64) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let object = s.document.get_object(ObjectId(id)).unwrap();
    (object.raster_data.clone(), s.document.pixel_layers.get(ObjectId(id)).map(|layer| layer.to_rgba()))
}

#[test]
fn from_selection_then_invert_undo_twice_restores() {
    let mut s = session();
    let id = place(&mut s);
    let raw = pixels(&s, id);
    select_right_half(&mut s);
    let result = s.run_named("layer.mask.from_selection", json!({"id":id})).unwrap();
    assert_eq!(result["changes"][0]["name"], "layer.mask.from_selection");
    assert_eq!(mask(&s, id), Some(vec![0, 255]));
    let mask_state = s.document.layer_masks[0].coverage.clone();
    assert_eq!([mask_state.width, mask_state.height], [2, 1]);
    s.run_named("layer.mask.invert", json!({"id":id})).unwrap();
    assert_eq!(mask(&s, id), Some(vec![255, 0]));
    assert_eq!(pixels(&s, id), raw);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(mask(&s, id), Some(vec![0, 255]));
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(mask(&s, id), None);
    assert_eq!(pixels(&s, id), raw);
}

#[test]
fn release_removes_mask_in_one_undo_step() {
    let mut s = session();
    let id = place(&mut s);
    let raw = pixels(&s, id);
    s.run_named("layer.mask.set", json!({"id":id,"mask":{"width":2,"height":1,"coverage":[10,200]}})).unwrap();
    s.run_named("layer.mask.release", json!({"id":id})).unwrap();
    assert_eq!(mask(&s, id), None);
    assert_eq!(pixels(&s, id), raw);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(mask(&s, id), Some(vec![10, 200]));
}

#[test]
fn invert_and_release_without_mask_are_refused() {
    let mut s = session();
    let id = place(&mut s);
    let version = s.document.version;
    assert!(s.run_named("layer.mask.invert", json!({"id":id})).is_err());
    assert!(s.run_named("layer.mask.release", json!({"id":id})).is_err());
    assert_eq!(s.document.version, version);
}

#[test]
fn from_selection_needs_a_pixel_selection() {
    let mut s = session();
    let id = place(&mut s);
    let err = s.run_named("layer.mask.from_selection", json!({"id":id})).unwrap_err();
    assert_eq!(err.code, "NOTHING_SELECTED");
    s.document.pixel_selection = Some(PixelSelection::empty(400, 300));
    assert!(s.run_named("layer.mask.from_selection", json!({"id":id})).is_err());
    assert_eq!(mask(&s, id), None);
}

#[test]
fn locked_objects_are_refused() {
    let mut s = session();
    let id = place(&mut s);
    s.run_named("layer.mask.set", json!({"id":id,"mask":{"width":2,"height":1,"coverage":[0,255]}})).unwrap();
    select_right_half(&mut s);
    s.document.objects.iter_mut().find(|o| o.id == ObjectId(id)).unwrap().locked = true;
    for name in ["layer.mask.invert", "layer.mask.release", "layer.mask.from_selection"] {
        assert_eq!(s.run_named(name, json!({"id":id})).unwrap_err().code, "LOCKED");
    }
    assert_eq!(mask(&s, id), Some(vec![0, 255]));
}

#[test]
fn query_still_reports_mask() {
    let mut s = session();
    let id = place(&mut s);
    select_right_half(&mut s);
    s.run_named("layer.mask.from_selection", json!({"id":id})).unwrap();
    let result = s.run_named("layer.mask.query", json!({"id":id})).unwrap();
    assert_eq!(result["mask"]["coverage"], json!([0, 255]));
}