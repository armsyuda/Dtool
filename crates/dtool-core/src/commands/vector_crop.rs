use crate::command::Session;
use crate::commands::arrange::{commit_snapshot, snapshot};
use crate::object::{ObjectId, VectorCrop};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const SET: &str = "crop.vector.set";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        SET => Some(plan(session, &command.params).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        SET => Some(set(session, &command.params)),
        _ => None,
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    (command.name == SET).then(|| {
        let (id, crop) = plan(session, &command.params)?;
        apply_crop(session, id, crop)
    })
}

fn target(session: &Session, params: &Value) -> Option<ObjectId> {
    match params.get("id") {
        Some(value) => value.as_u64().map(ObjectId),
        None => session.selection.ids.first().copied(),
    }
}

pub fn parse_rect(value: &Value) -> Result<Option<VectorCrop>, CommandError> {
    match value {
        Value::Null => Ok(None),
        Value::Array(items) if items.len() == 4 => {
            let mut rect = [0.0f32; 4];
            for (slot, item) in rect.iter_mut().zip(items) {
                let v = item.as_f64().filter(|v| v.is_finite()).ok_or_else(CommandError::invalid_params)?;
                *slot = v as f32;
            }
            if rect.iter().any(|v| !v.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 {
                return Err(CommandError::invalid_params());
            }
            Ok(Some(VectorCrop { rect }))
        }
        _ => Err(CommandError::invalid_params()),
    }
}

fn plan(session: &Session, params: &Value) -> Result<(ObjectId, Option<VectorCrop>), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let id = target(session, params).ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if session.document.is_object_locked(object) {
        return Err(CommandError::new("LOCKED"));
    }
    let crop = parse_rect(params.get("rect").ok_or_else(CommandError::invalid_params)?)?;
    Ok((id, crop))
}

fn set(session: &mut Session, params: &Value) -> CommandResult {
    let (id, crop) = plan(session, params)?;
    let previous = session.document.get_object(id).and_then(|o| o.vector_crop);
    if previous == crop {
        return Ok(json!({ "id": id.0, "vector_crop": crop, "changed": false, "doc_version": session.generation }));
    }
    let before = snapshot(session);
    apply_crop(session, id, crop)?;
    commit_snapshot(session, SET, before, json!({ "id": id.0, "vector_crop": crop, "previous": previous, "changed": true }))
}

fn apply_crop(session: &mut Session, id: ObjectId, crop: Option<VectorCrop>) -> Result<(), CommandError> {
    let object = session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    object.vector_crop = crop;
    Ok(())
}

#[cfg(test)]
mod contracts {
    use super::*;
    use crate::document::{Document, Units};
    use crate::object::Object;

    #[test]
    fn set_clear_and_undo() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { size: [100.0, 50.0], ..Object::default() });
        assert!(session.run(&Command::with_params(SET, json!({ "id": id.0, "rect": [0.0, 0.0, 0.0, 5.0] }))).is_err());
        let reply = session.run(&Command::with_params(SET, json!({ "id": id.0, "rect": [5.0, 5.0, 20.0, 10.0] }))).unwrap();
        assert_eq!(reply["changed"], json!(true));
        assert_eq!(session.document.get_object(id).unwrap().vector_crop, Some(VectorCrop { rect: [5.0, 5.0, 20.0, 10.0] }));
        let same = session.run(&Command::with_params(SET, json!({ "id": id.0, "rect": [5.0, 5.0, 20.0, 10.0] }))).unwrap();
        assert_eq!(same["changed"], json!(false));
        session.run(&Command::with_params(SET, json!({ "id": id.0, "rect": null }))).unwrap();
        assert_eq!(session.document.get_object(id).unwrap().vector_crop, None);
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(session.document.get_object(id).unwrap().vector_crop.is_some());
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(session.document.get_object(id).unwrap().vector_crop.is_none());
    }
}
