use crate::command::Session;
use crate::grid::{GridError, GridSetArgs};
use dtool_protocol::{Command, CommandError};

pub const GRID_SET: &str = "grid.set";
pub const GRID_QUERY: &str = "grid.query";
pub const GRID_QUICK_SET: &str = "grid.quick.set";

fn quick_args(params: &serde_json::Value) -> Result<GridSetArgs, GridError> {
    let mut params = if params.is_null() { serde_json::json!({}) } else { params.clone() };
    let object = params.as_object_mut().ok_or(GridError::InvalidParams)?;
    if let Some(preset) = object.remove("preset") {
        if object.contains_key("kind") { return Err(GridError::InvalidParams); }
        object.insert("kind".into(), preset);
    }
    crate::grid::QuickGrid::parse(&params).map(|quick| quick.to_grid_set_args())
}

fn grid_error(error: GridError) -> CommandError {
    CommandError { code: "INVALID_PARAMS".into(), message_key: error.message_key().into() }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        GRID_SET => Some(session.has_document && GridSetArgs::parse(&command.params).is_ok()),
        GRID_QUERY => Some(session.has_document && crate::command::check_no_params(&command.params).is_ok()),
        GRID_QUICK_SET => Some(session.has_document && quick_args(&command.params).is_ok()),
        _ => None,
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != GRID_SET { return None; }
    Some(GridSetArgs::parse(&command.params).and_then(|args| session.document.grid.set(&args)).map_err(grid_error))
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    if command.name == GRID_QUICK_SET {
        return Some(quick_args(&command.params).map_err(grid_error).and_then(|args| {
            let set = Command::with_params(GRID_SET, serde_json::to_value(args).map_err(|_| CommandError::invalid_params())?);
            run(session, &set).unwrap_or_else(|| Err(CommandError::invalid_params()))
        }));
    }
    if !matches!(command.name.as_str(), GRID_SET | GRID_QUERY) { return None; }
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        if command.name == GRID_SET {
            let args = GridSetArgs::parse(&command.params).map_err(grid_error)?;
            let before = session.document.grid.clone();
            session.document.grid.set(&args).map_err(grid_error)?;
            if session.document.grid != before {
                let undo = Command::with_params(GRID_SET, serde_json::to_value(GridSetArgs::from_grid(&before)).unwrap_or_default());
                let redo = Command::with_params(GRID_SET, serde_json::to_value(GridSetArgs::from_grid(&session.document.grid)).unwrap_or_default());
                session.undo.push(crate::undo::Change { name: GRID_SET.into(), undo, redo });
                session.generation += 1;
                session.document.version += 1;
            }
        } else {
            crate::command::check_no_params(&command.params)?;
        }
        Ok(serde_json::json!({ "grid": session.document.grid, "doc_version": session.generation }))
    })())
}
