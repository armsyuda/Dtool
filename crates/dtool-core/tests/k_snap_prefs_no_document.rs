use dtool_core::snap_prefs::set_thread_path;
use dtool_core::{Command, Scope, Workspace};
use serde_json::json;

#[test]
fn snap_prefs_work_without_a_document_and_carry_into_new_documents() {
    let dir = std::env::temp_dir().join(format!("dtool_snap_nodoc_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("snap_prefs.json");
    set_thread_path(Some(path.clone()));
    let mut workspace = Workspace::default();
    let (context, query) = workspace.run(Scope::active(), Command::with_params("snap.prefs.query", json!({})));
    assert_eq!(context, None);
    assert_eq!(query.unwrap()["snap_to_grid"], json!(false));
    let (_, set) = workspace.run(Scope::active(), Command::with_params("snap.prefs.set", json!({"snap_to_grid": true})));
    assert!(set.is_ok());
    assert!(path.exists());
    let (_, query) = workspace.run(Scope::active(), Command::with_params("snap.prefs.query", json!({})));
    assert_eq!(query.unwrap()["snap_to_grid"], json!(true));
    let (_, bad) = workspace.run(Scope::active(), Command::with_params("snap.prefs.set", json!({"snap_to_grid": "yes"})));
    assert_eq!(bad.unwrap_err().code, "INVALID_PARAMS");
    let id = workspace.new_document("a");
    let (_, query) = workspace.run(Scope::of(id), Command::with_params("snap.prefs.query", json!({})));
    assert_eq!(query.unwrap()["snap_to_grid"], json!(true));
    set_thread_path(None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn app_commands_answer_without_a_document() {
    let mut workspace = Workspace::default();
    let (_, about) = workspace.run(Scope::active(), Command::with_params("app.about", json!({})));
    assert!(about.unwrap().get("version").is_some());
    let (_, status) = workspace.run(Scope::active(), Command::with_params("app.status", json!({})));
    assert_eq!(status.unwrap_err().code, "NO_DOCUMENT");
    let (_, bad) = workspace.run(Scope::active(), Command::with_params("app.about", json!({"x": 1})));
    assert_eq!(bad.unwrap_err().code, "INVALID_PARAMS");
    let (_, other) = workspace.run(Scope::active(), Command::with_params("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [1.0, 1.0]})));
    assert_eq!(other.unwrap_err().code, "NO_DOCUMENT");
}
