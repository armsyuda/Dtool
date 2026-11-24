use dtool_core::workspace::{CloseAnswer, ContextId};
use dtool_core::Workspace;
use dtool_ui::dialogs::export::ExportDialog;
use dtool_ui::dialogs::{DialogKind, DialogOwner, DialogRegistry};

fn two_documents() -> (Workspace, ContextId, ContextId) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    workspace.activate(a).unwrap();
    (workspace, a, b)
}

#[test]
fn sync_opens_moves_and_closes_one_entry_per_kind() {
    let (_, a, b) = two_documents();
    let mut registry = DialogRegistry::default();
    registry.sync(DialogKind::Guides, Some(DialogOwner::Document(a)));
    registry.sync(DialogKind::Guides, Some(DialogOwner::Document(a)));
    assert_eq!(registry.dialogs.len(), 1);
    registry.sync(DialogKind::Guides, Some(DialogOwner::Document(b)));
    assert_eq!(registry.dialogs.len(), 1);
    assert_eq!(registry.owner_for(DialogKind::Guides), Some(&DialogOwner::Document(b)));
    registry.sync(DialogKind::Guides, None);
    assert!(!registry.is_open(DialogKind::Guides));
    assert!(registry.dialogs.is_empty());
}

#[test]
fn sync_document_follows_open_flag_and_owner() {
    let (_, a, _) = two_documents();
    let mut registry = DialogRegistry::default();
    registry.sync_document(DialogKind::GridAxis, true, Some(a));
    assert_eq!(registry.owner_for(DialogKind::GridAxis), Some(&DialogOwner::Document(a)));
    registry.sync_document(DialogKind::GridAxis, false, Some(a));
    assert!(!registry.is_open(DialogKind::GridAxis));
    registry.sync_document(DialogKind::GridAxis, true, None);
    assert!(!registry.is_open(DialogKind::GridAxis));
}

#[test]
fn kinds_for_lists_document_dialogs_only() {
    let (_, a, b) = two_documents();
    let mut registry = DialogRegistry::default();
    registry.sync(DialogKind::Export, Some(DialogOwner::Document(a)));
    registry.sync(DialogKind::ResourceManager, Some(DialogOwner::Document(a)));
    registry.sync(DialogKind::LockedDelete, Some(DialogOwner::Document(b)));
    registry.sync(DialogKind::About, Some(DialogOwner::App));
    assert_eq!(registry.kinds_for(a), vec![DialogKind::Export, DialogKind::ResourceManager]);
    assert_eq!(registry.kinds_for(b), vec![DialogKind::LockedDelete]);
}

#[test]
fn closing_a_document_reports_and_drops_only_its_dialogs() {
    let (mut workspace, a, b) = two_documents();
    let mut registry = DialogRegistry::default();
    registry.sync(DialogKind::Export, Some(DialogOwner::Document(a)));
    registry.sync(DialogKind::BaselineGrid, Some(DialogOwner::Document(a)));
    registry.sync(DialogKind::Guides, Some(DialogOwner::Document(b)));
    registry.sync(DialogKind::About, Some(DialogOwner::App));
    assert!(registry.gone_kinds(&workspace).is_empty());
    workspace.close_with(a, CloseAnswer::No).unwrap();
    assert_eq!(registry.gone_kinds(&workspace), vec![DialogKind::Export, DialogKind::BaselineGrid]);
    assert_eq!(registry.take_gone(&workspace), vec![DialogKind::Export, DialogKind::BaselineGrid]);
    assert!(registry.is_open(DialogKind::Guides));
    assert!(registry.is_open(DialogKind::About));
    assert!(registry.take_gone(&workspace).is_empty());
}

#[test]
fn export_dialog_owner_feeds_the_registry() {
    let (mut workspace, a, b) = two_documents();
    let mut export = ExportDialog::new();
    export.open_for(a, &workspace.get(a).unwrap().session.document, None);
    let mut registry = DialogRegistry::default();
    registry.sync(DialogKind::Export, export.owner().map(DialogOwner::Document));
    workspace.activate(b).unwrap();
    registry.confirm_session(&mut workspace, DialogKind::Export).unwrap().view.zoom = 4.0;
    assert_eq!(workspace.get(a).unwrap().session.view.zoom, 4.0);
    assert_ne!(workspace.get(b).unwrap().session.view.zoom, 4.0);
    export.close();
    registry.sync(DialogKind::Export, export.owner().map(DialogOwner::Document));
    assert!(!registry.is_open(DialogKind::Export));
}