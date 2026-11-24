use dtool_core::workspace::{CloseAnswer, CloseCheck, ContextId, Workspace};
use dtool_ui::dialogs::{resolve_close_answer, show_pending_close, sync_close_dialog, CloseDocumentDialogState, CloseResolution};
use serde_json::json;

fn dirty(workspace: &mut Workspace, title: &str) -> ContextId {
    let id = workspace.new_document(title);
    workspace.session_mut().unwrap().run_named("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    id
}

fn frame(pending: &mut Option<ContextId>, state: &mut Option<CloseDocumentDialogState>, workspace: &mut Workspace, events: Vec<egui::Event>) -> bool {
    let ctx = egui::Context::default();
    let mut save_as = false;
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        save_as = show_pending_close(ui, pending, state, workspace).is_some();
    });
    output.textures_delta.clear();
    save_as
}

fn escape() -> egui::Event {
    egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::default() }
}

#[test]
fn dirty_close_opens_prompt_and_escape_cancels() {
    let mut workspace = Workspace::default();
    let id = dirty(&mut workspace, "draft");
    assert_eq!(workspace.request_close(id).unwrap(), CloseCheck::NeedsAnswer);
    let mut pending = Some(id);
    let mut state = None;
    assert!(!frame(&mut pending, &mut state, &mut workspace, Vec::new()));
    assert_eq!(pending, Some(id));
    let shown = state.as_ref().unwrap();
    assert_eq!(shown.context_id, id);
    assert_eq!(shown.document_name, "draft");
    assert!(!frame(&mut pending, &mut state, &mut workspace, vec![escape()]));
    assert_eq!(pending, None);
    assert!(state.is_none());
    assert!(workspace.get(id).is_some());
}

#[test]
fn answers_close_keep_or_ask_for_save_as() {
    let mut workspace = Workspace::default();
    let other = workspace.new_document("other");
    let id = dirty(&mut workspace, "draft");
    workspace.activate(other).unwrap();
    assert_eq!(resolve_close_answer(&mut workspace, id, CloseAnswer::Cancel).unwrap(), CloseResolution::Kept);
    assert_eq!(resolve_close_answer(&mut workspace, id, CloseAnswer::Yes).unwrap(), CloseResolution::NeedsSaveAs);
    assert_eq!(workspace.active(), Some(other));
    assert!(workspace.get(id).is_some());
    assert_eq!(resolve_close_answer(&mut workspace, id, CloseAnswer::No).unwrap(), CloseResolution::Closed);
    assert!(workspace.get(id).is_none());
}

#[test]
fn yes_saves_documents_with_a_path() {
    let dir = std::env::temp_dir().join(format!("dtool_f3j_close_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("saved.dtool");
    let mut workspace = Workspace::default();
    let id = dirty(&mut workspace, "saved");
    workspace.save_as(id, &path).unwrap();
    workspace.session_mut().unwrap().run_named("object.create", json!({"name": "b", "pos": [5.0, 5.0], "size": [10.0, 10.0]})).unwrap();
    assert_eq!(workspace.request_close(id).unwrap(), CloseCheck::NeedsAnswer);
    assert_eq!(resolve_close_answer(&mut workspace, id, CloseAnswer::Yes).unwrap(), CloseResolution::Closed);
    assert!(workspace.get(id).is_none());
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("\"b\""));
}

#[test]
fn sync_drops_stale_pending_close() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("");
    let mut pending = Some(id);
    let mut state = None;
    sync_close_dialog(&mut pending, &mut state, &workspace);
    assert_eq!(state.as_ref().unwrap().document_name, dtool_strings::text("doc.untitled"));
    assert_eq!(resolve_close_answer(&mut workspace, id, CloseAnswer::No).unwrap(), CloseResolution::Closed);
    sync_close_dialog(&mut pending, &mut state, &workspace);
    assert_eq!(pending, None);
    assert!(state.is_none());
}
