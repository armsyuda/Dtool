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
fn ctrl_wheel_zooms_about_pointer() {
    let mut view = rotated();
    let pointer = [210.0, 130.0];
    let anchor = view_to_doc(&view, pointer);
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    for delta in [120.0_f32, -120.0] {
        let input = egui::RawInput {
            events: vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                modifiers,
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
            }],
            ..Default::default()
        };
        let mut wheel = None;
        egui::Context::default()
            .run_ui(input, |ui| {
                wheel.get_or_insert_with(|| {
                    ui.input(|i| (i.smooth_scroll_delta(), i.zoom_delta()))
                });
            })
            .drop_without_applying_deltas();
        let (scroll, zoom) = wheel.unwrap();
        assert_eq!(scroll, egui::Vec2::ZERO);
        let previous_zoom = view.zoom;
        assert!(wheel_view(&mut view, [scroll.x, scroll.y], zoom, Some(pointer)));
        assert!((view.zoom - previous_zoom) * delta > 0.0);
        let mapped = doc_to_view(&view, anchor);
        assert!((mapped[0] - pointer[0]).abs() < 1e-2 && (mapped[1] - pointer[1]).abs() < 1e-2);
    }
}

#[test]
fn pinch_zooms_about_pointer() {
    let mut view = rotated();
    let pointer = [50.0, 70.0];
    let anchor = view_to_doc(&view, pointer);
    assert!(wheel_view(&mut view, [0.0, 0.0], 2.0, Some(pointer)));
    let mapped = doc_to_view(&view, anchor);
    assert!((mapped[0] - pointer[0]).abs() < 1e-2 && (mapped[1] - pointer[1]).abs() < 1e-2);
}

#[test]
fn plain_vertical_and_horizontal_scroll_pan_without_zoom() {
    let mut view = rotated();
    assert!(wheel_view(&mut view, [0.0, 7.0], 1.0, Some([1.0, 1.0])));
    assert_eq!(view.scroll, [40.0, -13.0]);
    assert_eq!(view.zoom, 1.5);
    assert!(wheel_view(&mut view, [9.0, 0.0], 1.0, Some([1.0, 1.0])));
    assert_eq!(view.scroll, [49.0, -13.0]);
    assert!(!wheel_view(&mut view, [0.0, 0.0], 1.0, Some([1.0, 1.0])));
}
