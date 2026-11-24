use dtool_core::workspace::{ContextId, Scope, SharedServices, Workspace};
use std::sync::Arc;

#[test]
fn test_run_with_active_scope_goes_to_active_tab() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let id_a = workspace.new_document("A");
    let id_b = workspace.new_document("B");

    workspace.activate(id_b).unwrap();
    assert_ne!(id_a, id_b);

    let command = dtool_protocol::Command::no_op();
    let scope = Scope { context: None };

    let (result_id, result) = workspace.run(scope, command);

    assert_eq!(result_id, Some(id_b));
    assert!(matches!(result, Ok(_)));
}

#[test]
fn test_run_with_scope_of_b_goes_to_b() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let id_a = workspace.new_document("A");
    let id_b = workspace.new_document("B");

    workspace.activate(id_a).unwrap();

    let command = dtool_protocol::Command::no_op();
    let scope = Scope { context: Some(id_b) };

    let (result_id, result) = workspace.run(scope, command);

    assert_eq!(result_id, Some(id_b));
    assert!(matches!(result, Ok(_)));
}

#[test]
fn test_unknown_id_gives_context_not_found_and_no_context_changes() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let id_a = workspace.new_document("A");
    let unknown_id = ContextId(9999);

    workspace.activate(id_a).unwrap();

    let command = dtool_protocol::Command::no_op();
    let scope = Scope { context: Some(unknown_id) };

    let (result_id, result) = workspace.run(scope, command);

    assert_eq!(result_id, None);
    assert!(matches!(result, Err(_)));
    
    if let Err(err) = result {
        assert_eq!(err.code, "CONTEXT_NOT_FOUND");
    }

    assert_eq!(workspace.active(), Some(id_a));
}

#[test]
fn test_no_tabs_gives_no_document() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let command = dtool_protocol::Command::no_op();
    let scope = Scope { context: None };

    let (result_id, result) = workspace.run(scope, command);

    assert_eq!(result_id, None);
    assert!(matches!(result, Err(_)));

    if let Err(err) = result {
        assert_eq!(err.code, "NO_DOCUMENT");
    }
}
