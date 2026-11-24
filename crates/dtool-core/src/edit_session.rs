use crate::command::Session;
use dtool_protocol::{Command, CommandError};

pub const NOT_PREVIEWABLE: &str = "NOT_PREVIEWABLE";

#[derive(Debug, Clone)]
pub struct EditSession {
    pub key: String,
    pub name: String,
    pub first_undo: Command,
    pub last_redo: Command,
    pub doc_generation_at_begin: u64,
    pub tool_at_begin: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditEnd {
    Commit,
    Cancel,
}

pub fn not_previewable() -> CommandError {
    CommandError { code: NOT_PREVIEWABLE.to_string(), message_key: "error.invalid_params".to_string() }
}

pub fn ends_edit(name: &str) -> bool {
    matches!(name, "edit.undo" | "edit.redo" | "ui.tool.set" | "ui.persona.set")
}

pub fn snapshot_for(session: &Session, command: &Command) -> Option<(Command, Command)> {
    match command.name.as_str() {
        crate::context_toolbar::OPTION_SET => {
            let raw = command.params.get("tool").and_then(|v| v.as_str()).unwrap_or(session.active_tool.as_str());
            let tool = crate::context_toolbar::canonical_tool(raw)?;
            let id = command.params.get("id").and_then(|v| v.as_str())?;
            let mut redo = command.params.clone();
            redo.as_object_mut()?.insert("tool".to_string(), serde_json::json!(tool));
            let undo = if tool == "tool.zoom" && id == "zoom.level" {
                let mut params = redo.get("params").cloned().unwrap_or_else(|| serde_json::json!({}));
                params.as_object_mut()?.insert("zoom".to_owned(), serde_json::json!(session.view.zoom));
                Command::with_params(crate::command::VIEW_ZOOM_SET, params)
            } else {
                let before = session.tool_options.value(tool, id)?;
                Command::with_params(crate::context_toolbar::OPTION_SET, serde_json::json!({ "tool": tool, "id": id, "value": before }))
            };
            Some((
                undo,
                Command::with_params(crate::context_toolbar::OPTION_SET, redo),
            ))
        }
        "view.rotate.set" => Some((
            Command::with_params("view.rotate.set", serde_json::json!({ "degrees": session.view.rotation_deg })),
            command.clone(),
        )),
        _ => None,
    }
}
