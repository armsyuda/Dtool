#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::document::Units;
use dtool_core::workspace::Workspace;
use dtool_ui::dialogs::{apply_preset, create_new_document, show_new_document_dialog, submit_new_document, NewDocumentDialogState};
use dtool_ui::shell::AppShell;
use dtool_ui::shortcuts::{Focus, Modifiers};

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::default() }
}

fn frame(state: &mut NewDocumentDialogState, workspace: &mut Workspace, events: Vec<egui::Event>) -> bool {
    let ctx = egui::Context::default();
    let mut closed = false;
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        closed = show_new_document_dialog(ui, state, workspace);
    });
    output.textures_delta.clear();
    closed
}

#[test]
fn ctrl_n_opens_dialog_without_creating() {
    let _home = test_home::enter("ctrl_n_opens_dialog_without_creating");
    let mut shell = AppShell::new();
    let before = shell.workspace.contexts().len();
    shell.new_document_dialog = None;
    let target = shell.handle_shortcut_key(Focus::Canvas, "N", Modifiers { ctrl: true, shift: false, alt: false });
    assert!(target.is_some());
    assert!(shell.new_document_dialog.is_some());
    assert_eq!(shell.workspace.contexts().len(), before);
}

#[test]
fn dialog_state_creates_document_through_doc_new() {
    let _home = test_home::enter("dialog_state_creates_document_through_doc_new");
    let mut workspace = Workspace::default();
    let mut state = NewDocumentDialogState::default();
    apply_preset(&mut state, "A4");
    state.pages = 3;
    let command = state.command();
    assert_eq!(command.name, "doc.new");
    assert_eq!(command.params["preset"], "A4");
    let id = create_new_document(&mut workspace, &state).unwrap();
    assert_eq!(workspace.active(), Some(id));
    let context = workspace.get(id).unwrap();
    assert_eq!(context.title, "A4");
    assert_eq!(context.session.document.units, Units::Mm);
    assert_eq!(context.session.document.pages.len(), 3);
    assert!(!context.is_modified());
}

#[test]
fn invalid_size_keeps_dialog_open_with_error() {
    let _home = test_home::enter("invalid_size_keeps_dialog_open_with_error");
    let mut workspace = Workspace::default();
    let mut state = NewDocumentDialogState::default();
    state.width = 0.0;
    state.preset = None;
    assert!(!submit_new_document(&mut workspace, &mut state));
    assert!(state.error_message.is_some());
    assert!(workspace.contexts().is_empty());
    assert!(!frame(&mut state, &mut workspace, Vec::new()));
    assert!(workspace.contexts().is_empty());
}

#[test]
fn rendered_dialog_escape_cancels_and_enter_creates() {
    let _home = test_home::enter("rendered_dialog_escape_cancels_and_enter_creates");
    let mut workspace = Workspace::default();
    let mut state = NewDocumentDialogState::default();
    assert!(!frame(&mut state, &mut workspace, Vec::new()));
    assert!(frame(&mut state, &mut workspace, vec![key(egui::Key::Escape)]));
    assert!(workspace.contexts().is_empty());
    state.preset = None;
    assert!(frame(&mut state, &mut workspace, vec![key(egui::Key::Enter)]));
    assert_eq!(workspace.contexts().len(), 1);
    assert_eq!(workspace.contexts()[0].title, dtool_strings::text("doc.untitled"));
}
