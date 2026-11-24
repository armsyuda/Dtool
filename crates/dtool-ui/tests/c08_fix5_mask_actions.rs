#[path = "../src/mask_actions.rs"]
mod mask_actions;

use dtool_core::document::Units;
use dtool_core::selection::PixelSelection;
use dtool_core::{Command, Document, ObjectId, Session};
use mask_actions::{enabled, run, show_mask_actions, MaskAction};
use serde_json::json;

fn session_with_image() -> (Session, ObjectId) {
    let mut s = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    let id = ObjectId(s.run_named("image.place", json!({"pixel_size":[2,1],"rgba":[255,0,0,255,0,255,0,200],"pos":[10,20]})).unwrap()["id"].as_u64().unwrap());
    s.selection.ids = vec![id];
    (s, id)
}

fn mask(s: &Session, id: ObjectId) -> Option<Vec<u8>> {
    s.document.layer_masks.iter().find(|m| m.object == id).map(|m| m.coverage.coverage.clone())
}

#[test]
fn invert_then_undo_restores_mask_bytes() {
    let (mut s, id) = session_with_image();
    assert!(!enabled(&s, MaskAction::Invert));
    s.run_named("layer.mask.set", json!({"id":id.0,"mask":{"width":2,"height":1,"coverage":[10,200]}})).unwrap();
    assert!(enabled(&s, MaskAction::Invert));
    let depth = s.undo.applied_steps();
    run(&mut s, MaskAction::Invert).unwrap();
    assert_eq!(mask(&s, id), Some(vec![245, 55]));
    assert_eq!(s.undo.applied_steps(), depth + 1);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(mask(&s, id), Some(vec![10, 200]));
}

#[test]
fn from_selection_with_empty_selection_is_refused() {
    let (mut s, id) = session_with_image();
    let version = s.document.version;
    assert!(!enabled(&s, MaskAction::FromSelection));
    assert_eq!(run(&mut s, MaskAction::FromSelection).unwrap_err().code, "NOTHING_SELECTED");
    s.document.pixel_selection = Some(PixelSelection::empty(400, 300));
    assert!(!enabled(&s, MaskAction::FromSelection));
    assert!(run(&mut s, MaskAction::FromSelection).is_err());
    assert_eq!(s.document.version, version);
    assert_eq!(mask(&s, id), None);
    s.selection.ids.clear();
    assert_eq!(run(&mut s, MaskAction::Invert).unwrap_err().code, "NOTHING_SELECTED");
}

#[test]
fn release_is_one_undo_and_missing_mask_does_not_dirty() {
    let (mut s, id) = session_with_image();
    let version = s.document.version;
    assert!(run(&mut s, MaskAction::Release).is_err());
    assert_eq!(s.document.version, version);
    s.run_named("layer.mask.set", json!({"id":id.0,"mask":{"width":2,"height":1,"coverage":[0,255]}})).unwrap();
    let depth = s.undo.applied_steps();
    run(&mut s, MaskAction::Release).unwrap();
    assert_eq!(mask(&s, id), None);
    assert_eq!(s.undo.applied_steps(), depth + 1);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(mask(&s, id), Some(vec![0, 255]));
}

#[test]
fn locked_target_disables_actions_and_panel_renders() {
    let (mut s, id) = session_with_image();
    s.run_named("layer.mask.set", json!({"id":id.0,"mask":{"width":2,"height":1,"coverage":[0,255]}})).unwrap();
    s.document.objects.iter_mut().find(|o| o.id == id).unwrap().locked = true;
    assert!(!enabled(&s, MaskAction::Invert) && !enabled(&s, MaskAction::Release));
    let version = s.document.version;
    let ctx = egui::Context::default();
    let mut result = None;
    let mut output = ctx.run_ui(Default::default(), |ui| {
        result = show_mask_actions(ui, &mut s);
    });
    output.textures_delta.clear();
    assert!(result.is_none());
    assert_eq!(s.document.version, version);
}
