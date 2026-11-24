use crate::command::Session;
use crate::object::ObjectId;
use crate::slice::{Slice, SliceId, SliceSource, create_linked_slice, create_manual_slice};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize)]
struct State { slices: Vec<Slice>, next_slice_id: u64 }
fn state(session: &Session) -> Value { json!({"slices": session.document.slices, "next_slice_id": session.document.next_slice_id}) }
fn bounds(value: &Value) -> Result<[f32; 4], CommandError> {
    let rect: [f32; 4] = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
    if rect.iter().any(|value| !value.is_finite()) || rect[0] >= rect[2] || rect[1] >= rect[3] { return Err(CommandError::invalid_params()); }
    Ok(rect)
}
fn id(command: &Command) -> Result<SliceId, CommandError> {
    command.params.get("id").and_then(Value::as_u64).map(SliceId).ok_or_else(CommandError::invalid_params)
}
fn geometry_update(command: &Command) -> Option<serde_json::Map<String, Value>> {
    if command.name != "node.update" { return None; }
    let fields = command.params.get("set").unwrap_or(&command.params).as_object()?;
    if !fields.contains_key("pos") && !fields.contains_key("size") { return None; }
    if fields.keys().any(|key| !matches!(key.as_str(), "pos" | "size" | "id" | "node")) { return None; }
    Some(fields.iter().filter(|(key, _)| matches!(key.as_str(), "pos" | "size")).map(|(key, value)| (key.clone(), value.clone())).collect())
}
pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if geometry_update(command).is_some() { return Some(session.has_document); }
    matches!(command.name.as_str(), "slice.create" | "slice.list" | "slice.update" | "slice.reset_auto").then_some(session.has_document)
}
pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != "slice.restore" { return None; }
    Some((|| {
        let restored: State = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
        let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        session.document.slices = restored.slices;
        session.document.next_slice_id = session.document.next_slice_id.max(restored.next_slice_id);
        crate::slice::refresh_auto_bounds(&mut session.document);
        session.document.version = version;
        Ok(())
    })())
}
pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if let Some(mut params) = geometry_update(command) {
        return Some((|| {
            if !session.has_document { return Err(CommandError::no_document()); }
            let id = command.params.get("id").or_else(|| command.params.get("node")).and_then(Value::as_u64)
                .or_else(|| (session.selection.ids.len() == 1).then(|| session.selection.ids[0].0)).ok_or_else(CommandError::invalid_params)?;
            let target = session.document.get_object(ObjectId(id)).ok_or_else(CommandError::not_found)?;
            if target.locked { return Err(CommandError::invalid_params()); }
            params.insert("id".into(), json!(id));
            let result = session.run_named("object.update", Value::Object(params))?;
            crate::slice::refresh_auto_bounds(&mut session.document);
            Ok(result)
        })());
    }

    can_execute(session, command)?;
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        if command.name == "slice.list" {
            let mut slices = session.document.slices.clone();
            for slice in &mut slices { slice.bounds = slice.resolved_bounds(&session.document)?; }
            return Ok(json!({"slices": slices, "doc_version": session.generation}));
        }
        let before = state(session);
        let mut slices = session.document.slices.clone();
        let mut next = session.document.next_slice_id;
        let target;
        let params = command.params.as_object().ok_or_else(CommandError::invalid_params)?;
        match command.name.as_str() {
            "slice.create" => {
                if params.keys().any(|key| !matches!(key.as_str(), "rect" | "source" | "name")) { return Err(CommandError::invalid_params()); }
                let high = slices.iter().map(|slice| slice.id.0).max().unwrap_or(0).checked_add(1).ok_or_else(CommandError::invalid_params)?;
                target = SliceId(next.max(high).max(1));
                next = target.0.checked_add(1).ok_or_else(CommandError::invalid_params)?;
                let name = match params.get("name") {
                    None => dtool_strings::format("slice.default_name", &[("id", &target.0.to_string())]),
                    Some(Value::String(name)) if !name.trim().is_empty() => name.clone(),
                    _ => return Err(CommandError::invalid_params()),
                };
                let slice = match (params.get("rect"), params.get("source")) {
                    (Some(value), None) => create_manual_slice(bounds(value)?, name, target),
                    (None, Some(value)) => {
                        let source = value.as_u64().map(ObjectId).ok_or_else(CommandError::invalid_params)?;
                        let mut slice = create_linked_slice(source, name, target);
                        slice.bounds = crate::slice::source_bounds(&session.document, source)?;
                        slice
                    },
                    _ => return Err(CommandError::invalid_params()),
                };
                slices.push(slice);
            },
            "slice.update" => {
                target = id(command)?;
                slices = crate::slice::updated_slices(&session.document, &command.params)?;
            },
            "slice.reset_auto" => {
                if params.keys().any(|key| key != "id") { return Err(CommandError::invalid_params()); }
                target = id(command)?;
                let slice = slices.iter_mut().find(|slice| slice.id == target).ok_or_else(CommandError::not_found)?;
                let SliceSource::Linked(source) = slice.source else { return Err(CommandError::invalid_params()); };
                slice.bounds = crate::slice::source_bounds(&session.document, source)?;
                slice.auto_bounds = true;
            },
            _ => return Err(CommandError::new("UNKNOWN_COMMAND")),
        }
        let generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        let after = json!({"slices": slices, "next_slice_id": next});
        session.undo.push(crate::undo::Change { name: command.name.clone(), undo: Command::with_params("slice.restore", before), redo: Command::with_params("slice.restore", after) });
        session.document.slices = slices;
        session.document.next_slice_id = next;
        session.document.version = version;
        session.generation = generation;
        Ok(json!({"id": target.0, "doc_version": generation, "changes": [{"name": command.name}]}))
    })())
}
