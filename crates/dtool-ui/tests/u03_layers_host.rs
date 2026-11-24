use std::collections::BTreeSet;

use dtool_core::workspace::Workspace;
use dtool_ui::panels::layers::{layer_rows, LayersPanel};
use serde_json::json;

fn create(workspace: &mut Workspace, name: &str) -> u64 {
    workspace.session_mut().unwrap().run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    workspace.session().unwrap().document.objects.last().unwrap().id.0
}

#[test]
fn layers_panel_lists_objects_top_most_first() {
    let mut workspace = Workspace::default();
    workspace.new_document("u03");
    let a = create(&mut workspace, "a");
    let b = create(&mut workspace, "b");
    let session = workspace.session().unwrap();
    let rows = layer_rows(&session.document, &session.selection.ids, &BTreeSet::new());
    let ids: Vec<u64> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids, vec![b, a]);
}

#[test]
fn layers_row_click_sets_selection_without_mutating_document() {
    let mut workspace = Workspace::default();
    workspace.new_document("u03");
    let a = create(&mut workspace, "a");
    let version = workspace.session().unwrap().document.version;
    let mut panel = LayersPanel::default();
    panel.select(&mut workspace, a, "Replace");
    let session = workspace.session().unwrap();
    assert_eq!(panel.last_error, None);
    assert_eq!(session.document.version, version);
    assert!(session.selection.ids.iter().any(|id| id.0 == a));
}

#[test]
fn layers_panel_empty_without_document() {
    let workspace = Workspace::default();
    assert!(workspace.session().is_none());
}
