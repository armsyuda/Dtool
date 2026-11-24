use super::model::{CellId, TableNode};
use crate::{Command, CommandError, CommandResult, Document, ObjectId, ObjectKind, Session};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SET: &str = "table.selection.set";
pub const QUERY: &str = "table.selection.query";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSelection {
    pub table: Option<ObjectId>,
    pub cells: Vec<CellId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracks: Option<TrackSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackSelection {
    Rows(Vec<u32>),
    Columns(Vec<u32>),
}

fn table(document: &Document, id: ObjectId) -> Result<&TableNode, CommandError> {
    match document.get_object(id).map(|object| &object.kind) {
        Some(ObjectKind::Table(table)) => Ok(table),
        _ => Err(CommandError::not_found()),
    }
}

fn owner(table: &TableNode, mut id: CellId) -> Result<CellId, CommandError> {
    for _ in 0..=table.cells.len() {
        let cell = table.cells.iter().find(|cell| cell.id == id).ok_or_else(CommandError::not_found)?;
        match cell.merged_into { Some(parent) => id = parent, None => return Ok(id) }
    }
    Err(CommandError::invalid_params())
}

impl TableSelection {
    pub fn reconciled(&self, document: &Document) -> Self {
        let Some(id) = self.table else { return Self::default(); };
        let Ok(table) = table(document, id) else { return Self::default(); };
        let mut cells = Vec::new();
        for id in &self.cells {
            if let Ok(id) = owner(table, *id) {
                if !cells.contains(&id) { cells.push(id); }
            }
        }
        let mut tracks = self.tracks.clone();
        if let Some(active) = &mut tracks {
            let (indices, limit) = match active {
                TrackSelection::Rows(indices) => (indices, table.row_heights_pt.len()),
                TrackSelection::Columns(indices) => (indices, table.col_widths_pt.len()),
            };
            indices.retain(|index| (*index as usize) < limit);
        }
        Self { table: Some(id), cells, tracks }
    }

    pub fn track_indices(&self, table: &TableNode, rows: bool) -> Vec<u32> {
        match (&self.tracks, rows) {
            (Some(TrackSelection::Rows(indices)), true) | (Some(TrackSelection::Columns(indices)), false) => return indices.clone(),
            _ => {}
        }
        if self.cells.is_empty() { return Vec::new(); }
        let selected: std::collections::HashSet<_> = self.cells.iter().copied().collect();
        let count = if rows { table.row_heights_pt.len() } else { table.col_widths_pt.len() };
        let mut covered = vec![false; count];
        for cell in table.cells.iter().filter(|cell| selected.contains(&cell.id)) {
            let (start, span) = if rows { (cell.row, cell.row_span) } else { (cell.col, cell.col_span) };
            if let Some(tracks) = covered.get_mut(start as usize..start.saturating_add(span).min(count as u32) as usize) {
                tracks.fill(true);
            }
        }
        covered.into_iter().enumerate().filter_map(|(index, selected)| selected.then_some(index as u32)).collect()
    }
}

pub fn current(session: &Session) -> TableSelection {
    let selection = session.table_selection.reconciled(&session.document);
    if selection.table.is_some_and(|id| !session.selection.ids.contains(&id)) { TableSelection::default() } else { selection }
}

pub fn reconcile(session: &mut Session) {
    session.table_selection = current(session);
}

fn cell_id(value: &Value) -> Result<CellId, CommandError> {
    value.as_u64().or_else(|| value.as_str().and_then(|s| s.strip_prefix('c').unwrap_or(s).parse().ok())).map(CellId).ok_or_else(CommandError::invalid_params)
}

fn indices(params: &Value, key: &str, maximum: usize) -> Result<Vec<u32>, CommandError> {
    let values = params.get(key).and_then(Value::as_array).ok_or_else(CommandError::invalid_params)?;
    let mut indices: Vec<_> = values.iter().map(|value| {
        value.as_u64().filter(|n| *n < maximum as u64).map(|n| n as u32).ok_or_else(CommandError::invalid_params)
    }).collect::<Result<_, _>>()?;
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

fn run_inner(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    if command.name == QUERY {
        let selection = current(session);
        let mut result = json!({"table":selection.table.map(|id|id.0),"cells":selection.cells.iter().map(|id|format!("c{}",id.0)).collect::<Vec<_>>(),"generation":session.generation,"document_version":session.document.version});
        match selection.tracks {
            Some(TrackSelection::Rows(rows)) => result["rows"] = json!(rows),
            Some(TrackSelection::Columns(columns)) => result["columns"] = json!(columns),
            None => {}
        }
        return Ok(result);
    }
    let id = ObjectId(command.params.get("id").or_else(||command.params.get("object_id")).and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?);
    let table = table(&session.document, id)?;
    let mut tracks = None;
    let requested: Vec<CellId> = match command.params.get("mode").and_then(Value::as_str).unwrap_or("cells") {
        "cells" => command.params.get("cells").and_then(Value::as_array).ok_or_else(CommandError::invalid_params)?.iter().map(cell_id).collect::<Result<_,_>>()?,
        "rows" => {
            let rows = indices(&command.params,"rows",table.row_heights_pt.len())?;
            let cells = table.cells.iter().filter(|cell|rows.iter().any(|row|*row>=cell.row && *row<cell.row.saturating_add(cell.row_span))).map(|cell|cell.id).collect();
            tracks = Some(TrackSelection::Rows(rows));
            cells
        }
        "columns" => {
            let columns = indices(&command.params,"columns",table.col_widths_pt.len())?;
            let cells = table.cells.iter().filter(|cell|columns.iter().any(|column|*column>=cell.col && *column<cell.col.saturating_add(cell.col_span))).map(|cell|cell.id).collect();
            tracks = Some(TrackSelection::Columns(columns));
            cells
        }
        "all" => table.cells.iter().map(|cell|cell.id).collect(),
        _ => return Err(CommandError::invalid_params()),
    };
    let mut cells = Vec::new();
    for cell in requested {
        let cell = owner(table,cell)?;
        if !cells.contains(&cell) { cells.push(cell); }
    }
    let selected = TableSelection { table:Some(id), cells, tracks };
    let changed = session.table_selection != selected || session.selection.ids != vec![id];
    session.table_selection = selected;
    session.selection.ids = vec![id];
    if changed { session.generation += 1; }
    let mut result = run_inner(session,&Command::new(QUERY))?;
    result["changed"] = json!(changed);
    Ok(result)
}

pub fn can_execute(session:&Session,command:&Command)->Option<bool> {
    matches!(command.name.as_str(),SET|QUERY).then_some(session.has_document)
}

pub fn run(session:&mut Session,command:&Command)->Option<CommandResult> {
    matches!(command.name.as_str(),SET|QUERY).then(||run_inner(session,command))
}
