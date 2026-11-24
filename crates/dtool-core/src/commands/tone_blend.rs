use super::arrange;
use crate::tone_blend::ToneBlend;
use crate::{Command, CommandError, CommandResult, ObjectId, Session};
use serde_json::{json, Value};

pub const CREATE: &str = "toneblend.create";
pub const UPDATE: &str = "toneblend.update";
pub const QUERY: &str = "toneblend.query";

pub fn target_group(session: &Session) -> Option<ObjectId> {
    let [id] = session.selection.ids.as_slice() else { return None };
    session.document.get_object(*id)?.tone_blend.map(|_| *id)
}

fn locked(session: &Session, id: ObjectId) -> bool {
    let mut current = Some(id);
    for _ in 0..session.document.objects.len() {
        let Some(object) = current.and_then(|id| session.document.get_object(id)) else { return false; };
        if object.locked { return true; }
        current = object.parent_id;
    }
    current.is_some()
}

fn parameters(session: &Session, command: &Command) -> Result<Vec<ObjectId>, CommandError> {
    let ids = arrange::target_ids(session, command)?;
    let parent = session.document.get_object(ids[0]).and_then(|object| object.parent_id);
    let page = crate::data_merge::layout::object_page_index(&session.document, ids[0]);
    for &id in &ids {
        let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
        if object.parent_id != parent || crate::artboard::is_artboard(object)
            || crate::data_merge::layout::object_page_index(&session.document, id) != page
            || crate::clip_mask::is_mask(&session.document, object) || crate::tone::attached(&session.document, object)
        {
            return Err(CommandError::invalid_params());
        }
        if locked(session, id) {
            return Err(CommandError::new("LOCKED"));
        }
    }
    Ok(ids)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        CREATE => Some(session.has_document && parameters(session, command).is_ok()),
        UPDATE | QUERY => Some(session.has_document),
        _ => None,
    }
}

fn settings(command: &Command, initial: ToneBlend) -> Result<ToneBlend, CommandError> {
    let mut config = serde_json::to_value(initial).map_err(|_| CommandError::invalid_params())?;
    let key_values = command.params.get("settings");
    if let Some(values) = key_values {
        for (key, value) in values.as_object().ok_or_else(CommandError::invalid_params)? {
            config[key] = value.clone();
        }
    }
    let tone: ToneBlend = serde_json::from_value(config).map_err(|_| CommandError::invalid_params())?;
    if !tone.valid() { return Err(CommandError::invalid_params()); }
    Ok(tone)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        CREATE | UPDATE | QUERY => Some(execute(session, command)),
        _ => None,
    }
}

fn execute(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    if command.name == CREATE {
        let ids = parameters(session, command)?;
        let tone = settings(command, ToneBlend::default())?;
        let name = command.params.get("name").and_then(Value::as_str).map(str::trim)
            .filter(|name| !name.is_empty()).unwrap_or_else(|| dtool_strings::text("layer.kind.tone_blend"));
        return arrange::create_group(session, CREATE, &ids, name, Some(tone));
    }

    let id = command.params.get("id").map(|value| value.as_u64().map(ObjectId).ok_or_else(CommandError::invalid_params))
        .transpose()?.or_else(|| target_group(session)).ok_or_else(CommandError::invalid_params)?;
    let initial = session.document.get_object(id).ok_or_else(CommandError::not_found)?
        .tone_blend.ok_or_else(CommandError::invalid_params)?;
    if command.name == QUERY {
        return Ok(json!({"id": id.0, "settings": initial, "changes": []}));
    }
    if locked(session, id) {
        return Err(CommandError::new("LOCKED"));
    }
    let tone = settings(command, initial)?;
    let result = json!({"id": id.0, "settings": tone, "changes": []});
    if tone == initial { return Ok(result); }
    let before = arrange::snapshot(session);
    session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?.tone_blend = Some(tone);
    arrange::commit(session, UPDATE, before, json!({"id": id.0, "settings": tone}))
}
