use dtool_core::workspace::{CloseAnswer, ContextId};
use dtool_core::Workspace;
use dtool_ui::dialogs::export::{ExportDialog, UI_EXPORT_CLOSE, UI_EXPORT_OPEN, UI_EXPORT_QUERY, UI_EXPORT_SET};
use serde_json::json;

fn two_documents() -> (Workspace, ContextId, ContextId) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    workspace.activate(a).unwrap();
    (workspace, a, b)
}

fn is_document(workspace: &Workspace, id: ContextId, doc: &dtool_core::Document) -> bool {
    std::ptr::eq(doc, &workspace.get(id).unwrap().session.document)
}

#[test]
fn open_for_keeps_opening_document_after_switch() {
    let (mut workspace, a, b) = two_documents();
    let mut dialog = ExportDialog::new();
    dialog.open_for(a, &workspace.get(a).unwrap().session.document, None);
    workspace.activate(b).unwrap();
    assert_eq!(dialog.owner(), Some(a));
    let doc = dialog.owner_document(&workspace).unwrap();
    assert!(is_document(&workspace, a, doc));
    assert!(!is_document(&workspace, b, doc));
}

#[test]
fn command_opened_dialog_binds_active_once() {
    let (mut workspace, a, b) = two_documents();
    let mut dialog = ExportDialog::new();
    dialog.run(&workspace.get(a).unwrap().session.document, UI_EXPORT_OPEN, &json!({})).unwrap().unwrap();
    assert_eq!(dialog.owner(), None);
    assert!(dialog.owner_document(&workspace).is_some());
    assert_eq!(dialog.owner(), Some(a));
    workspace.activate(b).unwrap();
    let doc = dialog.owner_document(&workspace).unwrap();
    assert!(is_document(&workspace, a, doc));
    dialog.bind_owner(Some(b));
    assert_eq!(dialog.owner(), Some(a));
}

#[test]
fn run_for_refuses_other_document() {
    let (workspace, a, b) = two_documents();
    let doc_a = &workspace.get(a).unwrap().session.document;
    let doc_b = &workspace.get(b).unwrap().session.document;
    let mut dialog = ExportDialog::new();
    dialog.run_for(a, doc_a, UI_EXPORT_OPEN, &json!({})).unwrap().unwrap();
    assert_eq!(dialog.owner(), Some(a));
    let error = dialog.run_for(b, doc_b, UI_EXPORT_SET, &json!({ "format": "png" })).unwrap().unwrap_err();
    assert_eq!(error.code, "CONTEXT_NOT_FOUND");
    let error = dialog.run_for(b, doc_b, UI_EXPORT_QUERY, &json!({})).unwrap().unwrap_err();
    assert_eq!(error.code, "CONTEXT_NOT_FOUND");
    assert!(dialog.run_for(a, doc_a, UI_EXPORT_QUERY, &json!({})).unwrap().is_ok());
    assert_eq!(dialog.owner(), Some(a));
    assert!(dialog.run_for(b, doc_b, UI_EXPORT_CLOSE, &json!({})).unwrap().is_ok());
    assert!(!dialog.is_open());
    assert_eq!(dialog.owner(), None);
}

#[test]
fn reopen_for_other_document_rebinds() {
    let (workspace, a, b) = two_documents();
    let doc_a = &workspace.get(a).unwrap().session.document;
    let doc_b = &workspace.get(b).unwrap().session.document;
    let mut dialog = ExportDialog::new();
    dialog.open_for(a, doc_a, None);
    dialog.open_for(b, doc_b, None);
    assert!(dialog.is_open());
    assert_eq!(dialog.owner(), Some(b));
    dialog.run_for(a, doc_a, UI_EXPORT_OPEN, &json!({})).unwrap().unwrap();
    assert!(dialog.is_open());
    assert_eq!(dialog.owner(), Some(a));
}

#[test]
fn closing_owner_drops_dialog() {
    let (mut workspace, a, b) = two_documents();
    let mut dialog = ExportDialog::new();
    dialog.open_for(a, &workspace.get(a).unwrap().session.document, None);
    workspace.activate(b).unwrap();
    assert!(!dialog.owner_gone(&workspace));
    workspace.close_with(a, CloseAnswer::No).unwrap();
    assert!(dialog.owner_gone(&workspace));
    assert!(dialog.drop_if_gone(&workspace));
    assert!(!dialog.is_open());
    assert_eq!(dialog.owner(), None);
    assert!(dialog.owner_document(&workspace).is_none());
    assert!(!dialog.drop_if_gone(&workspace));
}