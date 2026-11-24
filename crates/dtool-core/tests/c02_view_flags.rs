use dtool_core::document::{Document, Units};
use dtool_core::view_state::ViewMode;
use dtool_core::{Command, Session, view_to_doc};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1))
}

#[test]
fn rotated_zoom_keeps_pivot_and_leaves_document_alone() {
    let mut s = session();
    s.run(&Command::with_params("view.rotate.set", json!({"degrees": 30.0}))).unwrap();
    let pivot = [123.0, 45.0];
    let before = view_to_doc(&s.view, pivot);
    let version = s.document.version;
    let undo = s.undo.can_undo();
    s.run(&Command::with_params("view.zoom.set", json!({"zoom": 3.5, "around": pivot}))).unwrap();
    let after = view_to_doc(&s.view, pivot);
    assert!((before[0] - after[0]).abs() < 1e-3 && (before[1] - after[1]).abs() < 1e-3);
    assert_eq!(s.document.version, version);
    assert_eq!(s.undo.can_undo(), undo);
}

#[test]
fn flags_set_and_query_are_view_only() {
    let mut s = session();
    let version = s.document.version;
    let undo = s.undo.can_undo();
    let r = s.run(&Command::with_params("view.flags.set", json!({"clip_to_canvas": true, "view_mode": "Outline"}))).unwrap();
    assert_eq!(r["flags"]["clip_to_canvas"], json!(true));
    assert!(s.view.canvas_flags.clip_to_canvas);
    assert_eq!(s.view.canvas_flags.view_mode, ViewMode::Outline);
    assert!(!s.view.canvas_flags.force_pixel_alignment);
    let q = s.run(&Command::with_params("view.flags.query", json!({}))).unwrap();
    assert_eq!(q["flags"]["view_mode"], json!("Outline"));
    assert!(s.run(&Command::with_params("view.flags.set", json!({"bogus": 1}))).is_err());
    assert_eq!(s.document.version, version);
    assert_eq!(s.undo.can_undo(), undo);
}

#[test]
fn zoom_to_fit_matches_fit() {
    let mut a = session();
    let mut b = session();
    let ra = a.run(&Command::with_params("view.zoom.fit", json!({"view_size": [400.0, 300.0]}))).unwrap();
    let rb = b.run(&Command::with_params("view.zoom.to_fit", json!({"view_size": [400.0, 300.0]}))).unwrap();
    assert_eq!(ra["zoom"], rb["zoom"]);
    assert_eq!(a.view.scroll, b.view.scroll);
}

#[test]
fn catalog_lists_view_flags_rows() {
    let catalog = dtool_protocol::CommandCatalog::default();
    for name in ["view.flags.set", "view.flags.query", "view.zoom.to_fit", "view.zoom.fit"] {
        let spec = catalog.get(name).unwrap();
        assert!(spec.implemented && !spec.undoable, "{name}");
    }
}

#[test]
fn xray_is_view_only() {
    let mut s = session();
    let version = s.document.version;
    let undo = s.undo.can_undo();
    assert_eq!(s.run(&Command::with_params("view.xray", json!({}))).unwrap()["xray"], json!(true));
    assert_eq!(s.run(&Command::with_params("view.xray", json!({"on": false}))).unwrap()["xray"], json!(false));
    assert!(s.run(&Command::with_params("view.xray", json!({"on": 1}))).is_err());
    assert_eq!(s.document.version, version);
    assert_eq!(s.undo.can_undo(), undo);
}