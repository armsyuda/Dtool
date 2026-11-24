use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};

pub const TOOLS: &[&str] = crate::context_toolbar::TOOL_IDS;

fn ours(command: &Command) -> bool {
    matches!(command.name.as_str(), "ui.tool.set" | "ui.tool.query" | "ui.tools.list")
}

fn tool_param(command: &Command) -> Result<&str, CommandError> {
    let id = command.params.get("id").and_then(|v| v.as_str()).ok_or_else(CommandError::invalid_params)?;
    if crate::context_toolbar::canonical_tool(id).is_some() { Ok(id) } else { Err(CommandError::invalid_params()) }
}

pub fn can_execute(_session: &Session, command: &Command) -> Option<bool> {
    if !ours(command) {
        return None;
    }
    if command.name == "ui.tool.set" {
        return Some(tool_param(command).is_ok());
    }
    Some(true)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !ours(command) {
        return None;
    }
    Some(match command.name.as_str() {
        "ui.tool.set" => tool_param(command).map(|id| {
            session.active_tool = crate::context_toolbar::active_id(id);
            serde_json::json!({ "id": session.active_tool })
        }),
        "ui.tool.query" => Ok(serde_json::json!({ "id": session.active_tool })),
        _ => Ok(serde_json::json!({ "tools": TOOLS })),
    })
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != "ui.tool.set" {
        return None;
    }
    Some(tool_param(command).map(|id| session.active_tool = crate::context_toolbar::active_id(id)))
}
