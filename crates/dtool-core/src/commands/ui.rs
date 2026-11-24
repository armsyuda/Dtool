use crate::command::Session;
use crate::object::ObjectId;
use crate::scene::is_drawn_in_view;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::sync::OnceLock;

pub const FEATURE_FINDER: &str = "help.feature_finder";
pub const UI_EXPORT_OPEN: &str = "ui.export.open";
pub const UI_EXPORT_SET: &str = "ui.export.set";
pub const UI_EXPORT_QUERY: &str = "ui.export.query";
pub const UI_EXPORT_PREVIEW: &str = "ui.export.preview";
pub const UI_EXPORT_CLOSE: &str = "ui.export.close";
pub const UI_EXPORT_COMMANDS: [&str; 5] = [UI_EXPORT_OPEN, UI_EXPORT_SET, UI_EXPORT_QUERY, UI_EXPORT_PREVIEW, UI_EXPORT_CLOSE];
pub const EXPORT_JOB_RUN: &str = "export.job.run";
pub const EXPORT_JOB_LIST: &str = "export.job.list";
pub const EXPORT_JOB_DELETE: &str = "export.job.delete";
/// Served by the registered export host (dtool-ui ExportDialog::run); schema uses the same provider.
pub const EXPORT_JOB_COMMANDS: [&str; 3] = [EXPORT_JOB_RUN, EXPORT_JOB_LIST, EXPORT_JOB_DELETE];
pub const CANVAS_DISPLAY_LIST: &str = "canvas.display_list";
pub const UI_ACTION_INVOKE: &str = "ui.action.invoke";

pub type ExportAction = fn(&Session, &Command) -> CommandResult;

pub type ExportActionFor = fn(Option<crate::workspace::ContextId>, &Session, &Command) -> CommandResult;

static EXPORT_ACTION: OnceLock<ExportAction> = OnceLock::new();

static EXPORT_ACTION_FOR: OnceLock<ExportActionFor> = OnceLock::new();

thread_local! {
    static PENDING_DELETE: RefCell<Option<Vec<ObjectId>>> = const { RefCell::new(None) };
}

pub fn register_export_action(action: ExportAction) -> bool {
    EXPORT_ACTION.set(action).is_ok()
}

pub fn register_export_action_for(action: ExportActionFor) -> bool {
    EXPORT_ACTION_FOR.set(action).is_ok()
}

pub fn export_action_registered() -> bool {
    EXPORT_ACTION.get().is_some() || EXPORT_ACTION_FOR.get().is_some()
}

pub fn is_export_command(name: &str) -> bool {
    UI_EXPORT_COMMANDS.contains(&name) || EXPORT_JOB_COMMANDS.contains(&name) || name == "export.options.schema"
}

fn export_needs_document(name: &str) -> bool {
    !matches!(name, UI_EXPORT_CLOSE | UI_EXPORT_QUERY | EXPORT_JOB_LIST | EXPORT_JOB_DELETE | "export.options.schema")
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name == FEATURE_FINDER {
        return Some(true);
    }
    if command.name == CANVAS_DISPLAY_LIST || command.name == UI_ACTION_INVOKE {
        return Some(session.has_document);
    }
    if is_export_command(&command.name) {
        return Some(export_action_registered() && (session.has_document || !export_needs_document(&command.name)));
    }
    None
}

fn export_action(session: &Session, command: &Command) -> CommandResult {
    if !export_action_registered() {
        return Err(CommandError::new("NOT_AVAILABLE"));
    }
    if export_needs_document(&command.name) && !session.has_document {
        return Err(CommandError::no_document());
    }
    if let Some(action) = EXPORT_ACTION_FOR.get() {
        return action(session.context, session, command);
    }
    let action = EXPORT_ACTION.get().ok_or_else(|| CommandError::new("NOT_AVAILABLE"))?;
    action(session, command)
}

fn parse_id(value: &Value) -> Option<ObjectId> {
    if let Some(id) = value.as_u64() {
        return Some(ObjectId(id));
    }
    let raw = value.as_str()?;
    let raw = raw.strip_prefix("node:").unwrap_or(raw);
    raw.parse::<u64>().ok().map(ObjectId)
}

fn action_ids(session: &Session, command: &Command) -> Result<Vec<ObjectId>, CommandError> {
    for key in ["nodes", "ids", "target"] {
        if let Some(value) = command.params.get(key) {
            let list = value.as_array().ok_or_else(CommandError::invalid_params)?;
            let mut ids = Vec::new();
            for item in list {
                let id = parse_id(item).ok_or_else(CommandError::invalid_params)?;
                if session.document.get_object(id).is_none() {
                    return Err(CommandError::not_found());
                }
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if !ids.is_empty() {
                return Ok(ids);
            }
        }
    }
    if !session.selection.ids.is_empty() {
        return Ok(session.selection.ids.clone());
    }
    Err(CommandError::invalid_params())
}

fn display_list(session: &Session) -> CommandResult {
    let mut items = Vec::new();
    for object in &session.document.objects {
        if !is_drawn_in_view(&session.document, &session.view, object.id) {
            continue;
        }
        items.push(json!({
            "object_id": object.id.0,
            "bounds": [object.pos[0], object.pos[1], object.size[0], object.size[1]]
        }));
    }
    Ok(json!({ "items": items }))
}

fn invoke_action(session: &mut Session, command: &Command) -> CommandResult {
    let action = command
        .params
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(CommandError::invalid_params)?;
    match action {
        "group" => {
            let mut params = command.params.clone();
            if let Some(object) = params.as_object_mut() {
                object.remove("action");
            }
            session.run(&Command::with_params("node.group", params))
        }
        "ungroup" => {
            let mut params = command.params.clone();
            if let Some(object) = params.as_object_mut() {
                object.remove("action");
            }
            session.run(&Command::with_params("node.ungroup", params))
        }
        "delete" => {
            let ids = action_ids(session, command)?;
            if crate::commands::arrange::any_locked(session, &ids) {
                PENDING_DELETE.with(|slot| *slot.borrow_mut() = Some(ids));
                return Err(crate::commands::arrange::locked_needs_confirm());
            }
            let nodes: Vec<u64> = ids.iter().map(|id| id.0).collect();
            session.run(&Command::with_params(
                "node.delete",
                json!({ "nodes": nodes, "confirm_locked": false }),
            ))
        }
        "delete_confirm_no" => {
            PENDING_DELETE.with(|slot| *slot.borrow_mut() = None);
            Ok(json!({}))
        }
        "delete_confirm_yes" => {
            let ids = PENDING_DELETE.with(|slot| slot.borrow_mut().take()).ok_or_else(CommandError::invalid_params)?;
            let nodes: Vec<u64> = ids.iter().map(|id| id.0).collect();
            session.run(&Command::with_params(
                "node.delete",
                json!({ "nodes": nodes, "confirm_locked": true }),
            ))
        }
        _ => Err(CommandError::invalid_params()),
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if is_export_command(&command.name) {
        return Some(export_action(session, command));
    }
    if command.name == CANVAS_DISPLAY_LIST {
        if !session.has_document {
            return Some(Err(CommandError::no_document()));
        }
        return Some(display_list(session));
    }
    if command.name == UI_ACTION_INVOKE {
        if !session.has_document {
            return Some(Err(CommandError::no_document()));
        }
        return Some(invoke_action(session, command));
    }
    if command.name != FEATURE_FINDER {
        return None;
    }
    let q = command.params.get("q").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    let hits: Vec<String> = session
        .catalog
        .names()
        .into_iter()
        .filter(|name| q.is_empty() || name.to_lowercase().contains(&q))
        .collect();
    Some(Ok(serde_json::json!({ "hits": hits })))
}

pub fn apply(_session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    (command.name == FEATURE_FINDER).then_some(Ok(()))
}
