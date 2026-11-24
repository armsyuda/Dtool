use crate::command::Session;
use crate::object::ObjectId;
use crate::undo::Change;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const LAYER_STATE_SAVE: &str = "layer.state.save";
pub const LAYER_STATE_RESTORE: &str = "layer.state.restore";
pub const LAYER_STATE_DELETE: &str = "layer.state.delete";
pub const LAYER_STATE_RENAME: &str = "layer.state.rename";
pub const LAYER_STATE_LIST: &str = "layer.state.list";
pub const LAYER_STATE_STORE: &str = "layer.state.store";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerState {
    pub name: String,
    pub flags: Vec<(ObjectId, bool, bool)>,
}

pub fn states(session: &Session) -> Vec<LayerState> {
    session.document.layer_states.clone()
}

pub fn set_states(session: &mut Session, states: Vec<LayerState>) {
    session.document.layer_states = states;
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        LAYER_STATE_SAVE => Some(session.has_document && name_of(command).is_some()),
        LAYER_STATE_RESTORE | LAYER_STATE_DELETE | LAYER_STATE_RENAME => {
            Some(session.has_document && find_index(session, command).is_some())
        }
        LAYER_STATE_LIST | LAYER_STATE_STORE => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        LAYER_STATE_SAVE => Some(save(session, command)),
        LAYER_STATE_RESTORE => Some(restore_flags(session, command)),
        LAYER_STATE_DELETE => Some(delete(session, command)),
        LAYER_STATE_RENAME => Some(rename(session, command)),
        LAYER_STATE_LIST => Some(list(session)),
        _ => None,
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != LAYER_STATE_STORE {
        return None;
    }
    let states: Vec<LayerState> = match serde_json::from_value(command.params.get("states").cloned().unwrap_or(json!([]))) {
        Ok(states) => states,
        Err(_) => return Some(Err(CommandError::invalid_params())),
    };
    set_states(session, states);
    Some(Ok(()))
}

fn name_of(command: &Command) -> Option<String> {
    command
        .params
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn find_index(session: &Session, command: &Command) -> Option<usize> {
    let current = states(session);
    if let Some(name) = name_of(command) {
        return current.iter().position(|state| state.name == name);
    }
    command
        .params
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|index| {
            let index = index as usize;
            (index < current.len()).then_some(index)
        })
}

fn flags_of(session: &Session) -> Vec<(ObjectId, bool, bool)> {
    session
        .document
        .objects
        .iter()
        .map(|object| (object.id, object.pickable, object.locked))
        .collect()
}

fn list_payload(items: &[LayerState]) -> Value {
    json!({
        "states": items.iter().map(|state| json!({
            "name": state.name,
            "flags": state.flags.iter().map(|(id, visible, locked)| json!({
                "id": id.0,
                "visible": visible,
                "locked": locked
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })
}

fn commit_store(session: &mut Session, name: &str, before: Vec<LayerState>, after: Vec<LayerState>, extra: Value) -> CommandResult {
    set_states(session, after.clone());
    session.undo.push(Change {
        name: name.to_string(),
        redo: Command::with_params(LAYER_STATE_STORE, json!({ "states": after })),
        undo: Command::with_params(LAYER_STATE_STORE, json!({ "states": before })),
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

fn save(session: &mut Session, command: &Command) -> CommandResult {
    let name = name_of(command).ok_or_else(CommandError::invalid_params)?;
    let state = LayerState { name: name.clone(), flags: flags_of(session) };
    let before = states(session);
    let mut after = before.clone();
    if let Some(index) = after.iter().position(|item| item.name == name) {
        after[index] = state;
    } else {
        after.push(state);
    }
    let extra = json!({ "name": name, "states": list_payload(&after)["states"] });
    commit_store(session, LAYER_STATE_SAVE, before, after, extra)
}

fn delete(session: &mut Session, command: &Command) -> CommandResult {
    let index = find_index(session, command).ok_or_else(CommandError::not_found)?;
    let before = states(session);
    let name = before[index].name.clone();
    let mut after = before.clone();
    after.remove(index);
    commit_store(session, LAYER_STATE_DELETE, before, after, json!({ "name": name }))
}

fn rename(session: &mut Session, command: &Command) -> CommandResult {
    let index = find_index(session, command).ok_or_else(CommandError::not_found)?;
    let new_name = command
        .params
        .get("new_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(CommandError::invalid_params)?
        .to_string();
    let before = states(session);
    if before.iter().enumerate().any(|(item_index, item)| item_index != index && item.name == new_name) {
        return Err(CommandError::invalid_params());
    }
    let mut after = before.clone();
    let old_name = after[index].name.clone();
    after[index].name = new_name.clone();
    commit_store(session, LAYER_STATE_RENAME, before, after, json!({ "name": new_name, "old_name": old_name }))
}

fn list(session: &Session) -> CommandResult {
    Ok(list_payload(&states(session)))
}

fn restore_flags(session: &mut Session, command: &Command) -> CommandResult {
    let index = find_index(session, command).ok_or_else(CommandError::not_found)?;
    let state = states(session)[index].clone();
    let before = super::arrange::snapshot(session);
    for (id, visible, locked) in &state.flags {
        if let Some(object) = session.document.objects.iter_mut().find(|object| object.id == *id) {
            object.pickable = *visible;
            object.locked = *locked;
        }
    }
    super::arrange::commit(session, LAYER_STATE_RESTORE, before, json!({
        "name": state.name
    }))
}
