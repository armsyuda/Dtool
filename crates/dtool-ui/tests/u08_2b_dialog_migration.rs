use dtool_core::workspace::{CloseAnswer, Workspace};
use dtool_ui::dialog_chrome::{backdrop_color, begin_modal_frame, modal_open};
use dtool_ui::dialogs::{show_close_document_dialog, show_file_dialog, show_mcp_confirm_dialog, CloseDocumentDialogState, FileDialogMode, FileDialogState, McpConfirmDialogState, McpGateClass};

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::default() }
}

fn run(ctx: &egui::Context, events: Vec<egui::Event>, add: impl FnMut(&mut egui::Ui)) {
    let mut add = add;
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| add(ui));
    output.textures_delta.clear();
}

#[test]
fn backdrop_token_is_translucent() {
    let colour = backdrop_color();
    assert!(colour.a() > 0 && colour.a() < 255);
    assert_ne!(colour, egui::Color32::from_rgb(255, 0, 255));
}

#[test]
fn close_dialog_escape_cancels_and_keeps_document() {
    let ctx = egui::Context::default();
    let mut workspace = Workspace::default();
    let id = workspace.new_document("a");
    let mut state = CloseDocumentDialogState::new(id, "a".to_string());
    let mut answer = None;
    run(&ctx, vec![key(egui::Key::Escape)], |ui| {
        answer = show_close_document_dialog(ui, &mut state, &|_, _| Ok(true));
    });
    assert_eq!(answer, Some(CloseAnswer::Cancel));
    assert!(workspace.get(id).is_some());
}

#[test]
fn modal_dialog_sets_blocking_flag_each_frame() {
    let ctx = egui::Context::default();
    let mut workspace = Workspace::default();
    let mut state = FileDialogState::new(FileDialogMode::Open);
    let mut flagged = false;
    run(&ctx, vec![], |ui| {
        assert!(!begin_modal_frame(ui.ctx()));
        assert!(!show_file_dialog(ui, &mut state, &mut workspace));
        flagged = modal_open(ui.ctx());
    });
    assert!(flagged);
    let mut carried = false;
    run(&ctx, vec![], |ui| {
        carried = begin_modal_frame(ui.ctx());
        assert!(!modal_open(ui.ctx()));
    });
    assert!(carried);
}

#[test]
fn file_dialog_escape_closes() {
    let ctx = egui::Context::default();
    let mut workspace = Workspace::default();
    let mut state = FileDialogState::new(FileDialogMode::Open);
    let mut closed = false;
    run(&ctx, vec![key(egui::Key::Escape)], |ui| {
        closed = show_file_dialog(ui, &mut state, &mut workspace);
    });
    assert!(closed);
}

#[test]
fn mcp_confirm_escape_declines() {
    let ctx = egui::Context::default();
    let mut state = McpConfirmDialogState::new(McpGateClass::C2Overwrite);
    let mut answer = None;
    run(&ctx, vec![key(egui::Key::Escape)], |ui| {
        answer = show_mcp_confirm_dialog(ui, &mut state);
    });
    assert_eq!(answer, Some(false));
    assert_eq!(state.confirmed, Some(false));
}
