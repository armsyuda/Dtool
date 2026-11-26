use crate::canvas_size::px_to_unit;
use crate::command::{ObjectRestoreArgs, execute_object_restore, transform_target_ids};
use crate::commands::{arrange, layer_meta};
use crate::object::{Object, ObjectId};
use crate::selection::SelectOp;
use crate::{Command, CommandError, CommandResult, Session};
use serde_json::{json, Value};

pub const SELECTION_NUDGE: &str = "selection.nudge";
pub const EDIT_SELECT_SAME: &str = "edit.select_same";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        SELECTION_NUDGE => Some(session.has_document && !nudge_targets(session, &command.params).is_empty()),
        EDIT_SELECT_SAME => Some(session.has_document && select_same_reference(session, &command.params).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        SELECTION_NUDGE => Some(nudge(session, command)),
        EDIT_SELECT_SAME => Some(select_same(session, command)),
        _ => None,
    }
}

fn nudge_targets(session: &Session, params: &Value) -> Vec<ObjectId> {
    let ids: Vec<ObjectId> = match params.get("ids").and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(Value::as_u64).map(ObjectId).collect(),
        None => session.selection.ids.clone(),
    };
    ids.into_iter().filter(|id| session.document.get_object(*id).is_some_and(|object| !object.locked)).collect()
}

fn delta(params: &Value, key: &str) -> Result<f32, CommandError> {
    let value = params.get(key).and_then(Value::as_f64).ok_or_else(CommandError::invalid_params)? as f32;
    if value.is_finite() { Ok(value) } else { Err(CommandError::invalid_params()) }
}

fn nudge(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let dx = delta(&command.params, "dx")?;
    let dy = delta(&command.params, "dy")?;
    let roots = nudge_targets(session, &command.params);
    if roots.is_empty() {
        return Err(CommandError::new("NOTHING_SELECTED"));
    }
    let units = session.document.units;
    let dpi = session.document.dpi;
    let step = [px_to_unit(dx, units, dpi), px_to_unit(dy, units, dpi)];
    if step.iter().any(|v| !v.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    let targets = transform_target_ids(&session.document, &roots);
    let before = session.snapshot_objects(&targets);
    let mut after = before.clone();
    for object in &mut after {
        object.pos[0] += step[0];
        object.pos[1] += step[1];
        if object.pos.iter().any(|v| !v.is_finite()) {
            return Err(CommandError::invalid_params());
        }
    }
    let after = ObjectRestoreArgs { objects: after };
    execute_object_restore(&mut session.document, &after)?;
    let redo = Command::with_params("object.restore", json!(after));
    session.push_restore_undo(&command.name, redo, before);
    Ok(json!({
        "doc_version": session.generation,
        "changes": [{ "name": command.name }],
        "ids": targets.iter().map(|id| id.0).collect::<Vec<u64>>(),
        "delta": step
    }))
}

fn same(attribute: &str, reference: &Object, object: &Object) -> Result<bool, CommandError> {
    Ok(match attribute {
        "FillColour" => object.fill == reference.fill,
        "StrokeColour" => object.stroke.paint == reference.stroke.paint,
        "StrokeWeight" => object.stroke.width == reference.stroke.width,
        "Width" => object.size[0] == reference.size[0],
        "Height" => object.size[1] == reference.size[1],
        "Rotation" => object.rotation_deg == reference.rotation_deg,
        "Name" => object.name == reference.name,
        _ => return Err(CommandError::invalid_params()),
    })
}

fn select_same_reference<'a>(session: &'a Session, params: &Value) -> Result<&'a Object, CommandError> {
    let id = match params.get("reference") {
        Some(id) => ObjectId(id.as_u64().ok_or_else(CommandError::invalid_params)?),
        None => *session.selection.ids.first().ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?,
    };
    let reference = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    let attribute = params["attribute"].as_str();
    if (params["layer_panel"] == true || attribute == Some("TagColour")) && (!reference.pickable || reference.locked) {
        return Err(CommandError::new("CANNOT_EXECUTE"));
    }
    match attribute {
        Some("TagColour") if layer_meta::effective_tag_colour(&session.document, id).is_none() =>
            Err(CommandError::new("CANNOT_EXECUTE")),
        Some("Name") if params["layer_panel"] == true && layer_name(&session.document, reference).is_none() =>
            Err(CommandError::new("CANNOT_EXECUTE")),
        _ => Ok(reference),
    }
}

fn layer_name<'a>(document: &crate::document::Document, object: &'a Object) -> Option<std::borrow::Cow<'a, str>> {
    if layer_meta::has_default_name(&object.name) {
        layer_meta::text_layer_name(document, object).map(std::borrow::Cow::Owned)
    } else {
        Some(std::borrow::Cow::Borrowed(&object.name))
    }
}

fn select_same(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let attribute = command.params["attribute"].as_str().ok_or_else(CommandError::invalid_params)?;
    let reference = select_same_reference(session, &command.params)?;
    let reference_id = reference.id;
    let document = &session.document;
    let by_tag = attribute == "TagColour";
    let scoped = command.params["layer_panel"] == true || by_tag;
    let name = (scoped && attribute == "Name").then(|| layer_name(document, reference)).flatten();
    let tags = by_tag.then(|| layer_meta::effective_tag_colours(document));
    let reference_tag = tags.as_ref().and_then(|tags| tags.get(&reference_id)).copied();
    let reference_colour = reference_tag.and_then(crate::paint::paint_from_legacy_str);
    let tree_command = arrange::layer_panel_tree_command(session, false);
    let scope = if scoped && (tree_command.params.get("page_id").is_some() || tree_command.params.get("master_id").is_some()) {
        let tree = arrange::tree_query(session, &tree_command)?;
        let rows = tree["nodes"].as_array().ok_or_else(|| CommandError::new("INTERNAL_ERROR"))?;
        Some(rows.iter().filter_map(|row| row["id"].as_u64().map(ObjectId)).collect::<std::collections::HashSet<_>>())
    } else {
        None
    };
    if scope.as_ref().is_some_and(|ids| !ids.contains(&reference_id)) {
        return Err(CommandError::new("CANNOT_EXECUTE"));
    }
    let mut ids = Vec::new();
    for object in &document.objects {
        if !object.pickable || object.locked || scope.as_ref().is_some_and(|ids| !ids.contains(&object.id)) {
            continue;
        }
        let matched = if let Some(tags) = &tags {
            tags.get(&object.id).is_some_and(|colour| {
                reference_colour.is_some()
                    && (reference_tag.is_some_and(|tag| tag.eq_ignore_ascii_case(colour))
                        || crate::paint::paint_from_legacy_str(colour) == reference_colour)
            })
        } else if name.is_some() {
            layer_name(document, object) == name
        } else {
            same(attribute, reference, object)?
        };
        if matched {
            ids.push(object.id);
        }
    }
    if !ids.contains(&reference_id) {
        ids.insert(0, reference_id);
    }
    let old: Vec<u64> = session.selection.ids.iter().map(|id| id.0).collect();
    session.selection.set(ids, SelectOp::Replace);
    let new: Vec<u64> = session.selection.ids.iter().map(|id| id.0).collect();
    session.undo.push(crate::undo::Change {
        name: command.name.clone(),
        undo: Command::with_params("selection.set", json!({ "ids": old, "op": SelectOp::Replace })),
        redo: Command::with_params("selection.set", json!({ "ids": new, "op": SelectOp::Replace })),
    });
    session.generation += 1;
    Ok(json!({
        "doc_version": session.generation,
        "changes": [{ "name": command.name }],
        "ids": new
    }))
}
