use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session};
use serde_json::json;

#[test]
fn zoom_in_and_out_step_the_view_and_reject_bad_factor() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    let start = s.view.zoom;
    let r = s.run(&Command::with_params("view.zoom.in", json!({}))).unwrap();
    assert_eq!(r["zoom"].as_f64().unwrap() as f32, s.view.zoom);
    assert!((s.view.zoom - start * 2.0).abs() < 1e-4);
    s.run(&Command::with_params("view.zoom.out", json!({"factor": 4.0}))).unwrap();
    assert!((s.view.zoom - start * 0.5).abs() < 1e-4);
    assert!(s.run(&Command::with_params("view.zoom.in", json!({"factor": 0.0}))).is_err());
    assert!((s.view.zoom - start * 0.5).abs() < 1e-4);
}