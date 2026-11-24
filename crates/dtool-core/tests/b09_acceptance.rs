use dtool_core::{Scope, Workspace};
use dtool_protocol::{Command, CommandResult};

fn workspace() -> Workspace {
    let mut ws = Workspace::default();
    ws.new_document("b09");
    ws
}

fn run(ws: &mut Workspace, command: Command) -> CommandResult {
    ws.run(Scope::active(), command).1
}

#[test]
fn test_guides_add_update_delete() {
    let mut ws = workspace();
    
    let cmd_add = Command::with_params("guides.add", serde_json::json!({
        "kind": "Horizontal",
        "position": 100.0,
        "name": "Test Guide"
    }));
    let res_add = run(&mut ws, cmd_add);
    assert!(res_add.is_ok());

    let cmd_update = Command::with_params("guides.update", serde_json::json!({
        "id": 1,
        "position": 200.0
    }));
    let res_update = run(&mut ws, cmd_update);
    assert!(res_update.is_ok());

    let cmd_delete = Command::with_params("guides.delete", serde_json::json!({
        "id": 1
    }));
    let res_delete = run(&mut ws, cmd_delete);
    assert!(res_delete.is_ok());
}

#[test]
fn test_guides_manager_rename() {
    let mut ws = workspace();
    
    let cmd_add = Command::with_params("guides.add", serde_json::json!({
        "kind": "Vertical",
        "position": 50.0,
        "name": "Old Name"
    }));
    let _ = run(&mut ws, cmd_add);

    let cmd_rename = Command::with_params("guides.update", serde_json::json!({
        "id": 1,
        "name": "New Name"
    }));
    let res_rename = run(&mut ws, cmd_rename);
    assert!(res_rename.is_ok());
}

#[test]
fn test_grid_kind_and_show_flags() {
    let mut ws = workspace();

    let cmd_set_grid = Command::with_params("grid.set", serde_json::json!({
        "kind": "Isometric",
        "show": true,
        "spacing": [10.0, 10.0],
        "subdivisions": 4,
        "show_pixel": false,
        "show_baseline": true
    }));
    let res_set = run(&mut ws, cmd_set_grid);
    assert!(res_set.is_ok());

    let cmd_query = Command::with_params("grid.query", serde_json::json!({}));
    let res_query = run(&mut ws, cmd_query);
    assert!(res_query.is_ok());
}

#[test]
fn test_snap_candidates_from_prefs() {
    let mut ws = workspace();
    
    let cmd_prefs = Command::with_params("prefs.set", serde_json::json!({
        "snap_enabled": true,
        "snap_smart_guides": true
    }));
    let _ = run(&mut ws, cmd_prefs);

    let cmd_candidates = Command::with_params("snap.candidates", serde_json::json!({
        "position": [10.0, 20.0]
    }));
    let res_candidates = run(&mut ws, cmd_candidates);
    assert!(res_candidates.is_ok());
}

#[test]
fn test_smart_guide_overlay_not_persisting() {
    let mut ws = workspace();

    let cmd_add_guide = Command::with_params("guides.add", serde_json::json!({
        "kind": "Horizontal",
        "position": 10.0,
        "name": "Persisted Guide"
    }));
    let _ = run(&mut ws, cmd_add_guide);

    let cmd_smart_feedback = Command::with_params("snap.smart_guide.feedback", serde_json::json!({
        "kind": "Vertical",
        "position": 50.0
    }));
    let res_feedback = run(&mut ws, cmd_smart_feedback);
    assert!(res_feedback.is_ok());

    let cmd_list = Command::with_params("guides.list", serde_json::json!({}));
    let res_list = run(&mut ws, cmd_list);
    if let Ok(data) = res_list {
        let guides = data.get("guides").and_then(|v| v.as_array());
        assert!(guides.is_some());
        assert_eq!(guides.unwrap().len(), 1);
    } else {
        panic!("Expected Ok result for guides.list");
    }
}
