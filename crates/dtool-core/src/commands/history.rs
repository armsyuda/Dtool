use crate::command::Session;
use dtool_protocol::{Command, CommandError};

pub const HISTORY_UNDO: &str = "history.undo";
pub const HISTORY_REDO: &str = "history.redo";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if matches!(command.name.as_str(), HISTORY_UNDO | HISTORY_REDO) {
        Some(session.has_document && crate::command::check_no_params(&command.params).is_ok())
    } else { None }
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    let (name, available) = match command.name.as_str() {
        HISTORY_UNDO => ("edit.undo", session.undo.can_undo()),
        HISTORY_REDO => ("edit.redo", session.undo.can_redo()),
        _ => return None,
    };
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        crate::command::check_no_params(&command.params)?;
        if !available { return Ok(serde_json::json!({ "doc_version": session.generation, "changes": [] })); }
        session.run(&Command::with_params(name, command.params.clone()))
    })())
}
