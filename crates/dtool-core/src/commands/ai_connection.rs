use crate::workspace::{ContextId, Workspace};
use dtool_protocol::{Command, CommandError, CommandResult};

pub fn run_workspace(workspace: &Workspace, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    match command.name.as_str() {
        "app.ai_connection.query" => Some((workspace.active(), Ok(serde_json::json!({ "enabled": workspace.ai_connection().enabled() })))),
        // GUI-only switch (AiConnection::set_from_gui); catalog row is implemented=false.
        "app.ai_connection.set" => Some((workspace.active(), Err(CommandError::not_implemented()))),
        _ => None,
    }
}
