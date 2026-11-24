#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::prefs_cmd;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use serde_json::json;

#[test]
fn new_and_opened_contexts_take_stored_prefs_without_a_shell() {
    let home = test_home::enter("c_hub_b3_prefs");
    assert!(prefs_cmd::install(home.root.join("prefs.json")));
    let mut prefs = prefs_cmd::snapshot().unwrap();
    prefs.undo_limit = 2;
    prefs.recent_count = 2;
    assert!(prefs_cmd::replace(prefs));
    let mut ws = Workspace::default();
    let id = ws.new_document("Prefs");
    assert_eq!(ws.get(id).unwrap().session.undo.limit(), 2);
    for name in ["a", "b", "c"] {
        ws.run(Scope::of(id), Command::with_params("object.create", json!({ "name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0] }))).1.unwrap();
    }
    let mut undos = 0;
    while ws.get(id).unwrap().session.undo.can_undo() {
        ws.run(Scope::of(id), Command::new("edit.undo")).1.unwrap();
        undos += 1;
    }
    assert_eq!(undos, 2);
    for name in ["x", "y", "z"] {
        let path = home.root.join(format!("{name}.dtool"));
        std::fs::write(&path, serde_json::to_string(&dtool_core::document::Document::default()).unwrap()).unwrap();
        let opened = ws.open_path(&path).unwrap();
        assert_eq!(ws.get(opened).unwrap().session.undo.limit(), 2);
    }
    assert_eq!(ws.recent().len(), 2);
}

#[test]
fn catalog_lists_pref_performance_set() {
    let spec = dtool_protocol::CommandCatalog::new().resolve("pref.performance.set").cloned().expect("row");
    assert!(spec.implemented && spec.mutating && !spec.undoable);
}