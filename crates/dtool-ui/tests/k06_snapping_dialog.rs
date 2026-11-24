use dtool_core::snap_prefs::set_thread_path;
use dtool_core::Workspace;
use dtool_ui::snapping_dialog::{apply_toggle, handles_menu, query_prefs, row_enabled, SnappingDialog, ROWS};
use serde_json::json;

fn temp_prefs(name: &str) {
    let dir = std::env::temp_dir().join(format!("dtool_k06_snap_{}_{}", name, std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    set_thread_path(Some(dir.join("snap_prefs.json")));
}

#[test]
fn toggle_off_changes_global_query_without_doc_version() {
    temp_prefs("global");
    let mut workspace = Workspace::default();
    let first = workspace.new_document("a");
    let second = workspace.new_document("b");
    let versions = [workspace.get(first).unwrap().session.document.version, workspace.get(second).unwrap().session.document.version];
    workspace.activate(first);
    apply_toggle(&mut workspace, "enabled", json!(false)).unwrap();
    assert_eq!(query_prefs(&mut workspace).unwrap()["enabled"], json!(false));
    workspace.activate(second);
    assert_eq!(query_prefs(&mut workspace).unwrap()["enabled"], json!(false));
    assert_eq!(workspace.get(first).unwrap().session.document.version, versions[0]);
    assert_eq!(workspace.get(second).unwrap().session.document.version, versions[1]);
    set_thread_path(None);
}

#[test]
fn construction_and_curve_not_in_dialog_rows() {
    assert!(ROWS.iter().all(|row| row.0 != "construction" && row.0 != "curve"));
    assert_eq!(ROWS.len(), 24);
}

#[test]
fn child_rows_follow_parent_flags() {
    let prefs = json!({"enabled": true, "snap_to_spreads": false, "object_snapping": true, "snap_to_object_bounds": true});
    assert!(!row_enabled(&prefs, Some("snap_to_spreads")));
    assert!(row_enabled(&prefs, Some("snap_to_object_bounds")));
    let off = json!({"enabled": false, "object_snapping": true, "snap_to_object_bounds": true});
    assert!(!row_enabled(&off, Some("snap_to_object_bounds")));
}

#[test]
fn appshell_has_snapping_menu_handler() {
    let mut dialog = SnappingDialog::default();
    assert!(handles_menu("view.snapping", &mut dialog));
    assert!(dialog.open);
    assert!(!handles_menu("view.other", &mut SnappingDialog::default()));
}
