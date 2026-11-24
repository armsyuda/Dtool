use crate::command::{invalid_params_error, Session};
use crate::object::{Object, ObjectId};
use crate::selection::SelectOp;
use dtool_protocol::{Command, CommandError};
use serde::{Deserialize, Serialize};

pub const EDIT_DUPLICATE_DRAG: &str = "edit.duplicate_drag";
pub const EDIT_DUPLICATE_DRAG_UNDO: &str = "edit.duplicate_drag.undo";
pub const EDIT_DUPLICATE_DRAG_REDO: &str = "edit.duplicate_drag.redo";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditDuplicateDragArgs {
    pub offset: [f32; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateDragStep {
    #[serde(default)]
    pub objects: Vec<Object>,
    #[serde(default)]
    pub remove: Vec<u64>,
    pub selection: Vec<u64>,
}

pub fn apply_step(session: &mut Session, step: DuplicateDragStep) -> Result<(), CommandError> {
    if let Some(missing) = step.remove.iter().find(|id| session.document.get_object(ObjectId(**id)).is_none()) {
        return session.document.delete_object(ObjectId(*missing));
    }
    for id in &step.remove {
        if session.document.get_object(ObjectId(*id)).is_some() {
            session.document.delete_object(ObjectId(*id))?;
        }
    }
    for obj in step.objects {
        if obj.id.0 == 0 || session.document.get_object(obj.id).is_some() {
            return Err(invalid_params_error());
        }
        session.document.create_object(obj);
    }
    let ids: Vec<ObjectId> = step.selection.into_iter().map(ObjectId).collect();
    session.selection.set(ids, SelectOp::Replace);
    session.document.version += 1;
    Ok(())
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != EDIT_DUPLICATE_DRAG_UNDO && command.name != EDIT_DUPLICATE_DRAG_REDO {
        return None;
    }
    let step: DuplicateDragStep = match serde_json::from_value(command.params.clone()) {
        Ok(step) => step,
        Err(_) => return Some(Err(invalid_params_error())),
    };
    Some(apply_step(session, step))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if let Some(allowed) = crate::clipboard::can_execute(session, command) {
        return Some(allowed);
    }
    if command.name == EDIT_DUPLICATE_DRAG {
        Some(!session.selection.ids.is_empty())
    } else {
        None
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    if let Some(result) = crate::clipboard::run(session, command) {
        return Some(result);
    }
    if command.name == EDIT_DUPLICATE_DRAG {
        Some(duplicate_drag(session, command))
    } else {
        None
    }
}

pub fn duplicate_drag(session: &mut Session, command: &Command) -> Result<serde_json::Value, CommandError> {
    let args: EditDuplicateDragArgs =
        serde_json::from_value(command.params.clone()).map_err(|_| invalid_params_error())?;
    if !args.offset[0].is_finite() || !args.offset[1].is_finite() {
        return Err(invalid_params_error());
    }
    if args.offset[0] == 0.0 && args.offset[1] == 0.0 {
        return Err(invalid_params_error());
    }
    if session.selection.ids.is_empty() {
        return Err(invalid_params_error());
    }
    let mut reply = crate::clipboard::duplicate_selection(session, EDIT_DUPLICATE_DRAG, args.offset)?;
    if let Some(map) = reply.as_object_mut() {
        map.insert("offset".to_string(), serde_json::json!(args.offset));
    }
    Ok(reply)
}
