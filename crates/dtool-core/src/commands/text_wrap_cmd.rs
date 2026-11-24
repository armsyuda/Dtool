use super::document_change;
use crate::command::Session;
use crate::document::Document;
use crate::text_wrap::{TextWrap, WrapTable, CLEAR, IGNORE, QUERY, SET};
use crate::ObjectId;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{Value, json};

fn target(session: &Session, params: &Value) -> Result<ObjectId, CommandError> {
    let id = ObjectId(params.get("id").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?);
    session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    Ok(id)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), SET | CLEAR | IGNORE | QUERY).then(|| session.has_document && target(session, &command.params).is_ok())
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    if !matches!(name, SET | CLEAR | IGNORE | QUERY) {
        return None;
    }
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        let id = target(session, &command.params)?;
        if name == QUERY {
            let mut value = session.document.text_wraps.query(id);
            value["doc_version"] = json!(session.generation);
            return Ok(value);
        }
        if session.document.get_object(id).is_some_and(|object| object.locked) {
            return Err(CommandError::new("LOCKED"));
        }
        let Some(table) = session.document.text_wraps.edited(name, &command.params)? else {
            return Ok(json!({"id": id.0, "doc_version": session.generation, "changes": []}));
        };
        let mut document = session.document.clone();
        document.text_wraps = table;
        let selection = session.selection.ids.clone();
        document_change::commit(session, name, document, selection, json!({"id": id.0}))
    })())
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    let name = command.name.as_str();
    if !matches!(name, SET | CLEAR | IGNORE) {
        return None;
    }
    Some((|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        let id = target(session, &command.params)?;
        if session.document.get_object(id).is_some_and(|object| object.locked) {
            return Err(CommandError::new("LOCKED"));
        }
        if let Some(table) = session.document.text_wraps.edited(name, &command.params)? {
            session.document.text_wraps = table;
        }
        Ok(())
    })())
}

pub fn edit(doc: &Document, table: &mut WrapTable, cmd: &Command) -> Result<Option<WrapTable>, CommandError> {
    target_in(doc, &cmd.params)?;
    if cmd.name == QUERY {
        return Ok(None);
    }
    Ok(table.edited(&cmd.name, &cmd.params)?.map(|next| std::mem::replace(table, next)))
}

fn target_in(doc: &Document, params: &Value) -> Result<ObjectId, CommandError> {
    let id = ObjectId(params.get("id").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?);
    doc.get_object(id).map(|object| object.id).ok_or_else(CommandError::not_found)
}

pub fn run_with(session: &mut Session, table: &mut WrapTable, cmd: &Command) -> Option<CommandResult> {
    if !matches!(cmd.name.as_str(), SET | CLEAR | IGNORE | QUERY) {
        return None;
    }
    Some(edit(&session.document, table, cmd).and_then(|before| {
        let id = target_in(&session.document, &cmd.params)?;
        if cmd.name == QUERY {
            return Ok(table.query(id));
        }
        Ok(json!({ "doc_version": session.generation, "changed": before.is_some(), "wrap": table.query(id) }))
    }))
}

pub fn frame_wrappers(table: &WrapTable, doc: &Document, frame: ObjectId) -> Vec<(TextWrap, [f32; 4])> {
    let Some(origin) = doc.get_object(frame).map(|object| object.pos) else { return Vec::new() };
    let candidates: Vec<(ObjectId, [f32; 4])> = doc
        .objects
        .iter()
        .filter(|object| object.id != frame)
        .map(|object| {
            let world = crate::geometry::object_world_bounds(object);
            (object.id, [world[0] - origin[0], world[1] - origin[1], world[2], world[3]])
        })
        .collect();
    table.wrappers_for(frame, &candidates)
}