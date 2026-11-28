use dtool_ui::dialog_chrome::{decide, ChromeInput, ClosePolicy, DialogChrome, DialogMode, DialogResult};

fn input(escape: bool, enter: bool, click_outside: bool) -> ChromeInput {
    ChromeInput { escape, enter, click_outside }
}

#[test]
fn escape_cancels_unless_job_running() {
    assert_eq!(decide(input(true, false, false), DialogMode::Modal, ClosePolicy::Free, true), DialogResult::Cancel);
    assert_eq!(decide(input(true, false, false), DialogMode::Modal, ClosePolicy::CancelOnly, true), DialogResult::Cancel);
    assert_eq!(decide(input(true, true, true), DialogMode::Modal, ClosePolicy::JobRunning, true), DialogResult::Open);
}

#[test]
fn click_outside_closes_free_modals_only() {
    assert_eq!(decide(input(false, false, true), DialogMode::Modal, ClosePolicy::Free, false), DialogResult::Cancel);
    assert_eq!(decide(input(false, false, true), DialogMode::Modal, ClosePolicy::CancelOnly, false), DialogResult::Open);
    assert_eq!(decide(input(false, false, true), DialogMode::Nonmodal, ClosePolicy::Free, false), DialogResult::Open);
}

#[test]
fn enter_confirms_when_allowed() {
    assert_eq!(decide(input(false, true, false), DialogMode::Modal, ClosePolicy::CancelOnly, true), DialogResult::Confirm);
    assert_eq!(decide(input(false, true, false), DialogMode::Modal, ClosePolicy::CancelOnly, false), DialogResult::Open);
    assert_eq!(decide(ChromeInput::default(), DialogMode::Modal, ClosePolicy::Free, true), DialogResult::Open);
}

#[test]
fn chrome_runs_headless() {
    let ctx = egui::Context::default();
    let mut chrome = DialogChrome::default();
    let mut seen = false;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let (result, value) = chrome.show(ui.ctx(), "test.chrome", "title", DialogMode::Modal, ClosePolicy::Free, false, |_| 7);
        assert_eq!(result, DialogResult::Open);
        assert_eq!(value, 7);
        seen = true;
    });
    output.textures_delta.clear();
    assert!(seen);
    assert_eq!(chrome.frames(), 1);
    chrome.reset();
    assert_eq!(chrome.frames(), 0);
}

#[test]
fn title_drag_moves_dialog_when_release_arrives_in_same_frame() {
    fn title_position(output: &egui::FullOutput) -> egui::Pos2 {
        fn find(shape: &egui::Shape) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.text() == "Drag title" => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(find),
                _ => None,
            }
        }
        output.shapes.iter().find_map(|shape| find(&shape.shape)).expect("dialog title")
    }

    let ctx = egui::Context::default();
    let mut chrome = DialogChrome::default();
    let mut show = |events, mode| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(630.0, 440.0))),
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            let (result, ()) = chrome.show(ui.ctx(), "test.movable", "Drag title", mode, ClosePolicy::CancelOnly, false, |ui| {
                ui.set_width(180.0);
                ui.label("Options");
            });
            assert_eq!(result, DialogResult::Open);
        })
    };
    let mode = DialogMode::Modal;
    show(Vec::new(), mode);
    let start = title_position(&show(Vec::new(), mode));
    let delta = egui::vec2(90.0, -45.0);
    let end = start + delta;
    let pointer = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    show(vec![pointer(start, true), egui::Event::PointerMoved(end), pointer(end, false)], mode);
    let moved = title_position(&show(Vec::new(), mode));
    assert!((moved - start - delta).length() <= 1.0, "{start:?} -> {moved:?}");
}
