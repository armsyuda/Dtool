use dtool_core::view_state::ViewState;
use dtool_core::{doc_to_view, view_to_doc};
use dtool_ui::app_canvas::wheel_view;

fn rotated() -> ViewState {
    let mut view = ViewState::default();
    view.zoom = 1.5;
    view.scroll = [40.0, -20.0];
    view.rotation_deg = 30.0;
    view
}

#[test]
fn wheel_zooms_about_pointer() {
    let mut view = rotated();
    let pointer = [210.0, 130.0];
    let anchor = view_to_doc(&view, pointer);
    assert!(wheel_view(&mut view, [0.0, 120.0], 1.0, Some(pointer), false));
    assert!(view.zoom > 1.5);
    let mapped = doc_to_view(&view, anchor);
    assert!((mapped[0] - pointer[0]).abs() < 1e-2 && (mapped[1] - pointer[1]).abs() < 1e-2);
    let zoomed = view.zoom;
    assert!(wheel_view(&mut view, [0.0, -120.0], 1.0, Some(pointer), false));
    assert!(view.zoom < zoomed);
}

#[test]
fn pinch_zooms_about_pointer() {
    let mut view = rotated();
    let pointer = [50.0, 70.0];
    let anchor = view_to_doc(&view, pointer);
    assert!(wheel_view(&mut view, [0.0, 0.0], 2.0, Some(pointer), false));
    let mapped = doc_to_view(&view, anchor);
    assert!((mapped[0] - pointer[0]).abs() < 1e-2 && (mapped[1] - pointer[1]).abs() < 1e-2);
}

#[test]
fn space_or_horizontal_scroll_pans() {
    let mut view = rotated();
    assert!(wheel_view(&mut view, [5.0, 7.0], 1.0, Some([1.0, 1.0]), true));
    assert_eq!(view.scroll, [45.0, -13.0]);
    assert_eq!(view.zoom, 1.5);
    assert!(wheel_view(&mut view, [9.0, 0.0], 1.0, Some([1.0, 1.0]), false));
    assert_eq!(view.scroll, [54.0, -13.0]);
    assert!(!wheel_view(&mut view, [0.0, 0.0], 1.0, Some([1.0, 1.0]), false));
}
