use dtool_core::workspace::{CloseAnswer, CloseCheck, SharedServices, Workspace};
use std::sync::Arc;

#[test]
fn unmodified_close_removes_at_once() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);
    let id = workspace.new_document("Test");
    
    assert_eq!(workspace.request_close(id).ok(), Some(CloseCheck::Closed));
    assert!(workspace.close_with(id, CloseAnswer::No).is_err());
    assert!(workspace.get(id).is_none());
}

#[test]
fn modified_close_needs_an_answer() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);
    let id = workspace.new_document("Test");
    
    if let Some(ctx) = workspace.get_mut(id) {
        ctx.pending_edit = true;
    }
    
    assert_eq!(workspace.request_close(id).ok(), Some(CloseCheck::NeedsAnswer));
}

#[test]
fn no_removes_cancel_keeps() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);
    let id = workspace.new_document("Test");
    
    if let Some(ctx) = workspace.get_mut(id) {
        ctx.pending_edit = true;
    }
    
    assert_eq!(workspace.close_with(id, CloseAnswer::No).ok(), Some(true));
    assert!(workspace.get(id).is_none());
    
    let id2 = workspace.new_document("Test2");
    if let Some(ctx) = workspace.get_mut(id2) {
        ctx.pending_edit = true;
    }
    
    assert_eq!(workspace.close_with(id2, CloseAnswer::Cancel).ok(), Some(false));
    assert!(workspace.get(id2).is_some());
}

#[test]
fn yes_on_untitled_keeps_tab_until_save_as() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);
    let id = workspace.new_document("Test");
    
    if let Some(ctx) = workspace.get_mut(id) {
        ctx.pending_edit = true;
    }
    
    assert!(workspace.close_with(id, CloseAnswer::Yes).is_err());
    assert!(workspace.get(id).is_some());
}

#[test]
fn closing_middle_active_tab_activates_right_neighbour() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);
    
    let id1 = workspace.new_document("Doc1");
    let id2 = workspace.new_document("Doc2");
    let id3 = workspace.new_document("Doc3");
    
    workspace.activate(id2).unwrap();
    assert_eq!(workspace.active(), Some(id2));
    
    workspace.close_with(id2, CloseAnswer::No).unwrap();
    
    assert_eq!(workspace.active(), Some(id3));
    assert!(workspace.get(id1).is_some());
}

#[test]
fn close_others_returns_only_modified_tabs() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);
    
    let id1 = workspace.new_document("Doc1");
    let id2 = workspace.new_document("Doc2");
    let id3 = workspace.new_document("Doc3");
    
    if let Some(ctx) = workspace.get_mut(id2) {
        ctx.pending_edit = true;
    }
    
    let closed = workspace.close_others(id1);
    
    assert_eq!(closed, vec![id2]);
    assert!(workspace.get(id1).is_some());
    assert!(workspace.get(id2).is_some());
    assert!(workspace.get(id3).is_none());
}
