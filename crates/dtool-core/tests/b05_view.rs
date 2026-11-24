use dtool_core::command::{Session, ViewZoomSetArgs, view_pan_apply};
use dtool_core::document::{Document, Units};
use dtool_core::view::{clamp_zoom, doc_to_view, fit_zoom, view_to_doc};
use dtool_core::view_state::{ViewState, ZOOM_MAX, ZOOM_MIN};

fn session() -> Session {
    Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1))
}

#[test]
fn test_clamp_zoom() {
    assert_eq!(clamp_zoom(0.5), 0.5);
    assert_eq!(clamp_zoom(ZOOM_MIN - 1.0), ZOOM_MIN);
    assert_eq!(clamp_zoom(ZOOM_MAX + 1.0), ZOOM_MAX);
}

#[test]
fn test_reset_rotation_leaves_document() {
    let mut session = session();
    session.view_rotate_set(45.0).unwrap();
    let before = serde_json::to_value(&session.document.objects).unwrap();
    let generation = session.generation;
    let result = session.view_rotate_reset();
    assert!(result.is_object());
    assert_eq!(session.view.rotation_deg, 0.0);
    assert_eq!(serde_json::to_value(&session.document.objects).unwrap(), before);
    assert_eq!(session.generation, generation);
}

#[test]
fn test_fit_uses_rotated_bounds() {
    let page_size = [100.0, 200.0];
    let view_size = [500.0, 500.0];
    let zoom_no_rot = fit_zoom(page_size, view_size, 0.0);
    let zoom_rot_45 = fit_zoom(page_size, view_size, 45.0);
    assert!(zoom_rot_45 < zoom_no_rot);
}

#[test]
fn test_inverse_hit_test_round_trip() {
    let mut state = ViewState::default();
    state.zoom = 2.0;
    state.scroll = [10.0, 20.0];
    state.rotation_deg = 30.0;
    let doc_point: [f32; 2] = [50.0, 60.0];
    let view_point = doc_to_view(&state, doc_point);
    let recovered_doc_point = view_to_doc(&state, view_point);
    assert!((doc_point[0] - recovered_doc_point[0]).abs() < 1e-3);
    assert!((doc_point[1] - recovered_doc_point[1]).abs() < 1e-3);
}

#[test]
fn test_view_pan_command() {
    let mut state = ViewState::default();
    view_pan_apply(&mut state, [5.0, -5.0]);
    assert_eq!(state.scroll[0], 5.0);
    assert_eq!(state.scroll[1], -5.0);
    let mut session = session();
    let result = session.view_pan([5.0, -5.0]);
    assert_eq!(result["changes"][0]["name"], "view.pan");
    assert_eq!(session.view.scroll, [5.0, -5.0]);
}

#[test]
fn test_view_zoom_set_clamped() {
    let mut session = session();
    let args: ViewZoomSetArgs = serde_json::from_value(serde_json::json!({"zoom": 200.0})).unwrap();
    let result = session.view_zoom_set(args);
    assert!(result.is_object());
    assert_eq!(session.view.zoom, ZOOM_MAX.min(200.0));
}

#[test]
fn test_view_rotate_set() {
    let mut session = session();
    let result = session.view_rotate_set(90.0);
    assert!(result.is_ok());
    assert_eq!(session.view.rotation_deg, 90.0);
}
