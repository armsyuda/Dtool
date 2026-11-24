use dtool_core::workspace::{SharedServices, Workspace};
use std::sync::Arc;

#[test]
fn view_zoom_survives_switching_tabs() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let id1 = workspace.new_document("Doc1");
    let id2 = workspace.new_document("Doc2");

    if let Some(ctx) = workspace.get_mut(id1) {
        ctx.view_state.zoom = 2.5;
    }

    workspace.activate(id2).unwrap();
    workspace.activate(id1).unwrap();

    let ctx = workspace.get(id1).unwrap();
    assert_eq!(ctx.view_state.zoom, 2.5);
}

#[test]
fn pending_edit_is_finished_before_switching() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let id1 = workspace.new_document("Doc1");
    let id2 = workspace.new_document("Doc2");

    if let Some(ctx) = workspace.get_mut(id1) {
        ctx.pending_edit = true;
    }

    workspace.activate(id2).unwrap();

    let ctx1 = workspace.get(id1).unwrap();
    assert!(!ctx1.pending_edit);
}

#[test]
fn activating_the_active_tab_changes_nothing() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let id1 = workspace.new_document("Doc1");
    
    if let Some(ctx) = workspace.get_mut(id1) {
        ctx.view_state.zoom = 3.0;
        ctx.pending_edit = true;
    }

    workspace.activate(id1).unwrap();

    let ctx = workspace.get(id1).unwrap();
    assert_eq!(ctx.view_state.zoom, 3.0);
    assert!(ctx.pending_edit);
}
