use dtool_core::data_merge::layout::{create_layout, default_layout, find_or_create_layout, update_layout};
use dtool_core::data_merge::{run, Corner, MergeLayout};
use dtool_core::{Command, CommandError, CommandResult, ObjectId, Session};
use serde_json::Value;

pub fn can_execute(cmd: &Command) -> bool {
    matches!(cmd.name.as_str(), "merge.layout.create" | "merge.layout.update" | "merge.run")
}

pub fn run(session: &mut Session, cmd: &Command) -> Result<CommandResult, CommandError> {
    match cmd.name.as_str() {
        "merge.layout.create" => layout_create(session, &cmd.params),
        "merge.layout.update" => layout_update(session, &cmd.params),
        "merge.run" => merge_run(session),
        _ => Err(CommandError::not_implemented()),
    }
}

fn object_id(params: &Value) -> Result<ObjectId, CommandError> {
    if let Some(value) = params.get("object_id") {
        if let Some(n) = value.as_u64() {
            return Ok(ObjectId(n));
        }
        if let Some(s) = value.as_str() {
            if let Ok(n) = s.parse::<u64>() {
                return Ok(ObjectId(n));
            }
        }
    }
    Err(CommandError::invalid_params())
}

fn apply_params(layout: &mut MergeLayout, params: &Value) {
    if let Some(rows) = params.get("rows").and_then(|v| v.as_u64()) {
        layout.rows = rows as u32;
    }
    if let Some(columns) = params.get("columns").and_then(|v| v.as_u64()) {
        layout.columns = columns as u32;
    }
    if let Some(gutter) = params.get("gutter").and_then(|v| v.as_array()) {
        if gutter.len() >= 2 {
            layout.gutter = [
                gutter[0].as_f64().unwrap_or(0.0) as f32,
                gutter[1].as_f64().unwrap_or(0.0) as f32,
            ];
        }
    }
    if let Some(cell) = params.get("cell").and_then(|v| v.as_array()) {
        if cell.len() >= 2 {
            layout.cell = [
                cell[0].as_f64().unwrap_or(0.0) as f32,
                cell[1].as_f64().unwrap_or(0.0) as f32,
            ];
        }
    }
    if let Some(preserve) = params.get("preserve_cell_size").and_then(|v| v.as_bool()) {
        layout.preserve_cell_size = preserve;
    }
    if let Some(offset) = params.get("record_offset").and_then(|v| v.as_u64()) {
        layout.record_offset = offset as u32;
    }
    if let Some(advance) = params.get("record_advance").and_then(|v| v.as_u64()) {
        layout.record_advance = advance as u32;
    }
    if let Some(origin) = params.get("origin").and_then(|v| v.as_str()) {
        layout.origin = match origin {
            "TopRight" | "top_right" => Corner::TopRight,
            "BottomLeft" | "bottom_left" => Corner::BottomLeft,
            "BottomRight" | "bottom_right" => Corner::BottomRight,
            _ => Corner::TopLeft,
        };
    }
    if let Some(rows_first) = params.get("rows_first").and_then(|v| v.as_bool()) {
        layout.rows_first = rows_first;
    }
}

fn layout_create(session: &mut Session, params: &Value) -> Result<CommandResult, CommandError> {
    let id = object_id(params)?;
    let doc = session.document_mut().ok_or_else(CommandError::no_document)?;
    if doc.get_object(id).is_none() {
        return Err(CommandError::not_found());
    }
    let mut layout = default_layout(id);
    apply_params(&mut layout, params);
    let index = create_layout(doc, layout);
    Ok(CommandResult::ok(serde_json::json!({ "index": index, "object_id": id.0 })))
}

fn layout_update(session: &mut Session, params: &Value) -> Result<CommandResult, CommandError> {
    let doc = session.document_mut().ok_or_else(CommandError::no_document)?;
    if let Some(index) = params.get("index").and_then(|v| v.as_u64()) {
        let index = index as usize;
        let Some(existing) = doc.merge_layouts.get(index).cloned() else {
            return Err(CommandError::not_found());
        };
        let mut layout = existing;
        if let Ok(id) = object_id(params) {
            if doc.get_object(id).is_none() {
                return Err(CommandError::not_found());
            }
            layout.object_id = id;
        }
        apply_params(&mut layout, params);
        if !update_layout(doc, index, layout) {
            return Err(CommandError::not_found());
        }
        return Ok(CommandResult::ok(serde_json::json!({ "index": index })));
    }
    let id = object_id(params)?;
    if doc.get_object(id).is_none() {
        return Err(CommandError::not_found());
    }
    let layout = find_or_create_layout(doc, id);
    apply_params(layout, params);
    Ok(CommandResult::ok(serde_json::json!({ "object_id": id.0 })))
}

fn merge_run(session: &mut Session) -> Result<CommandResult, CommandError> {
    let doc = session.document().cloned().ok_or_else(CommandError::no_document)?;
    let (out, warnings) = run::run_merge(&doc)?;
    if let Some(document) = session.document_mut() {
        *document = out;
    }
    let list: Vec<serde_json::Value> = warnings
        .into_iter()
        .map(|w| serde_json::json!({"row_index": w.row_index, "message": w.message}))
        .collect();
    Ok(CommandResult::ok(serde_json::json!({ "warnings": list })))
}

pub fn run_workspace(workspace: &mut dtool_core::Workspace) -> Result<CommandResult, CommandError> {
    let (id, warnings) = run::run(workspace)?;
    let list: Vec<serde_json::Value> = warnings
        .into_iter()
        .map(|w| serde_json::json!({"row_index": w.row_index, "message": w.message}))
        .collect();
    Ok(CommandResult::ok(serde_json::json!({ "context_id": id.0, "warnings": list })))
}
