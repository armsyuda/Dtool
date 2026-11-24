use super::{model::TableNode, selection};
use crate::{Command, CommandError, CommandResult, ObjectId, ObjectKind, Session};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{cmp::Ordering, collections::HashMap, ops::Range};

pub const ROWS: &str = "table.rows.sort";
pub const COLUMNS: &str = "table.columns.sort";
pub const OPTIONS_SET: &str = "table.sort_options.set";
pub const OPTIONS_QUERY: &str = "table.sort_options.query";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TableSortOptions {
    pub case_sensitive: bool,
    pub include_headers: bool,
    pub treat_numbers_as_text: bool,
}

impl TableSortOptions {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

pub(super) fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        OPTIONS_SET | OPTIONS_QUERY => Some(session.has_document),
        ROWS | COLUMNS => {
            Some(request(session, command).is_ok_and(|request| request.indices.len() > 1))
        }
        _ => None,
    }
}

pub(super) fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        ROWS | COLUMNS => Some(sort(session, command)),
        OPTIONS_SET | OPTIONS_QUERY => Some(options(session, command)),
        _ => None,
    }
}

fn options(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let before = session.document.table_sort_options;
    if command.name == OPTIONS_QUERY {
        return Ok(json!(before));
    }
    let mut patch = json!(before);
    for (key, value) in command
        .params
        .as_object()
        .ok_or_else(CommandError::invalid_params)?
    {
        patch[key] = value.clone();
    }
    let after = serde_json::from_value(patch).map_err(|_| CommandError::invalid_params())?;
    if before == after {
        return Ok(json!({"options": after, "changes": []}));
    }
    let mut doc = session.document.clone();
    doc.table_sort_options = after;
    crate::commands::document_change::commit(
        session,
        &command.name,
        doc,
        session.selection.ids.clone(),
        json!({"options": after}),
    )
}

struct Request<'a> {
    id: ObjectId,
    table: &'a TableNode,
    rows: bool,
    key: usize,
    descending: bool,
    indices: Vec<usize>,
}

fn request<'a>(session: &'a Session, command: &Command) -> Result<Request<'a>, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let params = &command.params;
    let active = selection::current(session);
    let id = match params
        .get("table")
        .or_else(|| params.get("id"))
        .or_else(|| params.get("object_id"))
    {
        Some(id) => ObjectId(id.as_u64().ok_or_else(CommandError::invalid_params)?),
        None => active
            .table
            .or_else(|| session.selection.ids.first().copied())
            .ok_or_else(CommandError::invalid_params)?,
    };
    let object = session
        .document
        .get_object(id)
        .ok_or_else(CommandError::not_found)?;
    if object.locked {
        return Err(CommandError::new("LOCKED"));
    }
    let ObjectKind::Table(table) = &object.kind else {
        return Err(CommandError::invalid_params());
    };
    let rows = command.name == ROWS;
    let (count, cross_count, key_field, scope_field) = if rows {
        (
            table.row_heights_pt.len(),
            table.col_widths_pt.len(),
            "column",
            "rows",
        )
    } else {
        (
            table.col_widths_pt.len(),
            table.row_heights_pt.len(),
            "row",
            "cols",
        )
    };
    let key = index(&params[key_field], cross_count)?;
    let descending = match params["order"].as_str() {
        Some("ascending") => false,
        Some("descending") => true,
        _ => return Err(CommandError::invalid_params()),
    };
    let mut indices = match params.get(scope_field) {
        Some(value) => value
            .as_array()
            .ok_or_else(CommandError::invalid_params)?
            .iter()
            .map(|value| index(value, count))
            .collect::<Result<Vec<_>, _>>()?,
        None if active.table == Some(id) => {
            let selected = active.track_indices(table, rows);
            if selected.len() > 1 {
                selected.into_iter().map(|index| index as usize).collect()
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    };
    if indices.is_empty() {
        indices.extend(0..count);
    }
    indices.sort_unstable();
    indices.dedup();
    if rows {
        let start = if session.document.table_sort_options.include_headers {
            0
        } else {
            usize::from(table.header_rows)
        };
        let end = count.saturating_sub(usize::from(table.footer_rows));
        indices.retain(|&row| (start..end).contains(&row));
    }
    Ok(Request {
        id,
        table,
        rows,
        key,
        descending,
        indices,
    })
}

fn index(value: &Value, count: usize) -> Result<usize, CommandError> {
    let index = value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(CommandError::invalid_params)?;
    if index < count {
        Ok(index)
    } else {
        Err(CommandError::out_of_range())
    }
}

fn axes(cell: &super::model::TableCell, rows: bool) -> (Range<usize>, Range<usize>) {
    let row = cell.row as usize..cell.row.saturating_add(cell.row_span) as usize;
    let col = cell.col as usize..cell.col.saturating_add(cell.col_span) as usize;
    if rows { (row, col) } else { (col, row) }
}

fn blocks(request: &Request<'_>, count: usize) -> Result<Vec<Range<usize>>, CommandError> {
    let mut selected = vec![false; count];
    for &index in &request.indices {
        selected[index] = true;
    }
    let mut ends: Vec<_> = (1..=count).collect();
    for cell in request
        .table
        .cells
        .iter()
        .filter(|cell| cell.merged_into.is_none())
    {
        let (span, _) = axes(cell, request.rows);
        let tracks = selected
            .get(span.clone())
            .filter(|span| !span.is_empty())
            .ok_or_else(CommandError::out_of_range)?;
        if tracks.iter().any(|&selected| selected) {
            if tracks.iter().any(|&selected| !selected) {
                return Err(CommandError {
                    code: "CONFLICT".into(),
                    message_key: "error.table.sort_crosses_merge".into(),
                });
            }
            ends[span.start] = ends[span.start].max(span.end);
        }
    }
    let mut blocks = Vec::new();
    let mut first = 0;
    while first < count {
        if !selected[first] {
            first += 1;
            continue;
        }
        let mut end = ends[first];
        let mut current = first + 1;
        while current < end {
            end = end.max(ends[current]);
            current += 1;
        }
        blocks.push(first..end);
        first = end;
    }
    Ok(blocks)
}

fn permutation(
    session: &Session,
    request: &Request<'_>,
    count: usize,
) -> Result<Vec<usize>, CommandError> {
    let blocks = blocks(request, count)?;
    let stories: HashMap<_, _> = session
        .document
        .stories
        .iter()
        .map(|story| (story.id, story.text.trim()))
        .collect();
    let options = session.document.table_sort_options;
    let mut keys = vec![None; count];
    for cell in request
        .table
        .cells
        .iter()
        .filter(|cell| cell.merged_into.is_none())
    {
        let (span, cross) = axes(cell, request.rows);
        if !cross.contains(&request.key) {
            continue;
        }
        let key = stories.get(&cell.story).copied().unwrap_or("");
        for slot in keys.get_mut(span).ok_or_else(CommandError::out_of_range)? {
            *slot = Some(key);
        }
    }
    let mut sorted = Vec::with_capacity(blocks.len());
    for block in blocks {
        let text = keys[block.start].unwrap_or("");
        let number = (!options.treat_numbers_as_text)
            .then(|| text.parse::<f64>().ok().filter(|x| x.is_finite()))
            .flatten();
        let text = if options.case_sensitive {
            text.to_owned()
        } else {
            text.to_lowercase()
        };
        sorted.push((block, text, number));
    }
    sorted.sort_by(|(_, a, x), (_, b, y)| {
        let order = match (*x, *y) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.cmp(b),
        };
        if request.descending {
            order.reverse()
        } else {
            order
        }
    });
    let mut destination: Vec<_> = (0..count).collect();
    let mut slots = request.indices.iter().copied();
    for (block, _, _) in sorted {
        let first = slots.next().ok_or_else(CommandError::out_of_range)?;
        for (offset, old) in block.enumerate() {
            let next = if offset == 0 {
                first
            } else {
                slots.next().ok_or_else(CommandError::out_of_range)?
            };
            if next != first + offset {
                return Err(CommandError {
                    code: "CONFLICT".into(),
                    message_key: "error.table.sort_crosses_merge".into(),
                });
            }
            destination[old] = next;
        }
    }
    Ok(destination)
}

fn sort(session: &mut Session, command: &Command) -> CommandResult {
    let request = request(session, command)?;
    let Request {
        id, rows, table, ..
    } = &request;
    let (id, rows) = (*id, *rows);
    let tracks = if rows {
        &table.row_heights_pt
    } else {
        &table.col_widths_pt
    };
    if request.indices.len() < 2 {
        return Ok(json!({"id": id.0, "changes": []}));
    }
    let destination = permutation(session, &request, tracks.len())?;
    if destination.iter().copied().eq(0..tracks.len()) {
        return Ok(json!({"id": id.0, "changes": []}));
    }
    let mut after = session.document.clone();
    let object = after
        .find_object_mut(id.0)
        .ok_or_else(CommandError::not_found)?;
    let ObjectKind::Table(table) = &mut object.kind else {
        return Err(CommandError::invalid_params());
    };
    let new_tracks = if rows {
        &mut table.row_heights_pt
    } else {
        &mut table.col_widths_pt
    };
    for (old, &new) in destination.iter().enumerate() {
        new_tracks[new] = tracks[old];
    }
    for cell in &mut table.cells {
        let position = if rows { &mut cell.row } else { &mut cell.col };
        *position = *destination
            .get(*position as usize)
            .ok_or_else(CommandError::out_of_range)? as u32;
    }
    table
        .cells
        .sort_unstable_by_key(|cell| (cell.row, cell.col));
    let selected = selection::current(session);
    crate::commands::document_change::commit_table(
        session,
        &command.name,
        after,
        session.selection.ids.clone(),
        selected,
        json!({"id":id.0,"destination":destination}),
    )
}
