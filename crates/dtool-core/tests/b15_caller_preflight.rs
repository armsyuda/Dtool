use dtool_core::command::preflight;
use dtool_core::workspace::{run_shared, Scope, SharedWorkspace, Workspace};
use dtool_protocol::Command;
use serde_json::json;

fn shared() -> SharedWorkspace {
    let mut workspace = Workspace::default();
    workspace.new_document("caller");
    workspace.into_shared()
}

fn generation(shared: &SharedWorkspace) -> u64 {
    shared.lock().unwrap().session().unwrap().generation
}

#[test]
fn product_shared_path_runs_read_commands() {
    let shared = shared();
    for name in ["app.about", "app.version", "app.status"] {
        let (_, result) = run_shared(&shared, Scope::active(), Command::with_params(name, json!({})));
        assert!(result.is_ok(), "{name}");
    }
}

#[test]
fn failing_mutating_command_is_rejected_by_preflight_and_changes_nothing() {
    let shared = shared();
    let before = generation(&shared);
    let command = Command::with_params("object.update", json!({ "id": 999_999, "name": "ghost" }));
    {
        let guard = shared.lock().unwrap();
        assert_eq!(preflight(guard.session().unwrap(), &command).unwrap_err().code, "INVALID_PARAMS");
    }
    let (_, result) = run_shared(&shared, Scope::active(), command);
    assert!(result.is_err());
    assert_eq!(generation(&shared), before);
}

#[test]
fn unknown_command_is_rejected_by_the_catalog_gate_and_changes_nothing() {
    let shared = shared();
    let before = generation(&shared);
    let (_, result) = run_shared(&shared, Scope::active(), Command::with_params("nonexistent.command", json!({})));
    assert_eq!(result.unwrap_err().code, "UNKNOWN_COMMAND");
    assert_eq!(generation(&shared), before);
}