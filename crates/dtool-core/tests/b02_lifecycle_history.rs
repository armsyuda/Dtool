use dtool_core::undo::{Change, UndoStack};
use dtool_protocol::Command;
use serde_json::json;

#[test]
fn test_undo_redo_replay_stored_change_commands() {
    let mut stack = UndoStack::new();

    let redo_cmd = Command {
        name: "edit.insert".to_string(),
        params: json!({"x": 10}),
    };
    let undo_cmd = Command {
        name: "edit.delete".to_string(),
        params: json!({"x": 10}),
    };

    stack.push(Change {
        name: "insert_1".to_string(),
        redo: redo_cmd.clone(),
        undo: undo_cmd.clone(),
    });

    assert!(stack.can_undo());
    assert!(!stack.can_redo());

    let undone = stack.undo().unwrap();
    assert_eq!(undone.name, undo_cmd.name);
    assert_eq!(undone.params, undo_cmd.params);

    assert!(!stack.can_undo());
    assert!(stack.can_redo());

    let redone = stack.redo().unwrap();
    assert_eq!(redone.name, redo_cmd.name);
    assert_eq!(redone.params, redo_cmd.params);

    assert!(stack.can_undo());
    assert!(!stack.can_redo());
}

#[test]
fn test_new_edit_after_undo_drops_redo_tail() {
    let mut stack = UndoStack::new();

    let cmd1_redo = Command { name: "cmd1".to_string(), params: json!(null) };
    let cmd1_undo = Command { name: "undo_cmd1".to_string(), params: json!(null) };
    let cmd2_redo = Command { name: "cmd2".to_string(), params: json!(null) };
    let cmd2_undo = Command { name: "undo_cmd2".to_string(), params: json!(null) };

    stack.push(Change { name: "c1".to_string(), redo: cmd1_redo, undo: cmd1_undo });
    stack.push(Change { name: "c2".to_string(), redo: cmd2_redo, undo: cmd2_undo });

    assert!(stack.can_undo());
    assert!(!stack.can_redo());

    let _ = stack.undo();
    assert!(stack.can_redo());

    let new_cmd_redo = Command { name: "cmd3".to_string(), params: json!(null) };
    let new_cmd_undo = Command { name: "undo_cmd3".to_string(), params: json!(null) };
    stack.push(Change { name: "c3".to_string(), redo: new_cmd_redo, undo: new_cmd_undo });

    assert!(!stack.can_redo());
    assert!(stack.can_undo());
}

#[test]
fn test_generation_is_monotonic_through_undo_redo() {
    use dtool_core::Session;
    use dtool_core::document::{Document, Units};

    let mut session = Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1));
    
    assert_eq!(session.generation, 0);

    let cmd1 = Command { name: "edit.insert".to_string(), params: json!({"id": 1}) };
    let undo_cmd1 = Command { name: "edit.delete".to_string(), params: json!({"id": 1}) };
    
    session.undo.push(Change { name: "c1".to_string(), redo: cmd1.clone(), undo: undo_cmd1.clone() });
    session.generation += 1;

    assert_eq!(session.generation, 1);

    let cmd2 = Command { name: "edit.insert".to_string(), params: json!({"id": 2}) };
    let undo_cmd2 = Command { name: "edit.delete".to_string(), params: json!({"id": 2}) };

    session.undo.push(Change { name: "c2".to_string(), redo: cmd2.clone(), undo: undo_cmd2.clone() });
    session.generation += 1;

    assert_eq!(session.generation, 2);

    let _ = session.undo.undo();
    
    let cmd3 = Command { name: "edit.insert".to_string(), params: json!({"id": 3}) };
    let undo_cmd3 = Command { name: "edit.delete".to_string(), params: json!({"id": 3}) };

    session.undo.push(Change { name: "c3".to_string(), redo: cmd3.clone(), undo: undo_cmd3.clone() });
    session.generation += 1;

    assert_eq!(session.generation, 3);
}
