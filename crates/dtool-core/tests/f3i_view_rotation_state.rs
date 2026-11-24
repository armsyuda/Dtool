use dtool_core::document::{Document, Units};
use dtool_core::Session;
use serde_json::json;

fn session() -> Session { Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1)) }

#[test]
fn rotate_set_updates_view_state_used_for_mapping() {
    let mut s = session();
    let before = dtool_core::view::view_to_doc(&s.view, [100.0, 50.0]);
    let out = s.run_named("view.rotate.set", json!({"degrees": 450.0})).unwrap();
    assert_eq!(s.view.rotation_deg, 90.0);
    assert_eq!(out["rotation_deg"], json!(90.0));
    assert_ne!(dtool_core::view::view_to_doc(&s.view, [100.0, 50.0]), before);
    s.run_named("view.rotate.reset", json!({})).unwrap();
    assert_eq!(s.view.rotation_deg, 0.0);
}

#[test]
fn zoom_fit_keeps_rotation() {
    let mut s = session();
    s.run_named("view.rotate.set", json!({"degrees": 90.0})).unwrap();
    s.run_named("view.zoom.fit", json!({"view_size": [400.0, 400.0]})).unwrap();
    assert_eq!(s.view.rotation_deg, 90.0);
}
