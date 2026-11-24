use crate::{Command, CommandError, CommandResult, Session};
use std::sync::OnceLock;

static ACTION: OnceLock<fn(&Command) -> CommandResult> = OnceLock::new();

pub const NAMES: &[&str] = &["ui.shortcuts.list", "ui.shortcut.set", "ui.shortcuts.reset", "ui.shortcuts.import", "ui.shortcuts.export", "ui.focus.query"];

pub fn register_action(action: fn(&Command) -> CommandResult) -> bool {
    ACTION.set(action).is_ok()
}

pub fn can_execute(_session: &Session, command: &Command) -> Option<bool> {
    NAMES.contains(&command.name.as_str()).then_some(ACTION.get().is_some())
}

pub fn run(_session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !NAMES.contains(&command.name.as_str()) {
        return None;
    }
    Some(ACTION.get().ok_or_else(|| CommandError::new("NOT_AVAILABLE")).and_then(|action| action(command)))
}