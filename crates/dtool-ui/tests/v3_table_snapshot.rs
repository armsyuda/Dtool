use dtool_core::{Command, Scope, Workspace};
use dtool_ui::table::snapshot::TableSnapshots;
use serde_json::json;

#[test]
fn snapshot_uses_actual_session_versions_and_same_selected_cells() {
    let mut workspace=Workspace::default();
    let context=workspace.new_document("table");
    let (_,created)=workspace.run(Scope::active(),Command::with_params("table.create",json!({"rows":2,"cols":2,"insert":true})));
    let table=created.unwrap()["id"].as_u64().unwrap();
    let (_,selected)=workspace.run(Scope::active(),Command::with_params("table.selection.set",json!({"id":table,"mode":"all"})));
    selected.unwrap();
    let mut snapshots=TableSnapshots::new();
    assert!(snapshots.refresh(&workspace));
    let session=&workspace.get(context).unwrap().session;
    let entry=snapshots.views(context).unwrap();
    let view=entry.views.get(&table).unwrap().as_ref().unwrap();
    assert_eq!(view.generation,session.generation);
    assert_eq!(view.document_version,session.document.version);
    assert_eq!(entry.selection.cells.len(),4);
    assert!(!snapshots.refresh(&workspace));
}
