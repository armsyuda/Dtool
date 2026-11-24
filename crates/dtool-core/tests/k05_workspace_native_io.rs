use dtool_core::native_io::temp_path;
use dtool_core::{Command, Scope, Workspace};
use serde_json::{json, Value};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_k05_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn native_with_forward_field(path: &std::path::Path) {
    let mut value = serde_json::to_value(dtool_core::document::Document::default()).unwrap();
    value.as_object_mut().unwrap().insert("forward_field".into(), json!({"kept": [1, 2, 3]}));
    value.as_object_mut().unwrap().insert("snap_feedback".into(), json!({"candidate": [1, 2]}));
    std::fs::write(path, serde_json::to_string(&value).unwrap()).unwrap();
}

#[test]
fn workspace_resave_keeps_unknown_top_level_keys() {
    let dir = temp_dir("extras");
    let path = dir.join("a.dtool");
    native_with_forward_field(&path);
    let mut workspace = Workspace::default();
    let id = workspace.open_path(&path).unwrap();
    assert_eq!(workspace.native_extras(id).unwrap()["forward_field"]["kept"], json!([1, 2, 3]));
    workspace.run(Scope::of(id), Command::with_params("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [5.0, 5.0]}))).1.unwrap();
    workspace.save(id, None).unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["forward_field"]["kept"], json!([1, 2, 3]));
    assert!(saved.get("snap_feedback").is_none());
    let copy = dir.join("b.dtool");
    workspace.save_as(id, &copy).unwrap();
    let copied: Value = serde_json::from_str(&std::fs::read_to_string(&copy).unwrap()).unwrap();
    assert_eq!(copied["forward_field"], saved["forward_field"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn workspace_open_removes_leftover_temp_file_and_uses_shared_reader() {
    let dir = temp_dir("tmp");
    let path = dir.join("a.dtool");
    std::fs::write(&path, serde_json::to_string(&dtool_core::document::Document::default()).unwrap()).unwrap();
    std::fs::write(temp_path(&path), b"partial").unwrap();
    let mut workspace = Workspace::default();
    let id = workspace.open_path(&path).unwrap();
    assert!(!temp_path(&path).exists());
    assert!(!workspace.get(id).unwrap().is_modified());
    assert!(workspace.native_extras(id).is_none());
    let broken = dir.join("broken.dtool");
    std::fs::write(&broken, b"{\"objects\": [").unwrap();
    assert_eq!(workspace.open_path(&broken).unwrap_err().code, "IO.INVALID_FORMAT");
    assert_eq!(workspace.open_path(&dir.join("missing.dtool")).unwrap_err().code, "IO.NOT_FOUND");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failed_workspace_write_keeps_old_file_and_dirty_state() {
    let dir = temp_dir("fail");
    let path = dir.join("a.dtool");
    let mut workspace = Workspace::default();
    let id = workspace.new_document("a");
    workspace.save_as(id, &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    workspace.run(Scope::of(id), Command::with_params("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [5.0, 5.0]}))).1.unwrap();
    std::fs::create_dir_all(temp_path(&path)).unwrap();
    assert!(workspace.save(id, None).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(workspace.get(id).unwrap().is_modified());
    assert!(temp_path(&path).is_dir());
    let _ = std::fs::remove_dir_all(&dir);
}
