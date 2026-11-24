use crate::command::Session;
use crate::object::ObjectId;
use crate::undo::Change;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const NODE_ISOLATE: &str = "node.isolate";
pub const NODE_ISOLATE_END: &str = "node.isolate.end";
pub const NODE_ISOLATE_RESTORE: &str = "node.isolate.restore";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        NODE_ISOLATE => Some(session.has_document && isolate_target(session, command).is_some()),
        NODE_ISOLATE_END => Some(session.has_document && session.view.isolate_target().is_some()),
        NODE_ISOLATE_RESTORE => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        NODE_ISOLATE => Some(enter(session, command)),
        NODE_ISOLATE_END => Some(end(session)),
        _ => None,
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != NODE_ISOLATE_RESTORE {
        return None;
    }
    session.view.set_isolate_target(target_from_params(&command.params));
    Some(Ok(()))
}

fn isolate_target(session: &Session, command: &Command) -> Option<ObjectId> {
    let ids = super::arrange::listed_ids(command).ok().flatten().unwrap_or_else(|| session.selection.ids.clone());
    let id = ids.first().copied()?;
    session.document.get_object(id).map(|object| object.id)
}

fn target_from_params(params: &Value) -> Option<ObjectId> {
    let value = params.get("target")?;
    if value.is_null() {
        return None;
    }
    value.as_u64().map(ObjectId)
}

fn commit(session: &mut Session, name: &str, before: Option<ObjectId>, after: Option<ObjectId>, extra: Value) -> CommandResult {
    session.view.set_isolate_target(after);
    session.undo.push(Change {
        name: name.to_string(),
        redo: Command::with_params(NODE_ISOLATE_RESTORE, json!({ "target": after.map(|id| id.0) })),
        undo: Command::with_params(NODE_ISOLATE_RESTORE, json!({ "target": before.map(|id| id.0) })),
    });
    session.generation += 1;
    let mut reply = json!({
        "doc_version": session.generation,
        "changes": [{ "name": name }]
    });
    if let (Some(target), Some(source)) = (reply.as_object_mut(), extra.as_object()) {
        target.extend(source.clone());
    }
    Ok(reply)
}

fn enter(session: &mut Session, command: &Command) -> CommandResult {
    let target = isolate_target(session, command).ok_or_else(CommandError::invalid_params)?;
    let before = session.view.isolate_target();
    commit(session, NODE_ISOLATE, before, Some(target), json!({
        "id": target.0,
        "object_id": target.0,
        "target": target.0
    }))
}

fn end(session: &mut Session) -> CommandResult {
    let before = session.view.isolate_target().ok_or_else(CommandError::invalid_params)?;
    commit(session, NODE_ISOLATE_END, Some(before), None, json!({ "target": Value::Null }))
}
