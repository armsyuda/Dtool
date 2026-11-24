use crate::command::Session;
use crate::colour::swatch::{self, Swatch, SwatchTarget};
use crate::object::ObjectId;
use crate::paint::{validate_paint, Hatch, Paint};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const FILL_HATCH_SET: &str = "fill.hatch.set";
pub const SWATCH_HATCH_ADD: &str = "swatch.hatch.add";

fn ours(name: &str) -> bool {
    name == FILL_HATCH_SET || name == SWATCH_HATCH_ADD
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !ours(&command.name) {
        return None;
    }
    Some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        FILL_HATCH_SET => Some(set_fill_hatch(session, command)),
        SWATCH_HATCH_ADD => Some(add_hatch_swatch(session, command)),
        _ => None,
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    match command.name.as_str() {
        FILL_HATCH_SET => Some(apply_fill_hatch(session, command).map(|_| ())),
        SWATCH_HATCH_ADD => Some(apply_add_hatch_swatch(session, command).map(|_| ())),
        _ => None,
    }
}

fn parse_hatch(params: &Value) -> Result<Hatch, CommandError> {
    let value = params.get("hatch").cloned().ok_or_else(CommandError::invalid_params)?;
    let hatch: Hatch = serde_json::from_value(value).map_err(|_| CommandError::invalid_params())?;
    if !validate_paint(&Paint::Hatch(hatch.clone())) {
        return Err(CommandError::invalid_params());
    }
    Ok(hatch)
}

fn parse_role(params: &Value) -> Result<SwatchTarget, CommandError> {
    match params.get("role").and_then(Value::as_str).map(|role| role.trim().to_ascii_lowercase()) {
        Some(role) if role == "fill" => Ok(SwatchTarget::Fill),
        Some(role) if role == "stroke" => Ok(SwatchTarget::Stroke),
        _ => Err(CommandError::invalid_params()),
    }
}

fn target_ids(session: &Session, params: &Value) -> Result<Vec<ObjectId>, CommandError> {
    if let Some(items) = params.get("ids").or_else(|| params.get("target")).or_else(|| params.get("targets")) {
        let mut ids = Vec::new();
        let values = match items {
            Value::Array(values) => values.clone(),
            other => vec![other.clone()],
        };
        for item in values {
            let id = match item {
                Value::Number(number) => number.as_u64(),
                Value::String(text) => text.trim_start_matches("node:").parse().ok(),
                _ => None,
            };
            ids.push(ObjectId(id.ok_or_else(CommandError::invalid_params)?));
        }
        if ids.is_empty() {
            return Err(CommandError::invalid_params());
        }
        return Ok(ids);
    }
    if session.selection.ids.is_empty() {
        return Err(CommandError::invalid_params());
    }
    Ok(session.selection.ids.clone())
}

fn apply_fill_hatch(session: &mut Session, command: &Command) -> Result<Vec<u64>, CommandError> {
    let role = parse_role(&command.params)?;
    let hatch = parse_hatch(&command.params)?;
    let ids = target_ids(session, &command.params)?;
    let paint = Paint::Hatch(hatch);
    let mut changed_ids = Vec::new();
    for object in &mut session.document.objects {
        if !ids.contains(&object.id) {
            continue;
        }
        if swatch::target_paint(object, role) == &paint {
            continue;
        }
        swatch::set_target_paint(object, role, paint.clone());
        changed_ids.push(object.id);
    }
    for id in &changed_ids {
        swatch::bind(&mut session.document, *id, role, None);
    }
    Ok(changed_ids.iter().map(|id| id.0).collect())
}

fn set_fill_hatch(session: &mut Session, command: &Command) -> CommandResult {
    let role = parse_role(&command.params)?;
    let hatch = parse_hatch(&command.params)?;
    let ids = target_ids(session, &command.params)?;
    let paint = Paint::Hatch(hatch);
    let mut old = Vec::new();
    let mut changed_ids = Vec::new();
    for object in &mut session.document.objects {
        if !ids.contains(&object.id) {
            continue;
        }
        if swatch::target_paint(object, role) == &paint {
            continue;
        }
        old.push(object.clone());
        swatch::set_target_paint(object, role, paint.clone());
        changed_ids.push(object.id);
    }
    for id in &changed_ids {
        swatch::bind(&mut session.document, *id, role, None);
    }
    let changed: Vec<u64> = changed_ids.iter().map(|id| id.0).collect();
    if !old.is_empty() {
        session.undo.push(crate::undo::Change {
            name: FILL_HATCH_SET.into(),
            undo: Command::with_params("object.restore", json!({ "objects": old })),
            redo: command.clone(),
        });
        session.generation += 1;
        session.document.version += 1;
    }
    Ok(json!({
        "doc_version": session.generation,
        "changes": [{ "name": FILL_HATCH_SET, "ids": changed, "role": role }]
    }))
}

fn apply_add_hatch_swatch(session: &mut Session, command: &Command) -> Result<u64, CommandError> {
    let hatch = parse_hatch(&command.params)?;
    let name = match command.params.get("name") {
        Some(Value::String(name)) if !name.trim().is_empty() => name.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    let palette = match command.params.get("palette") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(value)) => value.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    let id = swatch::next_id(&session.document);
    let mut added = Swatch::new(id, &name, hatch.colour);
    added.global = true;
    added.palette = palette.clone();
    added.paint = Some(Paint::Hatch(hatch));
    session.document.swatches.push(added);
    session.document.next_swatch_id = id + 1;
    if !palette.is_empty() && !session.document.palettes.iter().any(|item| item == &palette) {
        session.document.palettes.push(palette);
    }
    Ok(id)
}

fn swatch_snapshot(session: &Session) -> Result<Value, CommandError> {
    Ok(json!({
        "swatches": serde_json::to_value(&session.document.swatches).map_err(|_| CommandError::invalid_params())?,
        "next_swatch_id": session.document.next_swatch_id,
        "bindings": serde_json::to_value(&session.document.swatch_bindings).map_err(|_| CommandError::invalid_params())?,
        "objects": serde_json::to_value(&session.document.objects).map_err(|_| CommandError::invalid_params())?,
        "stories": serde_json::to_value(&session.document.stories).map_err(|_| CommandError::invalid_params())?
    }))
}

fn add_hatch_swatch(session: &mut Session, command: &Command) -> CommandResult {
    let before = swatch_snapshot(session)?;
    let id = apply_add_hatch_swatch(session, command)?;
    let name = command.params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let after = swatch_snapshot(session)?;
    session.undo.push(crate::undo::Change {
        name: SWATCH_HATCH_ADD.into(),
        undo: Command::with_params("swatch.restore", before),
        redo: Command::with_params("swatch.restore", after),
    });
    session.generation += 1;
    session.document.version += 1;
    Ok(json!({
        "doc_version": session.generation,
        "id": id,
        "name": name,
        "changes": [{ "name": SWATCH_HATCH_ADD, "id": id }]
    }))
}
