use serde::{Deserialize, Serialize};

pub mod catalog;

pub use catalog::{CommandCatalog, CommandSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandError {
    pub code: String,
    pub message_key: String,
}

impl CommandError {
    pub fn new(code: &str) -> Self {
        let lower_code = code.to_lowercase();
        Self {
            code: code.to_string(),
            message_key: format!("error.{}", lower_code),
        }
    }
}

pub type CommandResult = Result<serde_json::Value, CommandError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeName {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub doc_version: u64,
    pub changes: Vec<ChangeName>,
    pub payload: serde_json::Value,
}

pub trait CommandBus {
    fn run(&mut self, command: Command) -> CommandResult;
}

impl Command {
    pub const NO_OP_NAME: &'static str = "noop";
    pub const TABLE_CELL_FORMAT_SET: &'static str = "table.cell.format.set";

    pub fn no_op() -> Self {
        Self { name: Self::NO_OP_NAME.to_string(), params: serde_json::Value::Null }
    }

    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), params: serde_json::Value::Null }
    }

    pub fn with_params(name: &str, params: serde_json::Value) -> Self {
        Self { name: name.to_string(), params }
    }

    pub fn path_param(&self) -> Option<std::path::PathBuf> {
        self.params
            .get("path")
            .and_then(|v| v.as_str())
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
    }
}

impl CommandError {
    pub fn invalid_params() -> Self { Self::new("INVALID_PARAMS") }
    pub fn not_found() -> Self { Self::new("NOT_FOUND") }
    pub fn not_implemented() -> Self { Self::new("NOT_IMPLEMENTED") }
    pub fn context_not_found() -> Self { Self::new("CONTEXT_NOT_FOUND") }
    pub fn no_document() -> Self { Self::new("NO_DOCUMENT") }
    pub fn validation(code: &str) -> Self { Self::new(code) }
    pub fn out_of_range() -> Self { Self::new("OUT_OF_RANGE") }
    pub fn unauthorized() -> Self { Self::new("UNAUTHORIZED") }
    pub fn shutdown() -> Self { Self::new("SHUTDOWN") }
    pub fn ui_timeout() -> Self { Self::new("UI_TIMEOUT") }
    pub fn no_window() -> Self { Self::new("NO_WINDOW") }
    pub fn context_not_active() -> Self { Self::new("CONTEXT_NOT_ACTIVE") }
}
