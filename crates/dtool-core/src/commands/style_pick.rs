use crate::command::Session;
use crate::commands::arrange::{commit_snapshot, snapshot};
use crate::object::{Object, ObjectId};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const PICK: &str = "style.pick";
pub const APPLY: &str = "style.apply";
pub const PARTS: [&str; 4] = ["fill", "stroke", "paint_order", "opacity"];
pub const CREATE: &str = "style.create";
pub const DELETE: &str = "style.delete";
pub const RENAME: &str = "style.rename";
pub const LIST: &str = "style.list";
pub const STORE_KEY: &str = "dtool.object_styles";
pub const STORED_PARTS: [&str; 6] = ["fill", "stroke", "paint_order", "opacity", "effects", "blend_mode"];

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        PICK => Some(source(session, &command.params).is_some()),
        APPLY => Some(resolve_style(session, &command.params).is_ok() && !targets(session, &command.params).is_empty()),
        CREATE => Some(source(session, &command.params).is_some()),
        DELETE | RENAME => Some(stored_index(session, &command.params).is_some()),
        LIST => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        PICK => Some(pick(session, &command.params)),
        APPLY => Some(apply_style(session, &command.params)),
        CREATE => Some(with_undo(session, CREATE, &command.params, create_style)),
        DELETE => Some(with_undo(session, DELETE, &command.params, delete_style)),
        RENAME => Some(with_undo(session, RENAME, &command.params, rename_style)),
        LIST => Some(Ok(json!({ "styles": stored(session) }))),
        _ => None,
    }
}

pub fn style_of(object: &Object) -> Value {
    json!({
        "fill": object.fill,
        "stroke": object.stroke,
        "paint_order": object.paint_order,
        "opacity": object.opacity,
        "effects": object.effects,
        "blend_mode": object.blend_mode,
    })
}

fn id_of(value: &Value) -> Option<ObjectId> {
    value.as_u64().map(ObjectId)
}

fn source(session: &Session, params: &Value) -> Option<ObjectId> {
    let id = match params.get("id") {
        Some(value) => id_of(value)?,
        None => *session.selection.ids.first()?,
    };
    session.document.get_object(id).map(|object| object.id)
}

fn targets(session: &Session, params: &Value) -> Vec<ObjectId> {
    match params.get("ids").or_else(|| params.get("nodes")).and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(id_of).filter(|id| session.document.get_object(*id).is_some()).collect(),
        None => session.selection.ids.clone(),
    }
}

fn part_on(params: &Value, part: &str) -> bool {
    match params.get("parts") {
        Some(parts) => parts.get(part).and_then(Value::as_bool).unwrap_or(false),
        None => true,
    }
}

fn pick(session: &Session, params: &Value) -> CommandResult {
    let id = source(session, params).ok_or_else(CommandError::not_found)?;
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    Ok(json!({ "id": id.0, "style": style_of(object) }))
}

pub fn stored(session: &Session) -> Vec<Value> {
    session.document.export_defaults.get(STORE_KEY).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn stored_mut(session: &mut Session) -> &mut Vec<Value> {
    let defaults = &mut session.document.export_defaults;
    if !defaults.is_object() {
        *defaults = json!({});
    }
    let map = defaults.as_object_mut().expect("object");
    if !map.get(STORE_KEY).is_some_and(Value::is_array) {
        map.insert(STORE_KEY.to_string(), json!([]));
    }
    map.get_mut(STORE_KEY).and_then(Value::as_array_mut).expect("array")
}

fn stored_index(session: &Session, params: &Value) -> Option<usize> {
    let id = params.get("style_id").and_then(Value::as_u64)?;
    stored(session).iter().position(|entry| entry["id"].as_u64() == Some(id))
}

fn resolve_style(session: &Session, params: &Value) -> Result<Value, CommandError> {
    if let Some(style) = params.get("style").filter(|style| style.is_object()) {
        return Ok(style.clone());
    }
    if params.get("style_id").is_none() {
        return Err(CommandError::invalid_params());
    }
    let index = stored_index(session, params).ok_or_else(CommandError::not_found)?;
    Ok(stored(session)[index]["style"].clone())
}

fn name_param(params: &Value) -> Result<Option<String>, CommandError> {
    match params.get("name") {
        None => Ok(None),
        Some(value) => value.as_str().map(str::trim).filter(|name| !name.is_empty()).map(|name| Some(name.to_string())).ok_or_else(CommandError::invalid_params),
    }
}

fn category_param(params: &Value) -> Result<Option<String>, CommandError> {
    match params.get("category") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_str().map(|text| Some(text.trim().to_string()).filter(|text| !text.is_empty())).ok_or_else(CommandError::invalid_params),
    }
}

fn with_undo(session: &mut Session, name: &str, params: &Value, op: fn(&mut Session, &Value) -> CommandResult) -> CommandResult {
    let before = session.document.clone();
    let value = op(session, params)?;
    let after = session.document.clone();
    session.undo.push(crate::undo::Change {
        name: name.to_string(),
        undo: Command::with_params(crate::commands::feature_restore::FEATURE_RESTORE, json!({ "document": before })),
        redo: Command::with_params(crate::commands::feature_restore::FEATURE_RESTORE, json!({ "document": after })),
    });
    session.generation += 1;
    session.document.version += 1;
    let mut reply = value;
    reply["doc_version"] = json!(session.generation);
    Ok(reply)
}

fn create_style(session: &mut Session, params: &Value) -> CommandResult {
    let id = source(session, params).ok_or_else(CommandError::not_found)?;
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    let style = style_of(object);
    let next = stored(session).iter().filter_map(|entry| entry["id"].as_u64()).max().map_or(1, |max| max + 1);
    let name = name_param(params)?.unwrap_or_else(|| format!("Style {next}"));
    let mut entry = json!({ "id": next, "name": name, "style": style });
    if let Some(category) = category_param(params)? {
        entry["category"] = json!(category);
    }
    stored_mut(session).push(entry);
    Ok(json!({ "style_id": next, "name": name }))
}

fn delete_style(session: &mut Session, params: &Value) -> CommandResult {
    let index = stored_index(session, params).ok_or_else(CommandError::not_found)?;
    let removed = stored_mut(session).remove(index);
    Ok(json!({ "style_id": removed["id"] }))
}

fn rename_style(session: &mut Session, params: &Value) -> CommandResult {
    let index = stored_index(session, params).ok_or_else(CommandError::not_found)?;
    let name = name_param(params)?.ok_or_else(CommandError::invalid_params)?;
    let entry = &mut stored_mut(session)[index];
    entry["name"] = json!(name);
    Ok(json!({ "style_id": entry["id"], "name": name }))
}

fn apply_style(session: &mut Session, params: &Value) -> CommandResult {
    let style = resolve_style(session, params)?;
    let ids = targets(session, params);
    if ids.is_empty() {
        return Err(CommandError::new("NOTHING_SELECTED"));
    }
    let mut updated = Vec::with_capacity(ids.len());
    for id in &ids {
        let object = session.document.get_object(*id).ok_or_else(CommandError::not_found)?;
        let mut value = serde_json::to_value(object).map_err(|_| CommandError::invalid_params())?;
        for part in STORED_PARTS {
            if part_on(params, part) {
                if let Some(next) = style.get(part) {
                    value[part] = next.clone();
                }
            }
        }
        let next: Object = serde_json::from_value(value).map_err(|_| CommandError::invalid_params())?;
        updated.push(next);
    }
    let before = snapshot(session);
    let mut count = 0usize;
    for next in updated {
        if let Some(slot) = session.document.objects.iter_mut().find(|object| object.id == next.id) {
            if serde_json::to_value(&*slot).ok() != serde_json::to_value(&next).ok() {
                *slot = next;
                count += 1;
            }
        }
    }
    if count == 0 {
        return Ok(json!({ "applied_count": 0, "doc_version": session.generation }));
    }
    commit_snapshot(session, APPLY, before, json!({ "applied_count": count }))
}

#[cfg(test)]
mod contracts {
    use super::*;
    use crate::document::{Document, Units};

    #[test]
    fn pick_then_apply_opacity_is_one_undo_step() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let a = session.document.create_object(Object { opacity: 0.5, size: [10.0, 10.0], ..Object::default() });
        let b = session.document.create_object(Object { opacity: 1.0, size: [10.0, 10.0], ..Object::default() });
        let picked = session.run(&Command::with_params(PICK, json!({ "id": a.0 }))).unwrap();
        assert_eq!(picked["style"]["opacity"], json!(0.5));
        let generation = session.generation;
        let reply = session.run(&Command::with_params(APPLY, json!({ "ids": [b.0], "style": picked["style"], "parts": { "opacity": true } }))).unwrap();
        assert_eq!(reply["applied_count"], json!(1));
        assert_eq!(session.generation, generation + 1);
        assert_eq!(session.document.get_object(b).unwrap().opacity, 0.5);
        let again = session.run(&Command::with_params(APPLY, json!({ "ids": [b.0], "style": picked["style"] }))).unwrap();
        assert_eq!(again["applied_count"], json!(0));
        session.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(session.document.get_object(b).unwrap().opacity, 1.0);
        assert!(session.run(&Command::with_params(APPLY, json!({ "ids": [b.0], "style": { "opacity": "bad" } }))).is_err());
    }
}
