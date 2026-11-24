use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use super::selection_refine::{self, REFINE};

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == REFINE).then(|| session.has_document
        && session.document.pixel_selection.as_ref().is_some_and(|mask| !mask.is_empty()))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    (command.name == REFINE).then(|| selection_refine::run(session, command, true))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    (command.name == REFINE).then(|| selection_refine::run(session, command, false).map(|_| ()))
}
