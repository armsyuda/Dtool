use dtool_core::object::Object;
use dtool_core::{ContextId, Workspace};
use dtool_ui::float::{handles, should_dock, FloatManager, DOCK_ALL_MENU_ID, DOCK_MENU_ID, FLOAT_ALL_MENU_ID, FLOAT_MENU_ID};

fn snapshot(workspace: &Workspace, id: ContextId) -> (u64, u64, String, bool, usize) {
    let context = workspace.get(id).unwrap();
    let session = &context.session;
    (session.document.version, session.generation, serde_json::to_string(&session.document).unwrap(), context.is_modified(), workspace.contexts().len())
}

fn workspace_with(names: &[&str]) -> (Workspace, Vec<ContextId>) {
    let mut workspace = Workspace::default();
    let ids: Vec<ContextId> = names.iter().map(|name| workspace.new_document(name)).collect();
    for id in &ids {
        workspace.get_mut(*id).unwrap().session.document.create_object(Object { size: [10.0, 20.0], ..Object::default() });
    }
    (workspace, ids)
}

#[test]
fn float_then_dock_leaves_document_unchanged() {
    let (workspace, ids) = workspace_with(&["c01-float"]);
    let id = ids[0];
    let before = snapshot(&workspace, id);
    let mut floats = FloatManager::default();
    assert_eq!(workspace.active(), Some(id));
    assert!(floats.arrange(FLOAT_MENU_ID, &workspace));
    assert!(floats.is_floating(id));
    assert_eq!(snapshot(&workspace, id), before);
    assert!(floats.arrange(DOCK_MENU_ID, &workspace));
    assert!(!floats.is_floating(id));
    assert_eq!(snapshot(&workspace, id), before);
    floats.toggle_float(id);
    floats.toggle_float(id);
    assert!(floats.floating.is_empty());
    assert_eq!(snapshot(&workspace, id), before);
}

#[test]
fn floating_one_document_keeps_the_other_open_and_clean() {
    let (mut workspace, ids) = workspace_with(&["c01-a", "c01-b"]);
    workspace.activate(ids[0]).unwrap();
    let other = snapshot(&workspace, ids[1]);
    let mut floats = FloatManager::default();
    assert!(floats.arrange(FLOAT_MENU_ID, &workspace));
    assert!(floats.is_floating(ids[0]));
    assert!(!floats.is_floating(ids[1]));
    assert_eq!(floats.floating.len(), 1);
    assert_eq!(snapshot(&workspace, ids[1]), other);
    assert!(workspace.get(ids[1]).is_some());
    assert!(!workspace.get(ids[1]).unwrap().is_modified());
    assert_eq!(workspace.active(), Some(ids[0]));
    floats.float(ids[0]);
    assert_eq!(floats.floating.len(), 1);
}

#[test]
fn dock_all_brings_floated_tabs_back() {
    let (mut workspace, ids) = workspace_with(&["c01-x", "c01-y"]);
    let before: Vec<_> = ids.iter().map(|id| snapshot(&workspace, *id)).collect();
    let mut floats = FloatManager::default();
    assert!(floats.arrange(FLOAT_ALL_MENU_ID, &workspace));
    assert!(ids.iter().all(|id| floats.is_floating(*id)));
    assert!(floats.arrange(DOCK_ALL_MENU_ID, &workspace));
    assert!(floats.floating.is_empty());
    assert!(!floats.arrange(DOCK_ALL_MENU_ID, &workspace));
    floats.float(ids[1]);
    workspace.activate(ids[0]).unwrap();
    assert!(floats.arrange(DOCK_MENU_ID, &workspace));
    assert!(floats.floating.is_empty());
    let after: Vec<_> = ids.iter().map(|id| snapshot(&workspace, *id)).collect();
    assert_eq!(after, before);
}

#[test]
fn escape_dock_or_close_docks_a_floating_viewport() {
    assert!(should_dock(false, true, false));
    assert!(should_dock(false, false, true));
    assert!(should_dock(true, false, false));
    assert!(!should_dock(false, false, false));
    assert!(handles(FLOAT_MENU_ID) && handles(FLOAT_ALL_MENU_ID) && handles(DOCK_MENU_ID) && handles(DOCK_ALL_MENU_ID));
    assert!(!handles("window.arrange"));
    let workspace = Workspace::default();
    let mut floats = FloatManager::default();
    assert!(!floats.arrange(FLOAT_MENU_ID, &workspace));
    assert!(!floats.arrange(FLOAT_ALL_MENU_ID, &workspace));
    assert!(!floats.arrange(DOCK_MENU_ID, &workspace));
}