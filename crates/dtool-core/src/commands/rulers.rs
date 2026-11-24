use crate::command::Session;
use crate::rulers::{RULER_ORIGIN_RESET, RULER_ORIGIN_SET, RULER_UNIT_SET};
use dtool_protocol::{Command, CommandError};

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if crate::rulers::is_ruler_command(&command.name) {
        Some(session.has_document && crate::rulers::params_valid(command))
    } else { None }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if !crate::rulers::is_ruler_command(&command.name) { return None; }
    let generation = session.generation;
    crate::rulers::run(&mut session.document.rulers, generation, command).map(|result| result.map(|_| ()))
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    if !crate::rulers::is_ruler_command(&command.name) { return None; }
    if !session.has_document { return Some(Err(CommandError::no_document())); }
    let before = session.document.rulers;
    let generation = session.generation;
    let outcome = crate::rulers::run(&mut session.document.rulers, generation, command)?;
    if outcome.is_err() { return Some(outcome); }
    let after = session.document.rulers;
    let undo = match command.name.as_str() {
        RULER_ORIGIN_SET | RULER_ORIGIN_RESET if before.origin != after.origin => Some(Command::with_params(RULER_ORIGIN_SET, serde_json::json!({ "origin": before.origin }))),
        RULER_UNIT_SET if before.units != after.units || before.meters != after.meters => Some(Command::with_params(RULER_UNIT_SET, serde_json::json!({ "units": before.unit_value(None) }))),
        _ => None,
    };
    if let Some(undo) = undo {
        session.undo.push(crate::undo::Change { name: command.name.clone(), undo, redo: command.clone() });
        session.generation += 1;
        session.document.version += 1;
        return Some(Ok(after.result(&command.name, session.generation)));
    }
    Some(outcome)
}
