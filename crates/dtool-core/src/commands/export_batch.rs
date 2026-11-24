use crate::workspace::{ContextId, Scope, SharedWorkspace, Workspace};
use dtool_protocol::{Command, CommandResult};
use std::sync::{Mutex, OnceLock};

const NAMES: &[&str] = &[
    "export.batch.prepare",
    "export.batch.validate",
    "export.batch.start",
    "export.batch.query",
    "export.batch.cancel",
];

pub type BatchCommandFn = fn(&Command) -> CommandResult;

static HANDLER: OnceLock<Mutex<Option<BatchCommandFn>>> = OnceLock::new();

pub fn register(handler: BatchCommandFn) {
    *HANDLER.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|poison| poison.into_inner()) = Some(handler);
}

pub fn is_ours(command: &Command) -> bool {
    NAMES.contains(&command.name.as_str())
}

fn dispatch(command: &Command) -> CommandResult {
    let handler = HANDLER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|guard| *guard);
    match handler {
        Some(handler) => handler(command),
        None => Err(dtool_protocol::CommandError::not_implemented()),
    }
}

pub fn run(workspace: &mut Workspace, scope: Scope, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if !is_ours(command) {
        return None;
    }
    Some((scope.context.or_else(|| workspace.active()), dispatch(command)))
}

pub fn run_detached(scope: Scope, command: &Command) -> (Option<ContextId>, CommandResult) {
    (scope.context, dispatch(command))
}

#[allow(dead_code)]
pub fn run_shared(_workspace: &SharedWorkspace, scope: Scope, command: &Command) -> (Option<ContextId>, CommandResult) {
    run_detached(scope, command)
}
