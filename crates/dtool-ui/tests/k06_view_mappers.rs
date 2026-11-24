use dtool_core::ViewState;
use dtool_ui::canvas::{CanvasView, CanvasViewCommand};
use dtool_ui::navigator::view_box_quad;
use dtool_ui::rulers::{doc_to_screen, screen_to_doc};
use egui::{pos2, Rect};

fn rotated(rotation_deg: f32) -> ViewState {
    ViewState { zoom: 2.0, scroll: [10.0, -5.0], rotation_deg, ..ViewState::default() }
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

const ANGLES: [f32; 5] = [0.0, 30.0, 90.0, 180.0, 287.5];
const POINTS: [[f32; 2]; 3] = [[0.0, 0.0], [37.0, 12.5], [-20.0, 140.0]];

#[test]
fn ruler_mappers_match_core_with_canvas_offset() {
    let canvas_rect = Rect::from_min_max(pos2(40.0, 25.0), pos2(840.0, 625.0));
    for angle in ANGLES {
        let view = rotated(angle);
        for point in POINTS {
            let screen = pos2(canvas_rect.min.x + point[0], canvas_rect.min.y + point[1]);
            assert!(close(screen_to_doc(&view, canvas_rect, screen), dtool_core::view_to_doc(&view, point)));
            let back = doc_to_screen(&view, canvas_rect, dtool_core::view_to_doc(&view, point));
            assert!(close([back.x, back.y], [screen.x, screen.y]));
        }
    }
}

#[test]
fn ruler_mapper_keeps_zero_zoom_finite() {
    let view = ViewState { zoom: 0.0, ..rotated(45.0) };
    let canvas_rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
    let doc = screen_to_doc(&view, canvas_rect, pos2(10.0, 10.0));
    assert!(doc[0].is_finite() && doc[1].is_finite());
}

#[test]
fn navigator_view_box_is_core_view_corners() {
    let size = [800.0, 600.0];
    for angle in ANGLES {
        let view = rotated(angle);
        let quad = view_box_quad(&view, size, [0.0, 0.0], 1.0);
        for corner in [[0.0, 0.0], [size[0], 0.0], [0.0, size[1]], size] {
            let doc = dtool_core::view_to_doc(&view, corner);
            assert!(quad.iter().any(|q| close(*q, doc)));
        }
    }
}

#[test]
fn canvas_view_commands_follow_core_convention() {
    let mut canvas_view = CanvasView::new();
    let mut ours = rotated(90.0);
    let mut core = ours.clone();
    canvas_view.apply(&mut ours, CanvasViewCommand::Pan { delta: [7.0, -3.0] }, [800.0, 600.0]);
    dtool_core::command::view_pan_apply(&mut core, [7.0, -3.0]);
    assert_eq!(ours.scroll, core.scroll);

    let pivot = [120.0, 80.0];
    let before = dtool_core::view_to_doc(&ours, pivot);
    canvas_view.apply(&mut ours, CanvasViewCommand::ZoomSet { zoom: 5.0, around: Some(pivot) }, [800.0, 600.0]);
    assert_eq!(ours.zoom, 5.0);
    assert!(close(dtool_core::view_to_doc(&ours, pivot), before));

    canvas_view.apply(&mut ours, CanvasViewCommand::RotateSet { degrees: -30.0 }, [800.0, 600.0]);
    assert_eq!(ours.rotation_deg, 330.0);
}