use dtool_core::document::{Document, Units};
use dtool_core::undo::Change;
use dtool_core::Session;
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1))
}

fn push_change(session: &mut Session) {
    let change = Change {
        name: "test_change".to_string(),
        redo: Command { name: "edit.undo".to_string(), params: json!({}) },
        undo: Command { name: "edit.redo".to_string(), params: json!({}) },
    };
    session.undo.push(change);
    session.generation += 1;
}

#[test]
fn full_query_reports_current_doc_version() {
    let session = session();
    let cmd = Command { name: "doc.query".to_string(), params: json!({}) };
    assert_eq!(cmd.name, "doc.query");
    let result = session.query(None);
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["doc_version"], 0);
}

#[test]
fn since_last_version_returns_empty_change_list() {
    let mut session = session();
    push_change(&mut session);
    let result = session.query(Some(1));
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["doc_version"], 1);
    assert_eq!(value["changes"].as_array().unwrap().len(), 0);
}

#[test]
fn stale_since_errors_and_does_not_mutate() {
    let mut session = session();
    push_change(&mut session);
    push_change(&mut session);
    let result = session.query(Some(0));
    let generation = session.generation;
    if let Err(error) = result {
        assert_eq!(error.code, "INVALID_PARAMS");
    }
    assert_eq!(session.generation, generation);
    assert_eq!(session.generation, 2);
}

#[test]
fn future_since_errors_and_does_not_mutate() {
    let mut session = session();
    push_change(&mut session);
    let result = session.query(Some(2));
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(error.code, "INVALID_PARAMS");
    assert_eq!(session.generation, 1);
}
