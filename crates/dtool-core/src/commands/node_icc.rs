use crate::command::Session;
use crate::object::{apply_node_icc_set, is_node_icc_set, ObjectId};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

fn target(session: &Session, params: &Value) -> Result<ObjectId, CommandError> {
    let raw = params.get("id").or_else(|| params.get("node")).ok_or_else(CommandError::invalid_params)?;
    let id = match raw {
        Value::Number(number) => number.as_u64().map(ObjectId),
        Value::String(text) => text.strip_prefix("node:").unwrap_or(text).parse::<u64>().ok().map(ObjectId),
        _ => None,
    }
    .ok_or_else(CommandError::invalid_params)?;
    session.document.get_object(id).map(|_| id).ok_or_else(CommandError::not_found)
}

fn previous(session: &Session, id: ObjectId) -> Value {
    let icc = session.document.get_object(id).and_then(|object| object.icc.clone());
    let profile = icc.as_ref().and_then(|icc| icc.profile.clone());
    let intent = icc.as_ref().and_then(|icc| icc.intent).map(|intent| format!("{intent:?}"));
    json!({ "id": id.0, "profile": profile, "intent": intent })
}

fn set(session: &mut Session, params: &Value) -> Result<ObjectId, CommandError> {
    let id = target(session, params)?;
    let mut object = session.document.get_object(id).cloned().ok_or_else(CommandError::not_found)?;
    apply_node_icc_set(&mut object, params)?;
    let slot = session.document.objects.iter_mut().find(|item| item.id == id).ok_or_else(CommandError::not_found)?;
    *slot = object;
    Ok(id)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !is_node_icc_set(&command.name) {
        return None;
    }
    Some(session.has_document && target(session, &command.params).is_ok())
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !is_node_icc_set(&command.name) {
        return None;
    }
    Some((|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        let id = target(session, &command.params)?;
        let undo = Command::with_params(&command.name, previous(session, id));
        set(session, &command.params)?;
        session.undo.push(crate::undo::Change { name: command.name.clone(), redo: command.clone(), undo });
        session.generation += 1;
        session.document.version += 1;
        let icc = session.document.get_object(id).and_then(|object| object.icc.clone());
        Ok(json!({ "id": id.0, "icc": icc, "doc_version": session.generation }))
    })())
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if !is_node_icc_set(&command.name) {
        return None;
    }
    Some(set(session, &command.params).map(|_| ()))
}