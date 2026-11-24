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
