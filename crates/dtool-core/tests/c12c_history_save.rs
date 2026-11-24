#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::prefs_cmd;
use dtool_core::history_store::sidecar_path;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use serde_json::json;

fn names(workspace: &Workspace) -> Vec<String> {
    workspace.session().unwrap().document.objects.iter().map(|object| object.name.clone()).collect()
}

fn edited(workspace: &mut Workspace) -> dtool_core::workspace::ContextId {
    let id = workspace.new_document("History");
    for name in ["first", "second"] {
        workspace.run(Scope::of(id), Command::with_params("object.create", json!({ "name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0] }))).1.unwrap();
    }
    id
}

#[test]
fn c12c_doc_save_writes_history_only_when_flag_is_on_and_reopen_restores_it() {
    let home = test_home::enter("c12c_history_save");
    std::fs::write(home.root.join("prefs.json"), r#"{ "threads": 8, "undo_limit": 7 }"#).unwrap();
    assert!(prefs_cmd::install(home.root.join("prefs.json")));
    assert_eq!(prefs_cmd::snapshot().unwrap().undo_limit, 7);

    let mut workspace = Workspace::default();
    let id = edited(&mut workspace);
    let plain = home.root.join("plain.dtool");
    workspace.run(Scope::of(id), Command::with_params("doc.save_as", json!({ "path": plain }))).1.unwrap();
    assert!(plain.exists());
    assert!(!sidecar_path(&plain).exists());

    workspace.run(Scope::of(id), Command::with_params("history.save_with_doc.set", json!({ "enabled": true }))).1.unwrap();
    let kept = home.root.join("kept.dtool");
    workspace.run(Scope::of(id), Command::with_params("doc.save_as", json!({ "path": kept }))).1.unwrap();
    assert!(sidecar_path(&kept).exists());
    let bytes = std::fs::read(&kept).unwrap();

    let mut reopened = Workspace::default();
    let (opened, reply) = reopened.run(Scope::active(), Command::with_params("doc.open", json!({ "path": kept })));
    let reply = reply.unwrap();
    assert_eq!(reply["history"]["restored"], 2, "{reply}");
    let opened = opened.unwrap();
    assert!(reopened.get(opened).unwrap().session.history.save_with_document);
    assert_eq!(names(&reopened), vec!["first", "second"]);
    reopened.run(Scope::of(opened), Command::new("edit.undo")).1.unwrap();
    assert_eq!(names(&reopened), vec!["first"]);
    reopened.run(Scope::of(opened), Command::new("edit.undo")).1.unwrap();
    assert!(names(&reopened).is_empty());
    assert_eq!(std::fs::read(&kept).unwrap(), bytes);

    let mut fresh = Workspace::default();
    let (_, reply) = fresh.run(Scope::active(), Command::with_params("doc.open", json!({ "path": plain })));
    let reply = reply.unwrap();
    assert_ne!(reply["history"]["restored"], 2, "{reply}");
    assert!(!fresh.session().unwrap().undo.can_undo());

    let (_, reply) = workspace.run(Scope::of(id), Command::with_params("pref.performance.set", json!({ "threads": 3 })));
    assert_eq!(reply.unwrap_err().code, "INVALID_PARAMS");
    workspace.run(Scope::of(id), Command::with_params("pref.performance.set", json!({ "undo_limit": 9 }))).1.unwrap();
    let stored: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(home.root.join("prefs.json")).unwrap()).unwrap();
    assert_eq!(stored["undo_limit"], 9);
    assert!(stored.get("threads").is_none(), "{stored}");
    assert!(prefs_cmd::snapshot().unwrap().query(None).unwrap().get("threads").is_none());
}