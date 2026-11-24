use dtool_core::view::{canvas_to_doc, pan_by_doc, view_to_doc, visible_doc_quad, zoom_about};
use dtool_core::ViewState;
use dtool_ui::canvas::{CanvasView, CanvasViewCommand};
use dtool_ui::navigator::{view_box_drag_pan, view_box_quad};
use dtool_ui::rulers::screen_to_doc;

fn close(a: [f32; 2], b: [f32; 2], tol: f32) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

#[test]
fn all_view_helpers_agree() {
    let canvas = egui::Rect::from_min_size(egui::pos2(30.0, 40.0), egui::vec2(800.0, 600.0));
    for zoom in [0.5f32, 2.0] {
        for scroll in [[0.0f32, 0.0], [10.0, -20.0]] {
            for rotation_deg in [0.0f32, 37.0, 90.0] {
                let view = ViewState { zoom, scroll, rotation_deg, ..Default::default() };
                for p in [[0.0f32, 0.0], [100.0, 50.0], [-30.0, 200.0], [400.0, 300.0], [7.5, -9.25]] {
                    let core = view_to_doc(&view, p);
                    let screen = egui::pos2(p[0] + canvas.min.x, p[1] + canvas.min.y);
                    assert!(close(screen_to_doc(&view, canvas, screen), core, 1e-3));
                    assert!(close(dtool_tools::view_to_doc(&view, p), core, 1e-4));
                    assert!(close(dtool_ui::canvas::view_to_doc(&view, p), core, 1e-4));
                    assert!(close(canvas_to_doc(&view, [canvas.min.x, canvas.min.y], [screen.x, screen.y]), core, 1e-3));
                }
            }
        }
    }
}

#[test]
fn pan_sign_matches_core() {
    let mut a = ViewState::default();
    let mut b = ViewState::default();
    CanvasView::new().apply(&mut a, CanvasViewCommand::Pan { delta: [5.0, 0.0] }, [100.0, 100.0]);
    dtool_core::command::view_pan_apply(&mut b, [5.0, 0.0]);
    assert_eq!(a.scroll, b.scroll);
    let mut c = ViewState::default();
    dtool_tools::apply_view_command(&mut c, dtool_tools::ViewCommand::Pan { delta: [5.0, 0.0] }, [100.0, 100.0]);
    assert_eq!(c.scroll, b.scroll);
}

#[test]
fn zoom_about_keeps_pivot() {
    let mut view = ViewState { zoom: 1.5, scroll: [12.0, -4.0], rotation_deg: 30.0, ..Default::default() };
    let pivot = [200.0, 120.0];
    let before = view_to_doc(&view, pivot);
    zoom_about(&mut view, 3.0, pivot);
    assert!(close(view_to_doc(&view, pivot), before, 1e-3));
}

#[test]
fn navigator_box_is_visible_quad() {
    let view = ViewState { zoom: 2.0, scroll: [10.0, 0.0], rotation_deg: 90.0, ..Default::default() };
    let quad = view_box_quad(&view, [400.0, 300.0], [0.0, 0.0], 1.0);
    assert_eq!(quad, visible_doc_quad(&view, [400.0, 300.0]));
    let pan = view_box_drag_pan(&view, [4.0, 0.0], 2.0);
    let mut moved = view.clone();
    pan_by_doc(&mut moved, [2.0, 0.0]);
    assert!(close([view.scroll[0] + pan[0], view.scroll[1] + pan[1]], moved.scroll, 1e-4));
    let after = visible_doc_quad(&moved, [400.0, 300.0]);
    let start = visible_doc_quad(&view, [400.0, 300.0]);
    assert!(close([after[0][0] - start[0][0], after[0][1] - start[0][1]], [2.0, 0.0], 1e-3));
}

#[test]
fn zero_zoom_does_not_produce_nan() {
    let view = ViewState { zoom: 0.0, ..Default::default() };
    let p = view_to_doc(&view, [10.0, 10.0]);
    assert!(p[0].is_finite() && p[1].is_finite());
    assert_eq!(dtool_core::view::clamp_zoom(f32::NAN), 1.0);
}
