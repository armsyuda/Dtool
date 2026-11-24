#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::view_state::{CanvasFlags, ViewMode};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, ViewState};
use serde_json::json;

#[test]
fn c10_fix6_highlight_flags_default_off_and_set_independently() {
    let mut view = ViewState::default();
    assert_eq!(view.highlight(), (false, false));
    view.set_highlight(Some(true), None);
    assert_eq!(view.highlight(), (true, false));
    view.set_highlight(None, Some(true));
    assert_eq!(view.highlight(), (true, true));
    view.set_highlight(Some(false), None);
    assert_eq!(view.highlight(), (false, true));
    assert_eq!(ViewState::default().highlight(), (false, false));
}

#[test]
fn c10_fix6_highlight_flags_round_trip_and_old_json_defaults() {
    let mut view = ViewState::default();
    view.set_highlight(Some(true), Some(true));
    let back: ViewState = serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
    assert_eq!(back.highlight(), (true, true));
    let old: CanvasFlags = serde_json::from_value(json!({ "force_pixel_alignment": false, "clip_to_canvas": false, "preview_mode": false, "view_mode": ViewMode::default(), "show_bleed": true })).unwrap();
    assert!(!old.highlight_missing_fonts && !old.highlight_missing_chars && old.show_bleed);
}

#[test]
fn c10_fix6_view_flags_set_drives_highlight_per_document() {
    let _home = test_home::enter("c10_fix6_highlight_view");
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let b = ws.new_document("B");
    let q = ws.run(Scope::of(a), Command::new("view.flags.query")).1.unwrap();
    assert_eq!((q["flags"]["highlight_missing_fonts"].clone(), q["flags"]["highlight_missing_chars"].clone()), (json!(false), json!(false)));
    let set = ws.run(Scope::of(a), Command::with_params("view.flags.set", json!({ "highlight_missing_chars": true }))).1.unwrap();
    assert_eq!(set["flags"]["highlight_missing_chars"], true);
    assert_eq!(set["flags"]["highlight_missing_fonts"], false);
    assert_eq!(ws.get(a).unwrap().session.view.highlight(), (false, true));
    assert_eq!(ws.get(b).unwrap().session.view.highlight(), (false, false));
    ws.run(Scope::of(a), Command::with_params("view.flags.set", json!({ "highlight_missing_fonts": true, "show_bleed": true }))).1.unwrap();
    assert_eq!(ws.get(a).unwrap().session.view.highlight(), (true, true));
    assert!(ws.get(a).unwrap().session.view.canvas_flags.show_bleed);
    assert!(ws.run(Scope::of(a), Command::with_params("view.flags.set", json!({ "highlight_missing_fonts": "on" }))).1.is_err());
    assert_eq!(ws.get(a).unwrap().session.view.highlight(), (true, true));
}