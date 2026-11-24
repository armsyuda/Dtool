use crate::{Command, CommandError, CommandResult, Session};

pub const FEATURE_RESTORE: &str = "core.feature.restore";

fn install(session: &mut Session, mut document: crate::document::Document) {
    document.version = session.document.version.max(document.version) + 1;
    document.keep_high_water_from(&session.document);
    let high_water: std::collections::HashMap<_, _> = session.document.objects.iter().filter_map(|object| {
        if let crate::ObjectKind::Table(table) = &object.kind { Some((object.id, table.alloc.high_water)) } else { None }
    }).collect();
    for object in &mut document.objects {
        if let (crate::ObjectKind::Table(table), Some(previous)) = (&mut object.kind, high_water.get(&object.id)) {
            table.alloc.high_water = table.alloc.high_water.max(*previous);
        }
    }
    session.document = document;
    session.selection.ids.retain(|id| session.document.get_object(*id).is_some());
}

fn snapshot(command: &Command) -> Result<crate::document::Document, CommandError> {
    serde_json::from_value(command.params.get("document").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == FEATURE_RESTORE).then(|| session.has_document && snapshot(command).is_ok())
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != FEATURE_RESTORE { return None; }
    Some(restore(session, command))
}

fn restore(session: &mut Session, command: &Command) -> Result<(), CommandError> {
    let document = snapshot(command)?;
    let selection: Option<Vec<crate::ObjectId>> = command.params.get("selection")
        .map(|value| serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params()))
        .transpose()?;
    let cells: Option<crate::table::selection::TableSelection> = command.params.get("table_selection")
        .map(|value| serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params()))
        .transpose()?;
    let vector_brush_options: Option<[Option<f64>; 2]> = command.params.get("vector_brush_options")
        .map(|value| serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params()))
        .transpose()?;
    let guides: Option<crate::guides::Guides> = command.params.get("guides")
        .map(|value| serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params()))
        .transpose()?;
    install(session, document);
    if let Some(guides) = guides {
        session.guides.replace_with(guides);
    }
    if let Some(values) = vector_brush_options {
        use super::brush::{BRUSH_NUMBERS, VECTOR_TOOL};
        session.tool_options.reset_tool(VECTOR_TOOL);
        for (id, value) in BRUSH_NUMBERS.into_iter().zip(values) {
            if let Some(value) = value {
                session.tool_options.set_number(VECTOR_TOOL, id, value);
            }
        }
    }
    if let Some(selection) = selection {
        session.selection.ids = selection.into_iter().filter(|id| session.document.get_object(*id).is_some()).collect();
    }
    if let Some(cells) = cells {
        session.table_selection = cells.reconciled(&session.document);
    }
    Ok(())
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name != FEATURE_RESTORE { return None; }
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        restore(session, command)?;
        session.generation += 1;
        Ok(serde_json::json!({"doc_version": session.generation, "changes": [{"name": FEATURE_RESTORE}]}))
    })())
}
