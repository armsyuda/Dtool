#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use serde_json::json;

#[test]
fn bleed_visibility_is_per_document_view_state() {
    let _home = test_home::enter("c02_bleed_flag");
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let b = ws.new_document("B");
    let q = ws.run(Scope::of(a), Command::new("view.flags.query")).1.unwrap();
    assert_eq!(q["flags"]["show_bleed"], false);
    let set = ws.run(Scope::of(a), Command::with_params("view.flags.set", json!({ "show_bleed": true }))).1.unwrap();
    assert_eq!(set["flags"]["show_bleed"], true);
    assert!(ws.get(a).unwrap().session.view.canvas_flags.show_bleed);
    assert!(!ws.get(b).unwrap().session.view.canvas_flags.show_bleed);
    assert!(ws.run(Scope::of(a), Command::with_params("view.flags.set", json!({ "show_bleed": "yes" }))).1.is_err());
    assert!(ws.get(a).unwrap().session.view.canvas_flags.show_bleed);
    let old: dtool_core::view_state::CanvasFlags = serde_json::from_value(json!({ "force_pixel_alignment": false, "clip_to_canvas": false, "preview_mode": false, "view_mode": dtool_core::view_state::ViewMode::default() })).unwrap();
    assert!(!old.show_bleed);
}