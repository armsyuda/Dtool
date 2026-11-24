use dtool_core::document::{Document, Units};
use dtool_core::{Session, ViewState};
use dtool_ui::navigator::NavigatorPanel;
use serde_json::json;

fn session() -> Session { Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1)) }

#[test]
fn test_view_box_drag_changes_scroll() {
    let mut s = session();
    let before = s.view.scroll;
    let version = s.document.version;
    let out = s.run_named("view.pan", json!({"delta": [10.0, 20.0]})).unwrap();
    assert_ne!(s.view.scroll, before);
    assert_eq!(out["scroll"], json!(s.view.scroll));
    assert_eq!(s.document.version, version);
    assert!(s.run_named("view.pan", json!({"delta": [1.0]})).is_err());
}

#[test]
fn test_zoom_slider_clamp() {
    let mut s = session();
    s.run_named("view.zoom.set", json!({"zoom": 200.0})).unwrap();
    assert_eq!(s.view.zoom, dtool_core::ZOOM_MAX);
    s.run_named("view.zoom.set", json!({"zoom": 0.001})).unwrap();
    assert_eq!(s.view.zoom, dtool_core::ZOOM_MIN);
    assert!(s.run_named("view.zoom.set", json!({"zoom": 0.0})).is_err());
}

#[test]
fn test_zoom_set_keeps_pivot() {
    let mut s = session();
    let pivot = [100.0f32, 80.0];
    let before = dtool_core::view_to_doc(&s.view, pivot);
    s.run_named("view.zoom.set", json!({"zoom": 4.0, "around": pivot})).unwrap();
    let after = dtool_core::view_to_doc(&s.view, pivot);
    assert!((before[0] - after[0]).abs() < 1e-3 && (before[1] - after[1]).abs() < 1e-3);
}

#[test]
fn test_view_rotate_set_and_reset_do_not_change_document() {
    let mut s = session();
    let version = s.document.version;
    let out = s.run_named("view.rotate.set", json!({"degrees": 45.0})).unwrap();
    assert_eq!(out["rotation_deg"], json!(45.0));
    let out = s.run_named("view.rotate.reset", json!({})).unwrap();
    assert_eq!(out["rotation_deg"], json!(0.0));
    assert_eq!(s.document.version, version);
}

#[test]
fn test_view_zoom_fit() {
    let mut s = session();
    let out = s.run_named("view.zoom.fit", json!({"view_size": [800.0, 600.0]})).unwrap();
    assert!(s.view.zoom > 0.0);
    assert_eq!(out["zoom"], json!(s.view.zoom));
    assert!(s.run_named("view.zoom.fit", json!({"view_size": [0.0, 600.0]})).is_err());
}

#[test]
fn test_navigator_panel_defaults() {
    let panel = NavigatorPanel::default();
    assert!(!panel.view_box_dragging);
    assert_eq!(panel.drag_start_scroll, [0.0, 0.0]);
}

#[test]
fn test_clamp_zoom_function() {
    assert_eq!(dtool_core::clamp_zoom(200.0), dtool_core::ZOOM_MAX);
    assert_eq!(dtool_core::clamp_zoom(0.001), dtool_core::ZOOM_MIN);
    assert_eq!(dtool_core::clamp_zoom(1.5), 1.5);
}

#[test]
fn test_fit_zoom_function() {
    assert!(dtool_core::fit_zoom([100.0, 100.0], [200.0, 200.0], 0.0) > 0.0);
}

#[test]
fn test_view_to_doc_and_doc_to_view() {
    let mut state = ViewState::default();
    state.zoom = 2.0;
    state.scroll = [10.0, 10.0];
    let view_point = [50.0, 50.0];
    let back = dtool_core::doc_to_view(&state, dtool_core::view_to_doc(&state, view_point));
    assert!((back[0] - view_point[0]).abs() < 1e-4);
    assert!((back[1] - view_point[1]).abs() < 1e-4);
}

#[test]
fn test_navigator_view_box_matches_canvas_after_pan_zoom_rotate() {
    let mut s = session();
    s.run_named("view.pan", json!({"delta": [24.0, -18.0]})).unwrap();
    s.run_named("view.zoom.set", json!({"zoom": 2.0, "around": [80.0, 60.0]})).unwrap();
    s.run_named("view.rotate.set", json!({"degrees": 30.0})).unwrap();
    let view_size = [400.0f32, 300.0];
    let origin = [5.0f32, 7.0];
    let scale = 0.25f32;
    let quad = dtool_ui::navigator::view_box_quad(&s.view, view_size, origin, scale);
    let expected = [
        dtool_core::view_to_doc(&s.view, [0.0, 0.0]),
        dtool_core::view_to_doc(&s.view, [view_size[0], 0.0]),
        dtool_core::view_to_doc(&s.view, [view_size[0], view_size[1]]),
        dtool_core::view_to_doc(&s.view, [0.0, view_size[1]]),
    ];
    for (got, doc) in quad.iter().zip(expected) {
        let mapped = [origin[0] + doc[0] * scale, origin[1] + doc[1] * scale];
        assert!((got[0] - mapped[0]).abs() < 1e-3 && (got[1] - mapped[1]).abs() < 1e-3);
    }
}

#[test]
fn test_zoom_set_keeps_pivot_when_rotated() {
    let mut s = session();
    let version = s.document.version;
    s.run_named("view.rotate.set", json!({"degrees": 30.0})).unwrap();
    let pivot = [100.0f32, 80.0];
    let before = dtool_core::view_to_doc(&s.view, pivot);
    s.run_named("view.zoom.set", json!({"zoom": 4.0, "around": pivot})).unwrap();
    let after = dtool_core::view_to_doc(&s.view, pivot);
    assert!((before[0] - after[0]).abs() < 1e-3 && (before[1] - after[1]).abs() < 1e-3);
    assert_eq!(s.document.version, version);
}

#[test]
fn test_thumbnail_follows_display_list_rules() {
    use dtool_core::document::{Document, Units};
    use dtool_core::object::Object;
    use dtool_core::CanvasFlags;
    let mut doc = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let hidden = doc.create_object(Object { pickable: false, size: [100.0, 100.0], ..Object::default() });
    let _child = doc.create_object(Object { pos: [10.0, 10.0], size: [20.0, 20.0], parent_id: Some(hidden), ..Object::default() });
    let _inside = doc.create_object(Object { pos: [20.0, 20.0], size: [30.0, 30.0], ..Object::default() });
    let _outside = doc.create_object(Object { pos: [200.0, 200.0], size: [10.0, 10.0], ..Object::default() });
    let _partial = doc.create_object(Object { pos: [80.0, 80.0], size: [40.0, 40.0], ..Object::default() });
    let flags = CanvasFlags { clip_to_canvas: true, ..CanvasFlags::default() };
    let bounds = dtool_ui::navigator::thumbnail_item_bounds(&doc, &flags);
    assert_eq!(bounds, vec![[20.0, 20.0, 30.0, 30.0], [80.0, 80.0, 20.0, 20.0]]);
    let mut snap_doc = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let _snapped = snap_doc.create_object(Object { pos: [10.4, 11.6], size: [8.4, 9.4], ..Object::default() });
    let snap_flags = CanvasFlags { force_pixel_alignment: true, ..CanvasFlags::default() };
    assert_eq!(dtool_ui::navigator::thumbnail_item_bounds(&snap_doc, &snap_flags), vec![[10.0, 12.0, 8.0, 9.0]]);
}
