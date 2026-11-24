use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    crate::longdoc::can_execute(session, command)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    crate::longdoc::run(session, command)
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    crate::longdoc::apply(session, command)
}
