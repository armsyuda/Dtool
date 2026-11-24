use dtool_core::command::preflight;
use dtool_core::document::{Document, Units};
use dtool_core::Session;
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1))
}

fn create_cmd(name: &str) -> Command {
    Command { name: "object.create".to_string(), params: json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]}) }
}

fn rename_cmd(id: u64, name: &str) -> Command {
    Command { name: "object.update".to_string(), params: json!({"id": id, "name": name}) }
}

#[test]
fn test_mutating_command_bumps_generation_and_returns_doc_version() {
    let mut session = session();
    let result = session.run(&create_cmd("a"));
    assert!(result.is_ok());
    assert_eq!(session.generation, 1);
    let outcome = result.unwrap();
    assert_eq!(outcome["doc_version"], 1);
}

#[test]
fn test_failing_command_does_not_bump_generation() {
    let mut session = session();
    let invalid_command = Command { name: "unknown.command".to_string(), params: json!({}) };
    let result = session.run(&invalid_command);
    assert!(result.is_err());
    assert_eq!(session.generation, 0);
}

#[test]
fn test_document_after_successful_commit_matches_outcome() {
    let mut session = session();
    let outcome = session.run(&create_cmd("a")).unwrap();
    let snapshot = session.query(None).unwrap();
    assert_eq!(snapshot["doc_version"], outcome["doc_version"]);
    assert_eq!(session.document.objects.len(), 1);
    assert_eq!(session.document.objects[0].name, "a");
}

#[test]
fn test_batch_command_single_generation_bump() {
    let mut session = session();
    session.run(&create_cmd("a")).unwrap();
    let id = session.document.objects[0].id.0;
    let before = session.generation;
    let batch_command = Command {
        name: "edit.batch".to_string(),
        params: json!({
            "commands": [
                {"name": "object.create", "params": {"name": "b", "pos": [0.0, 0.0], "size": [10.0, 10.0]}},
                {"name": "object.update", "params": {"id": id, "name": "renamed"}}
            ]
        }),
    };
    let result = session.run(&batch_command);
    assert!(result.is_ok());
    let outcome = result.unwrap();
    assert_eq!(outcome["doc_version"], session.generation);
    assert!(session.generation > before);
    assert_eq!(outcome["changes"].as_array().unwrap().len(), 2);
}

#[test]
fn test_batch_preflight_failure_no_generation_bump() {
    let mut session = session();
    let batch_command = Command {
        name: "edit.batch".to_string(),
        params: json!({
            "commands": [
                {"name": "object.create", "params": {"name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0]}},
                {"name": "invalid.command", "params": {}}
            ]
        }),
    };
    let result = session.run(&batch_command);
    assert!(result.is_err());
    assert_eq!(session.generation, 0);
    assert!(session.document.objects.is_empty());
}

#[test]
fn test_undo_stack_push_on_mutating_command() {
    let mut session = session();
    let result = session.run(&create_cmd("a"));
    assert!(result.is_ok());
    assert!(session.undo.can_undo());
}

#[test]
fn test_query_since_returns_changes() {
    let mut session = session();
    session.run(&create_cmd("a")).unwrap();
    let id = session.document.objects[0].id.0;
    let result2 = session.run(&rename_cmd(id, "renamed_document"));
    assert!(result2.is_ok());
    let changes = session.query(Some(1)).unwrap();
    assert_eq!(changes["doc_version"], 2);
    assert!(changes["changes"].is_array());
}

#[test]
fn test_query_stale_since_returns_error() {
    let mut session = session();
    let _ = session.run(&create_cmd("a"));
    let query_result = session.query(Some(10));
    assert!(query_result.is_err());
}

#[test]
fn test_preflight_does_not_mutate() {
    let session = session();
    let command = create_cmd("a");
    let preflight_result = preflight(&session, &command);
    assert!(preflight_result.is_ok());
    assert_eq!(session.generation, 0);
    assert!(session.document.objects.is_empty());
}

#[test]
fn test_preflight_fails_for_invalid_command() {
    let session = session();
    let invalid_command = Command { name: "unknown.command".to_string(), params: json!({}) };
    let preflight_result = preflight(&session, &invalid_command);
    assert!(preflight_result.is_err());
}
