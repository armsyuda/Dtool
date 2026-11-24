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

#[test]
fn test_successful_batch_is_one_undo_step_and_one_generation_bump() {
    let mut session = session();
    session.run(&create_cmd("a")).unwrap();
    let id = session.document.objects[0].id.0;
    let cmd2 = Command { name: "object.update".to_string(), params: json!({"id": id, "name": "test"}) };
    session.run(&cmd2).unwrap();
    assert_eq!(session.generation, 2);
    assert!(session.undo.can_undo());
    let undo_cmd = session.undo.undo().unwrap();
    assert_eq!(undo_cmd.name, "object.restore");
    assert!(session.undo.can_redo());
}

#[test]
fn test_preflight_failure_runs_nothing() {
    let mut session = session();
    session.run(&create_cmd("a")).unwrap();
    let invalid_cmd = Command { name: "unknown.command".to_string(), params: json!({}) };
    let batch_cmd = Command {
        name: "edit.batch".to_string(),
        params: json!({ "commands": [create_cmd("b"), invalid_cmd] }),
    };
    let result = session.run(&batch_cmd);
    assert!(result.is_err());
    assert_eq!(session.generation, 1);
    assert_eq!(session.document.objects.len(), 1);
}

#[test]
fn test_empty_commands_list_returns_invalid_params() {
    let mut session = session();
    let batch_cmd = Command { name: "edit.batch".to_string(), params: json!({ "commands": [] }) };
    let result = session.run(&batch_cmd);
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.code, "INVALID_PARAMS");
    }
}
