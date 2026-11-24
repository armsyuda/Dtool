use crate::command::Session;
use crate::table::model::{CellId, CellFormat, CellIdAllocator, EdgeStyle, TableCell, TableEdgeMask as Edge, TableFrameFormat, TableNode};
use crate::table::selection::{self, TableSelection, TrackSelection};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const MAX_ROWS_COLS: u32 = 1000;

fn reserve_table_story_ids(document: &mut crate::document::Document) {
    let floor = document
        .objects
        .iter()
        .filter_map(|object| match &object.kind {
            crate::object::ObjectKind::Table(table) => table.cells.iter().map(|cell| cell.story.0).max(),
            _ => None,
        })
        .max()
        .map_or(0, |id| id + 1);
    let live = document.stories.iter().map(|story| story.id.0).max().unwrap_or(0) + 1;
    document.next_story_id = document.next_story_id.max(floor).max(live);
}

fn push_cell_story(stories: &mut Vec<crate::document::Story>, next_story_id: &mut u64, text: String) -> crate::document::StoryId {
    let id = *next_story_id;
    *next_story_id = id + 1;
    stories.push(crate::document::Story { id: crate::document::StoryId(id), text, ..Default::default() });
    crate::document::StoryId(id)
}

fn default_cell_format() -> CellFormat {
    CellFormat::default()
}

fn default_frame_format() -> TableFrameFormat {
    TableFrameFormat::default()
}

fn default_cell_mask() -> crate::table::model::CellFormatMask {
    crate::table::model::CellFormatMask::default()
}

fn default_frame_mask() -> crate::table::model::FrameFormatMask {
    crate::table::model::FrameFormatMask::default()
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if let Some(enabled) = super::merge::can_execute(session, command) { return Some(enabled); }
    if let Some(enabled) = super::sort::can_execute(session, command) { return Some(enabled); }
    match command.name.as_str() {
        "table.create" | "table.track.resize" | "table.row.insert" | "table.column.insert" | "table.row.delete" | "table.column.delete" => Some(session.has_document),
        "table.rows.distribute" => Some(track_target(session, command, TrackAxis::Row).is_ok_and(|(_, indices)| indices.len() > 1)),
        "table.columns.distribute" => Some(track_target(session, command, TrackAxis::Column).is_ok_and(|(_, indices)| indices.len() > 1)),
        "table.row.autofit" => Some(super::fit::available() && track_target(session, command, TrackAxis::Row).is_ok()),
        "table.column.autofit" => Some(super::fit::available() && track_target(session, command, TrackAxis::Column).is_ok()),
        "table.query" => Some(session.has_document && query_target(session, &command.params).is_ok()),
        Command::TABLE_CELL_FORMAT_SET | "table.format.set" => Some(session.has_document),
        _ => None,
    }
}

fn query_target(session: &Session, params: &serde_json::Value) -> Result<(crate::object::ObjectId, Option<CellId>), CommandError> {
    let obj = match params {
        serde_json::Value::Null => serde_json::Map::new(),
        serde_json::Value::Object(map) => map.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    if obj.keys().any(|k| !matches!(k.as_str(), "id" | "object_id" | "cell")) {
        return Err(CommandError::invalid_params());
    }
    let is_table = |id: crate::object::ObjectId| session.document.get_object(id).is_some_and(|o| matches!(o.kind, crate::object::ObjectKind::Table(_)));
    let id = match obj.get("id").or_else(|| obj.get("object_id")) {
        Some(v) => {
            let id = crate::object::ObjectId(v.as_u64().ok_or_else(CommandError::invalid_params)?);
            if !is_table(id) {
                return Err(CommandError::not_found());
            }
            id
        }
        None => session.selection.ids.iter().copied().find(|id| is_table(*id)).ok_or_else(CommandError::not_found)?,
    };
    let cell = match obj.get("cell") {
        Some(v) => Some(parse_cell_id(v).ok_or_else(CommandError::invalid_params)?),
        None => None,
    };
    Ok((id, cell))
}

fn handle_table_query(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let (id, cell) = query_target(session, &command.params)?;
    let mut out = crate::query::table_query_live(&session.document, id, cell);
    if let Some(cell) = cell {
        if out["cells"].as_array().is_some_and(|c| c.is_empty()) {
            return Err(CommandError::not_found());
        }
        out["cell"] = serde_json::json!(format!("c{}", cell.0));
    }
    out["id"] = serde_json::json!(id.0);
    out["generation"] = serde_json::json!(session.generation);
    out["document_version"] = serde_json::json!(session.document.version);
    Ok(out)
}

fn parse_pos(params: &serde_json::Value) -> Result<Option<[f32; 2]>, CommandError> {
    let Some(value) = params.get("pos") else { return Ok(None); };
    let arr = value.as_array().filter(|a| a.len() == 2).ok_or_else(CommandError::invalid_params)?;
    let x = arr[0].as_f64().ok_or_else(CommandError::invalid_params)? as f32;
    let y = arr[1].as_f64().ok_or_else(CommandError::invalid_params)? as f32;
    if !x.is_finite() || !y.is_finite() {
        return Err(CommandError::invalid_params());
    }
    Ok(Some([x, y]))
}

fn parse_insert(params: &serde_json::Value, pos: Option<[f32; 2]>) -> Result<bool, CommandError> {
    let flag = match params.get("insert") {
        None => false,
        Some(v) => v.as_bool().ok_or_else(CommandError::invalid_params)?,
    };
    Ok(pos.is_some() || flag)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if let Some(result) = super::merge::run(session, command) { return Some(result); }
    if let Some(result) = super::sort::run(session, command) { return Some(result); }
    if let Some(result) = crate::table::styles::run(session, command) { return Some(result); }
    match command.name.as_str() {
        "table.create" => Some(handle_table_create(session, command)),
        "table.track.resize" => Some(handle_track_resize(session, command)),
        "table.query" => Some(handle_table_query(session, command)),
        "table.row.insert" => Some(edit_track(session, command, TrackAxis::Row, true)),
        "table.row.delete" => Some(edit_track(session, command, TrackAxis::Row, false)),
        "table.column.insert" => Some(edit_track(session, command, TrackAxis::Column, true)),
        "table.column.delete" => Some(edit_track(session, command, TrackAxis::Column, false)),
        "table.rows.distribute" => Some(distribute(session, command, TrackAxis::Row)),
        "table.columns.distribute" => Some(distribute(session, command, TrackAxis::Column)),
        "table.row.autofit" => Some(autofit(session, command, TrackAxis::Row)),
        "table.column.autofit" => Some(autofit(session, command, TrackAxis::Column)),
        "table.from_tsv" => Some(handle_import(session, command)),
        "table.paste" => Some(handle_paste(session, command)),
        Command::TABLE_CELL_FORMAT_SET => Some(handle_cell_format_set(session, command)),
        "table.format.set" => Some(handle_frame_format_set(session, command)),
        _ => None,
    }
}

fn handle_table_create(session: &mut Session, command: &Command) -> CommandResult {
    let rows = command.params.get("rows").and_then(|v| v.as_u64()).unwrap_or(0);
    let cols = command.params.get("cols").and_then(|v| v.as_u64()).unwrap_or(0);

    if rows < 1 || cols < 1 || rows > u64::from(MAX_ROWS_COLS) || cols > u64::from(MAX_ROWS_COLS) {
        return Err(CommandError::out_of_range());
    }

    let (rows, cols) = (rows as u32, cols as u32);

    let bounds = command.params.get("bounds").and_then(|v| v.as_array());
    let (width, height) = if let Some(arr) = bounds {
        if arr.len() >= 2 {
            (arr[0].as_f64().unwrap_or(100.0) as f32, arr[1].as_f64().unwrap_or(100.0) as f32)
        } else {
            (100.0, 100.0)
        }
    } else {
        (100.0, 100.0)
    };

    if !session.has_document {
        return Err(CommandError::no_document());
    }
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Err(CommandError::invalid_params());
    }
    let pos = parse_pos(&command.params)?;
    let insert = parse_insert(&command.params, pos)?;
    let before = insert.then(|| session.document.clone());

    let col_width = width / cols as f32;
    let row_height = height / rows as f32;

    let header_rows = command.params.get("header_rows").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
    let footer_rows = command.params.get("footer_rows").and_then(|v| v.as_u64()).unwrap_or(0) as u16;

    reserve_table_story_ids(&mut session.document);
    let mut alloc = CellIdAllocator { high_water: 0 };
    let mut cells = Vec::new();
    let mut preview_story = session.document.next_story_id;
    let visible_grid = command.params.get("visible_grid").and_then(|value| value.as_bool()).unwrap_or(false)
        && session.document.default_table_format.is_none();
    let mut cell_format = default_cell_format();
    let mut cell_mask = default_cell_mask();
    if visible_grid {
        let edge = EdgeStyle {
            paint: crate::paint::Paint::Solid { colour: crate::paint::Colour::new(87, 87, 87, 255) },
            width_pt: 1.0,
            line_style: Default::default(),
        };
        cell_format.edges = std::array::from_fn(|_| Some(edge.clone()));
        cell_mask.0 |= super::resolve::CELL_EDGES.into_iter().fold(0, |mask, bit| mask | bit);
    }

    for r in 0..rows {
        for c in 0..cols {
            let id = alloc.next();
            let story_id = if insert {
                push_cell_story(&mut session.document.stories, &mut session.document.next_story_id, String::new()).0
            } else {
                preview_story += 1;
                preview_story - 1
            };
            cells.push(TableCell {
                id,
                row: r,
                col: c,
                row_span: 1,
                col_span: 1,
                merged_into: None,
                story: crate::document::StoryId(story_id),
                local: cell_format.clone(),
                mask: cell_mask,
            });
        }
    }

    let table = TableNode {
        alloc,
        col_widths_pt: vec![col_width; cols as usize],
        row_heights_pt: vec![row_height; rows as usize],
        cells,
        header_rows,
        footer_rows,
        format: None,
        frame_local: default_frame_format(),
        frame_mask: default_frame_mask(),
    };

    let Some(before) = before else {
        session.generation += 1;
        return Ok(serde_json::json!({ "table": table }));
    };
    let mut object = crate::object::Object::default();
    object.name = "Table".to_string();
    object.pos = pos.unwrap_or([0.0, 0.0]);
    object.size = [width, height];
    object.kind = crate::object::ObjectKind::Table(table.clone());
    let anchor = [object.pos[0] + width * 0.5, object.pos[1] + height * 0.5];
    crate::artboard::edit::assign_new_object(&session.document, &mut object, anchor);
    let id = session.document.create_object(object);
    session.selection.set(vec![id], crate::selection::SelectOp::Replace);
    let restore = |document: &crate::document::Document| Command {
        name: crate::commands::feature_restore::FEATURE_RESTORE.to_string(),
        params: serde_json::json!({ "document": document }),
    };
    session.undo.push(crate::undo::Change {
        name: "table.create".to_string(),
        redo: restore(&session.document),
        undo: restore(&before),
    });
    session.generation += 1;
    session.document.version += 1;
    Ok(serde_json::json!({
        "table": table,
        "object_id": id.0,
        "id": id.0,
        "ids": [id.0],
        "doc_version": session.generation,
        "changes": [{ "name": "table.create", "id": id.0 }]
    }))
}

fn handle_track_resize(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let params = &command.params;
    let id = crate::ObjectId(params.get("id").and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)?);
    let axis = match params.get("axis").and_then(|v| v.as_str()) {
        Some("column") => 0,
        Some("row") => 1,
        _ => return Err(CommandError::invalid_params()),
    };
    let parse_index = |value: &serde_json::Value| {
        value.as_u64().and_then(|value| usize::try_from(value).ok())
            .ok_or_else(CommandError::invalid_params)
    };
    let indices = match params.get("indices") {
        Some(values) if params.get("index").is_none() => values.as_array()
            .filter(|values| !values.is_empty())
            .ok_or_else(CommandError::invalid_params)?
            .iter().map(parse_index).collect::<Result<Vec<_>, _>>()?,
        None => vec![parse_index(&params["index"])?],
        Some(_) => return Err(CommandError::invalid_params()),
    };
    let size = params.get("size").and_then(|v| v.as_f64()).map(|v| v as f32)
        .filter(|v| v.is_finite() && *v > 0.0).ok_or_else(CommandError::invalid_params)?;
    let mut next = session.document.clone();
    let object = next.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let anchor = leading_corner(object);
    let crate::ObjectKind::Table(table) = &mut object.kind else { return Err(CommandError::invalid_params()); };
    let tracks = if axis == 0 { &mut table.col_widths_pt } else { &mut table.row_heights_pt };
    let mut changed = false;
    for &index in &indices {
        let track = tracks.get_mut(index).ok_or_else(CommandError::out_of_range)?;
        changed |= *track != size;
        *track = size;
    }
    if !changed { return Ok(serde_json::json!({ "id": id.0, "size": size, "changes": [] })); }
    let extent: f32 = tracks.iter().sum();
    set_extent(object, axis, extent, anchor)?;
    let mut reply = serde_json::json!({ "id": id.0, "axis": params["axis"], "size": size });
    if params.get("index").is_some() {
        reply["index"] = serde_json::json!(indices[0]);
    } else {
        reply["indices"] = serde_json::json!(indices);
    }
    crate::commands::document_change::commit(session, &command.name, next, session.selection.ids.clone(), reply)
}

fn leading_corner(object: &crate::Object) -> [f32; 2] {
    crate::geometry::apply_matrix(&crate::geometry::object_matrix(object), object.pos)
}

fn set_extent(object: &mut crate::Object, axis: usize, extent: f32, anchor: [f32; 2]) -> Result<(), CommandError> {
    if !extent.is_finite() || extent <= 0.0 { return Err(CommandError::out_of_range()); }
    object.size[axis] = extent.copysign(object.size[axis]);
    // Rotation uses the object's centre; keep the leading corner fixed as the extent changes.
    let shifted = leading_corner(object);
    object.pos[0] += anchor[0] - shifted[0];
    object.pos[1] += anchor[1] - shifted[1];
    if object.pos.iter().any(|v| !v.is_finite()) { return Err(CommandError::out_of_range()); }
    Ok(())
}

#[derive(Clone, Copy)]
enum TrackAxis {
    Row,
    Column,
}

impl TrackAxis {
    fn fields(self) -> (&'static str, &'static str) {
        match self { Self::Row => ("row", "rows"), Self::Column => ("col", "cols") }
    }

    fn axis(self) -> usize {
        match self { Self::Column => 0, Self::Row => 1 }
    }

    fn tracks(self, table: &TableNode) -> &[f32] {
        match self { Self::Row => &table.row_heights_pt, Self::Column => &table.col_widths_pt }
    }

    fn dimensions(self, cell: &TableCell) -> (u32, u32, u32, u32) {
        match self {
            Self::Row => (cell.row, cell.col, cell.row_span, cell.col_span),
            Self::Column => (cell.col, cell.row, cell.col_span, cell.row_span),
        }
    }

    fn coordinate(self, cell: &mut TableCell) -> (&mut u32, &mut u32) {
        match self {
            Self::Row => (&mut cell.row, &mut cell.row_span),
            Self::Column => (&mut cell.col, &mut cell.col_span),
        }
    }

    fn lengths(self, table: &mut TableNode) -> (&mut Vec<f32>, usize) {
        match self {
            Self::Row => (&mut table.row_heights_pt, table.col_widths_pt.len()),
            Self::Column => (&mut table.col_widths_pt, table.row_heights_pt.len()),
        }
    }

    fn default_size(self) -> f32 {
        match self { Self::Row => 20.0, Self::Column => 100.0 }
    }
}

fn track_list(value: &Value) -> Result<Vec<u32>, CommandError> {
    let mut indices: Vec<_> = value.as_array().ok_or_else(CommandError::invalid_params)?
        .iter().map(track_index).collect::<Result<_, _>>()?;
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

fn track_index(value: &Value) -> Result<u32, CommandError> {
    value.as_u64().and_then(|index| u32::try_from(index).ok()).ok_or_else(CommandError::invalid_params)
}

fn track_target(session: &Session, command: &Command, axis: TrackAxis) -> Result<(crate::ObjectId, Vec<u32>), CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let target = command.params.get("id").or_else(|| command.params.get("object_id"))
        .map_or(Value::Null, |id| json!({"id": id}));
    let (id, _) = query_target(session, &target)?;
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let crate::ObjectKind::Table(table) = &object.kind else { return Err(CommandError::invalid_params()); };
    let count = axis.tracks(table).len() as u32;
    let indices = command.params.get(axis.fields().1)
        .map(track_list).transpose()?.filter(|indices| !indices.is_empty()).unwrap_or_else(|| (0..count).collect());
    if indices.iter().any(|index| *index >= count) { return Err(CommandError::out_of_range()); }
    Ok((id, indices))
}

fn distribute(session: &mut Session, command: &Command, axis: TrackAxis) -> CommandResult {
    let (id, indices) = track_target(session, command, axis)?;
    if indices.len() < 2 { return Ok(json!({"id": id.0, "changes": []})); }
    let mut next = session.document.clone();
    let object = next.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    let crate::ObjectKind::Table(table) = &mut object.kind else { return Err(CommandError::invalid_params()); };
    let (tracks, _) = axis.lengths(table);
    let total: f64 = indices.iter().map(|&index| f64::from(tracks[index as usize])).sum();
    let size = (total / indices.len() as f64) as f32;
    if !size.is_finite() || size <= 0.0 { return Err(CommandError::out_of_range()); }
    if indices.iter().all(|&index| tracks[index as usize] == size) {
        return Ok(json!({"id": id.0, "changes": []}));
    }
    for &index in &indices {
        tracks[index as usize] = size;
    }
    crate::commands::document_change::commit(session, &command.name, next, session.selection.ids.clone(), json!({"id": id.0, "size": size}))
}

fn autofit(session: &mut Session, command: &Command, axis: TrackAxis) -> CommandResult {
    let (id, indices) = track_target(session, command, axis)?;
    if !super::fit::available() { return Err(CommandError::new("NOT_AVAILABLE")); }
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    let crate::ObjectKind::Table(table) = &object.kind else { return Err(CommandError::invalid_params()); };
    let tracks = axis.tracks(table);
    let sizes = super::fit::fit(&session.document, id, matches!(axis, TrackAxis::Row), tracks, &indices)
        .ok_or_else(CommandError::out_of_range)?;
    if sizes == tracks { return Ok(json!({"id": id.0, "sizes": sizes, "changes": []})); }
    let extent = sizes.iter().sum();
    let mut next = session.document.clone();
    let object = next.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    let anchor = leading_corner(object);
    let crate::ObjectKind::Table(table) = &mut object.kind else { return Err(CommandError::invalid_params()); };
    *axis.lengths(table).0 = sizes.clone();
    set_extent(object, axis.axis(), extent, anchor)?;
    crate::commands::document_change::commit(session, &command.name, next, session.selection.ids.clone(), json!({"id": id.0, "sizes": sizes}))
}

fn edit_track(session: &mut Session, command: &Command, axis: TrackAxis, insert: bool) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let params = &command.params;
    let id = crate::ObjectId(params.get("object_id").or_else(|| params.get("id")).and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)?);
    let (field, list) = axis.fields();
    let indices = match (params.get(field), params.get(list)) {
        (Some(value), None) => vec![track_index(value)?],
        (None, Some(values)) if !insert => track_list(values)?,
        (None, None) if insert => vec![0],
        _ => return Err(CommandError::invalid_params()),
    };
    let index = *indices.first().ok_or_else(CommandError::invalid_params)?;
    let select = params.get("select").and_then(|value| value.as_bool()).unwrap_or(false);
    let mut next = session.document.clone();
    if insert { reserve_table_story_ids(&mut next); }
    let object = next.objects.iter_mut().find(|object| object.id == id).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let anchor = leading_corner(object);
    let crate::ObjectKind::Table(table) = &mut object.kind else { return Err(CommandError::invalid_params()); };
    let (tracks, across) = axis.lengths(table);
    let at = index as usize;
    let count = tracks.len();
    if (insert && (at > count || count >= MAX_ROWS_COLS as usize))
        || (!insert && (indices.len() >= count || indices.iter().any(|i| *i as usize >= count)))
    {
        return Err(CommandError::out_of_range());
    }
    let mut removed = Vec::new();
    if insert {
        let size = if params.get("match_neighbor").and_then(|value| value.as_bool()) == Some(true) {
            tracks.get(at.min(count.saturating_sub(1))).copied().unwrap_or_else(|| axis.default_size())
        } else {
            axis.default_size()
        };
        tracks.insert(at, size);
        insert_cells(table, axis, index, across, &mut next.stories, &mut next.next_story_id)?;
        if matches!(axis, TrackAxis::Row) {
            if at < table.header_rows as usize { table.header_rows += 1; }
            else if at > count.saturating_sub(table.footer_rows as usize) { table.footer_rows += 1; }
        }
    } else {
        let mut kept = Vec::with_capacity(count + 1);
        kept.push(0);
        let mut next_deleted = indices.iter().copied().peekable();
        let mut index = 0;
        tracks.retain(|_| {
            let retain = next_deleted.next_if_eq(&index).is_none();
            let remaining = kept[index as usize] + u32::from(retain);
            kept.push(remaining);
            index += 1;
            retain
        });
        removed = delete_cells(table, axis, &kept)?;
        if matches!(axis, TrackAxis::Row) {
            table.header_rows = kept[(table.header_rows as usize).min(count)] as u16;
            table.footer_rows = (kept[count] - kept[count.saturating_sub(table.footer_rows as usize)]) as u16;
        }
    }
    table.cells.sort_by_key(|cell| (cell.row, cell.col));
    let selected_index = index.min((match axis { TrackAxis::Row => table.row_heights_pt.len(), TrackAxis::Column => table.col_widths_pt.len() } - 1) as u32);
    let selected_cells = select.then(|| TableSelection {
        table: Some(id),
        cells: table.cells.iter().filter(|cell| {
            let (start, _, span, _) = axis.dimensions(cell);
            cell.merged_into.is_none() && (start..start + span).contains(&selected_index)
        }).map(|cell| cell.id).collect(),
        tracks: Some(match axis {
            TrackAxis::Row => TrackSelection::Rows(vec![selected_index]),
            TrackAxis::Column => TrackSelection::Columns(vec![selected_index]),
        }),
    });
    let extent = match axis { TrackAxis::Row => table.row_heights_pt.iter().sum(), TrackAxis::Column => table.col_widths_pt.iter().sum() };
    set_extent(object, axis.axis(), extent, anchor)?;
    for story in removed {
        crate::frame::drop_story_if_unowned(&mut next, story);
    }
    let selection = if select { vec![id] } else { session.selection.ids.clone() };
    let cells = selected_cells.unwrap_or_else(|| {
        let mut selection = selection::current(session);
        if selection.table == Some(id) {
            let tracks = match (axis, &mut selection.tracks) {
                (TrackAxis::Row, Some(TrackSelection::Rows(tracks))) | (TrackAxis::Column, Some(TrackSelection::Columns(tracks))) => Some(tracks),
                _ => None,
            };
            if let Some(tracks) = tracks {
                tracks.retain_mut(|value| {
                    if insert {
                        *value += u32::from(*value >= index);
                        true
                    } else if let Err(removed_before) = indices.binary_search(value) {
                        *value -= removed_before as u32;
                        true
                    } else { false }
                });
            }
        }
        selection.reconciled(&next)
    });
    if session.text_edit.as_ref().is_some_and(|edit| !next.stories.iter().any(|story| story.id == edit.story)) {
        session.text_edit = None;
    }
    crate::commands::document_change::commit_table(session, &command.name, next, selection, cells, json!({ "ok": true, "id": id.0 }))
}

fn insert_cells(
    table: &mut TableNode, axis: TrackAxis, index: u32, across: usize,
    stories: &mut Vec<crate::Story>, next_story_id: &mut u64,
) -> Result<(), CommandError> {
    let mut anchors = vec![None; across];
    for cell in &mut table.cells {
        let (start, cross, span, cross_span) = axis.dimensions(cell);
        let owns_merge = cell.merged_into.is_none() && start < index && index < start + span;
        let id = cell.id;
        let (position, length) = axis.coordinate(cell);
        if start >= index {
            *position += 1;
        } else if owns_merge {
            *length += 1;
            let end = cross.checked_add(cross_span).ok_or_else(CommandError::out_of_range)?;
            anchors.get_mut(cross as usize..end as usize).ok_or_else(CommandError::out_of_range)?.fill(Some(id));
        }
    }
    table.cells.reserve(across);
    for (cross, merged_into) in anchors.into_iter().enumerate() {
        let (row, col) = match axis { TrackAxis::Row => (index, cross as u32), TrackAxis::Column => (cross as u32, index) };
        table.cells.push(TableCell {
            id: table.alloc.try_next()?, row, col, row_span: 1, col_span: 1, merged_into,
            story: push_cell_story(stories, next_story_id, String::new()),
            local: default_cell_format(), mask: default_cell_mask(),
        });
    }
    Ok(())
}

fn delete_cells(table: &mut TableNode, axis: TrackAxis, kept: &[u32]) -> Result<Vec<crate::StoryId>, CommandError> {
    let mut promoted = std::collections::HashMap::new();
    for cell in &table.cells {
        let (start, cross, span, _) = axis.dimensions(cell);
        let end = start.checked_add(span).filter(|end| span > 0 && (*end as usize) < kept.len()).ok_or_else(CommandError::out_of_range)?;
        let position = kept[start as usize];
        if cell.merged_into.is_none() && kept[start as usize + 1] == position && kept[end as usize] > position {
            promoted.insert(cell.id, (position, cross));
        }
    }
    let mut removed = Vec::new();
    table.cells.retain_mut(|cell| {
        let (start, cross, span, _) = axis.dimensions(cell);
        let position = kept[start as usize];
        let span = kept[(start + span) as usize] - position;
        let replaced_by_anchor = cell.merged_into.and_then(|id| promoted.get(&id)) == Some(&(position, cross));
        if span == 0 || replaced_by_anchor {
            removed.push(cell.story);
            return false;
        }
        let (coordinate, length) = axis.coordinate(cell);
        *coordinate = position;
        *length = span;
        true
    });
    Ok(removed)
}
fn handle_import(session: &mut Session, command: &Command) -> CommandResult {
    reserve_table_story_ids(&mut session.document);
    let tsv = command.params.get("tsv").and_then(|v| v.as_str()).unwrap_or("");
    let lines: Vec<&str> = tsv.lines().collect();
    if lines.is_empty() {
        return Err(CommandError::invalid_params());
    }

    let cols = lines[0].split('\t').count() as u32;
    let rows = lines.len() as u32;

    if rows < 1 || cols < 1 || rows > MAX_ROWS_COLS || cols > MAX_ROWS_COLS {
        return Err(CommandError::out_of_range());
    }

    let mut alloc = CellIdAllocator { high_water: 0 };
    let mut cells = Vec::new();

    for (r, line) in lines.iter().enumerate() {
        for (c, val) in line.split('\t').enumerate() {
            let id = alloc.next();
            let story_id = push_cell_story(&mut session.document.stories, &mut session.document.next_story_id, val.to_string()).0;

            cells.push(TableCell {
                id,
                row: r as u32,
                col: c as u32,
                row_span: 1,
                col_span: 1,
                merged_into: None,
                story: crate::document::StoryId(story_id),
                local: default_cell_format(),
                mask: default_cell_mask(),
            });
        }
    }

    let table = TableNode {
        alloc,
        col_widths_pt: vec![100.0; cols as usize],
        row_heights_pt: vec![20.0; rows as usize],
        cells,
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: default_frame_format(),
        frame_mask: default_frame_mask(),
    };

    session.generation += 1;
    Ok(serde_json::json!({ "table": table }))
}

fn handle_paste(session: &mut Session, command: &Command) -> CommandResult {
    reserve_table_story_ids(&mut session.document);
    let object_id = command.params.get("object_id").and_then(|v| v.as_u64()).unwrap_or(0);
    let start_row = command.params.get("row").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let start_col = command.params.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let tsv = command.params.get("tsv").and_then(|v| v.as_str()).unwrap_or("");
    let mut replaced = Vec::new();

    if let Some(obj) = session.document.objects.iter_mut().find(|o| o.id.0 == object_id) {
        if let crate::object::ObjectKind::Table(ref mut table) = obj.kind {
            let lines: Vec<&str> = tsv.lines().collect();
            for (r, line) in lines.iter().enumerate() {
                for (c, val) in line.split('\t').enumerate() {
                    let target_row = start_row + r as u32;
                    let target_col = start_col + c as u32;

                    if let Some(cell) = table.cells.iter_mut().find(|cell| cell.row == target_row && cell.col == target_col) {
                        replaced.push(cell.story);
                        cell.story = push_cell_story(&mut session.document.stories, &mut session.document.next_story_id, val.to_string());
                    } else {
                        let id = table.alloc.next();
                        let story_id = push_cell_story(&mut session.document.stories, &mut session.document.next_story_id, val.to_string()).0;

                        let new_cell = TableCell {
                            id,
                            row: target_row,
                            col: target_col,
                            row_span: 1,
                            col_span: 1,
                            merged_into: None,
                            story: crate::document::StoryId(story_id),
                            local: default_cell_format(),
                            mask: default_cell_mask(),
                        };

                        let insert_pos = table.cells.iter().position(|c| c.row > target_row || (c.row == target_row && c.col > target_col)).unwrap_or(table.cells.len());
                        table.cells.insert(insert_pos, new_cell);
                    }
                }
            }
        }
    }
    for story in replaced {
        crate::frame::drop_story_if_unowned(&mut session.document, story);
    }

    session.generation += 1;
    Ok(serde_json::json!({ "ok": true }))
}

#[derive(Clone)]
enum FormatChange<T> {
    Keep,
    Clear,
    Set(T),
}

impl<T: Clone> FormatChange<T> {
    fn apply(&self, bit: u32, target: &mut T, mask: &mut u32, default: T) {
        match self {
            FormatChange::Keep => {}
            FormatChange::Clear => {
                *target = default;
                *mask &= !bit;
            }
            FormatChange::Set(value) => {
                *target = value.clone();
                *mask |= bit;
            }
        }
    }
}

fn format_change<T>(params: &serde_json::Value, key: &str, parse: impl Fn(&serde_json::Value) -> Option<T>) -> Result<FormatChange<T>, CommandError> {
    match params.get(key) {
        None => Ok(FormatChange::Keep),
        Some(serde_json::Value::Null) => Ok(FormatChange::Clear),
        Some(value) => parse(value).map(FormatChange::Set).ok_or_else(CommandError::invalid_params),
    }
}

pub(super) fn parse_cell_id(value: &serde_json::Value) -> Option<CellId> {
    match value {
        serde_json::Value::Number(n) => n.as_u64().map(CellId),
        serde_json::Value::String(s) => s.strip_prefix('c').unwrap_or(s).parse::<u64>().ok().map(CellId),
        _ => None,
    }
}

fn parse_enum<T: serde::de::DeserializeOwned>(value: &serde_json::Value, aliases: &[(&str, &str)]) -> Option<T> {
    let text = value.as_str()?;
    let lower = text.to_ascii_lowercase();
    let name = aliases.iter().find(|(alias, _)| *alias == lower).map(|(_, name)| (*name).to_string()).unwrap_or_else(|| text.to_string());
    serde_json::from_value(serde_json::Value::String(name)).ok()
}

fn parse_rotation(value: &serde_json::Value) -> Option<crate::table::model::CellRotation> {
    use crate::table::model::CellRotation;
    match value.as_u64() {
        Some(0) => Some(CellRotation::R0),
        Some(90) => Some(CellRotation::R90),
        Some(180) => Some(CellRotation::R180),
        Some(270) => Some(CellRotation::R270),
        Some(_) => None,
        None => parse_enum(value, &[("r0", "R0"), ("r90", "R90"), ("r180", "R180"), ("r270", "R270")]),
    }
}

fn parse_edge(value: &serde_json::Value) -> Option<Option<crate::table::model::EdgeStyle>> {
    if value.is_null() {
        return Some(None);
    }
    let paint = value.get("paint").or_else(|| value.get("colour")).or_else(|| value.get("color")).and_then(crate::paint::paint_from_legacy_value)?;
    let width = value.get("width_pt").or_else(|| value.get("width")).and_then(|v| v.as_f64())? as f32;
    if !width.is_finite() || width < 0.0 || !crate::paint::validate_paint(&paint) {
        return None;
    }
    let line_style = value.get("line_style").map_or(Some(Default::default()), |style| {
        parse_enum(style, &[("solid", "Solid"), ("dashed", "Dashed"), ("dotted", "Dotted")])
    })?;
    Some(Some(crate::table::model::EdgeStyle { paint, width_pt: width, line_style }))
}

fn parse_borders(value: &serde_json::Value) -> Option<[Option<Option<crate::table::model::EdgeStyle>>; 4]> {
    use crate::table::resolve::{CELL_EDGE_BOTTOM, CELL_EDGE_LEFT, CELL_EDGE_RIGHT, CELL_EDGE_TOP};
    let map = value.as_object()?;
    let mut sides: [Option<Option<crate::table::model::EdgeStyle>>; 4] = [None, None, None, None];
    if let Some(mask) = map.get("mask") {
        let mask = mask.as_u64()? as u32;
        let style = parse_edge(map.get("edge").or_else(|| map.get("style")).unwrap_or(&serde_json::Value::Null))?;
        for (index, bit) in [CELL_EDGE_LEFT, CELL_EDGE_TOP, CELL_EDGE_RIGHT, CELL_EDGE_BOTTOM].iter().enumerate() {
            if mask & bit != 0 {
                sides[index] = Some(style.clone());
            }
        }
    }
    if let Some(all) = map.get("all") {
        let style = parse_edge(all)?;
        for side in sides.iter_mut() {
            *side = Some(style.clone());
        }
    }
    for (index, key) in ["left", "top", "right", "bottom"].iter().enumerate() {
        if let Some(edge) = map.get(*key) {
            sides[index] = Some(parse_edge(edge)?);
        }
    }
    let known = ["mask", "edge", "style", "all", "left", "top", "right", "bottom"];
    if map.keys().any(|key| !known.contains(&key.as_str())) || sides.iter().all(|side| side.is_none()) {
        return None;
    }
    Some(sides)
}

pub fn selection_border_slots<'a>(cells: impl Iterator<Item = &'a TableCell>) -> std::collections::HashSet<(u32, u32)> {
    cells.flat_map(|cell| {
        (cell.row..cell.row + cell.row_span).flat_map(move |row| (cell.col..cell.col + cell.col_span).map(move |col| (row, col)))
    }).collect()
}

pub fn selection_border_sides(cell: &TableCell, slots: &std::collections::HashSet<(u32, u32)>, scope: &Edge) -> [Option<bool>; 4] {
    let rows = cell.row..cell.row + cell.row_span;
    let cols = cell.col..cell.col + cell.col_span;
    let row_end = rows.end;
    let col_end = cols.end;
    let sides = [
        rows.clone().filter(|row| cell.col.checked_sub(1).is_some_and(|col| slots.contains(&(*row, col)))).count(),
        cols.clone().filter(|col| cell.row.checked_sub(1).is_some_and(|row| slots.contains(&(row, *col)))).count(),
        rows.filter(|row| slots.contains(&(*row, col_end))).count(),
        cols.filter(|col| slots.contains(&(row_end, *col))).count(),
    ];
    let exterior = [Edge::LEFT, Edge::TOP, Edge::RIGHT, Edge::BOTTOM];
    let interior = [Edge::INSIDE_V, Edge::INSIDE_H, Edge::INSIDE_V, Edge::INSIDE_H];
    let length = [cell.row_span, cell.col_span, cell.row_span, cell.col_span];
    std::array::from_fn(|side| {
        if sides[side] == 0 && scope.0 & exterior[side].0 != 0 {
            Some(true)
        } else if sides[side] == length[side] as usize && scope.0 & interior[side].0 != 0 {
            // A shared edge is drawn once, on the cell above or to its left.
            Some(side >= 2)
        } else {
            None
        }
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionEdgePatch {
    paint: Option<crate::paint::Paint>,
    width_pt: Option<f32>,
    line_style: Option<crate::paint::LineStyle>,
}

impl SelectionEdgePatch {
    fn apply(&self, current: Option<EdgeStyle>) -> EdgeStyle {
        use crate::paint::{Colour, Paint};
        let fallback_paint = || Paint::Solid { colour: Colour::new(0, 0, 0, 255) };
        let mut edge = current.unwrap_or_else(|| EdgeStyle {
            paint: fallback_paint(), width_pt: 1.0, line_style: Default::default(),
        });
        if let Some(paint) = &self.paint {
            if !matches!(paint, Paint::None) && edge.width_pt == 0.0 && self.width_pt.is_none() {
                edge.width_pt = 1.0;
            }
            edge.paint = paint.clone();
        }
        if let Some(width) = self.width_pt {
            edge.width_pt = width;
            if width > 0.0 && self.paint.is_none() && matches!(edge.paint, Paint::None) {
                edge.paint = fallback_paint();
            }
        }
        edge.line_style = self.line_style.unwrap_or(edge.line_style);
        edge
    }
}

fn apply_edges(
    sides: &[Option<Option<crate::table::model::EdgeStyle>>; 4],
    edges: &mut [Option<crate::table::model::EdgeStyle>; 4],
    mask: &mut u32,
    bits: [u32; 4],
) {
    for (index, side) in sides.iter().enumerate() {
        if let Some(style) = side {
            edges[index] = style.clone();
            if style.is_some() { *mask |= bits[index]; } else { *mask &= !bits[index]; }
        }
    }
}

fn edge_patch(params: &serde_json::Value, key: &str) -> Result<Option<[Option<Option<crate::table::model::EdgeStyle>>; 4]>, CommandError> {
    match params.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(Some([Some(None), Some(None), Some(None), Some(None)])),
        Some(value) => parse_borders(value).map(Some).ok_or_else(CommandError::invalid_params),
    }
}

fn handle_frame_format_set(session: &mut Session, command: &Command) -> CommandResult {
    use crate::table::resolve::{FRAME_BASELINE, FRAME_EDGES, FRAME_FILL, FRAME_IGNORE_BASELINE_GRID, FRAME_INSETS};
    use crate::table::model::{BaselineGrid, Insets};
    use serde_json::{json, Value};
    if !session.has_document { return Err(CommandError::no_document()); }
    let params = &command.params;
    if params.as_object().is_none_or(|values| values.keys().any(|key| !matches!(key.as_str(), "id" | "object_id" | "frame" | "header_rows" | "footer_rows" | "baseline"))) {
        return Err(CommandError::invalid_params());
    }
    let id = params.get("id").or_else(|| params.get("object_id")).and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?;
    let frame = params.get("frame").unwrap_or(&Value::Null);
    if !frame.is_null() && frame.as_object().is_none_or(|values| values.keys().any(|key| !matches!(key.as_str(), "fill" | "edges" | "insets" | "inset_patch" | "ignore_baseline_grid" | "baseline"))) {
        return Err(CommandError::invalid_params());
    }
    let fill = format_change(frame, "fill", |v| crate::paint::paint_from_legacy_value(v).filter(crate::paint::validate_paint).map(Some))?;
    let edges = edge_patch(frame, "edges")?;
    let mut insets = format_change(frame, "insets", parse_insets)?;
    let inset_patch = frame.get("inset_patch").map(|value| serde_json::from_value::<InsetPatch>(value.clone()).map_err(|_| CommandError::invalid_params())).transpose()?;
    let ignore_grid = format_change(frame, "ignore_baseline_grid", Value::as_bool)?;
    let baseline_patch = match (params.get("baseline"), frame.get("baseline")) {
        (Some(_), Some(_)) => return Err(CommandError::invalid_params()),
        (top, nested) => top.or(nested),
    };
    if inset_patch.is_some() && !matches!(insets, FormatChange::Keep) { return Err(CommandError::invalid_params()); }
    let count = |key: &str| -> Result<Option<u16>, CommandError> {
        params.get(key).map(|value| value.as_u64().and_then(|count| u16::try_from(count).ok()).ok_or_else(CommandError::invalid_params)).transpose()
    };
    let (header, footer) = (count("header_rows")?, count("footer_rows")?);
    let changes_bands = header.is_some() || footer.is_some();
    if matches!(fill, FormatChange::Keep) && edges.is_none() && !changes_bands
        && matches!(insets, FormatChange::Keep) && inset_patch.is_none()
        && matches!(ignore_grid, FormatChange::Keep) && baseline_patch.is_none()
    {
        return Err(CommandError::invalid_params());
    }
    let crate::ObjectKind::Table(table) = &session.document.get_object(crate::ObjectId(id)).ok_or_else(CommandError::not_found)?.kind else { return Err(CommandError::invalid_params()); };
    let current_frame = crate::table::resolve::resolve_frame(&session.document, table);
    if let Some(patch) = inset_patch { insets = FormatChange::Set(patch.apply(current_frame.insets)?); }
    let baseline = match baseline_patch {
        None => FormatChange::Keep,
        Some(Value::Null) => FormatChange::Clear,
        Some(patch) => {
            let mut current = json!(current_frame.baseline.unwrap_or_default());
            for (key, value) in patch.as_object().filter(|patch| !patch.is_empty()).ok_or_else(CommandError::invalid_params)? {
                if !current.as_object().is_some_and(|fields| fields.contains_key(key)) { return Err(CommandError::invalid_params()); }
                current[key] = value.clone();
            }
            let grid: BaselineGrid = serde_json::from_value(current).map_err(|_| CommandError::invalid_params())?;
            if !grid.start_pt.is_finite() || !grid.spacing_pt.is_finite() || grid.spacing_pt <= 0.0
                || !grid.color.is_ascii() || !matches!(crate::paint::paint_from_legacy_str(&grid.color), Some(crate::paint::Paint::Solid { .. }))
            { return Err(CommandError::invalid_params()); }
            FormatChange::Set(Some(grid))
        }
    };
    let mut next = session.document.clone();
    let object = next.find_object_mut(id).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let crate::ObjectKind::Table(table) = &mut object.kind else { return Err(CommandError::invalid_params()); };
    let header = header.unwrap_or(table.header_rows);
    let footer = footer.unwrap_or(table.footer_rows);
    if changes_bands && usize::from(header) + usize::from(footer) > table.row_heights_pt.len() {
        return Err(CommandError::out_of_range());
    }
    (table.header_rows, table.footer_rows) = (header, footer);
    fill.apply(FRAME_FILL, &mut table.frame_local.fill, &mut table.frame_mask.0, None);
    insets.apply(FRAME_INSETS, &mut table.frame_local.insets, &mut table.frame_mask.0, Insets::default());
    ignore_grid.apply(FRAME_IGNORE_BASELINE_GRID, &mut table.frame_local.ignore_baseline_grid, &mut table.frame_mask.0, false);
    baseline.apply(FRAME_BASELINE, &mut table.frame_local.baseline, &mut table.frame_mask.0, None);
    if let Some(edges) = edges {
        apply_edges(&edges, &mut table.frame_local.edges, &mut table.frame_mask.0, FRAME_EDGES);
    }
    crate::commands::document_change::commit(session, &command.name, next, session.selection.ids.clone(), json!({"id": id}))
}

fn parse_insets(value: &Value) -> Option<crate::table::model::Insets> {
    serde_json::from_value::<crate::table::model::Insets>(value.clone()).ok()
        .filter(|insets| [insets.left, insets.top, insets.right, insets.bottom].iter().all(|value| value.is_finite() && *value >= 0.0))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InsetPatch {
    left: Option<f32>,
    top: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    linked: Option<bool>,
}

impl InsetPatch {
    fn apply(&self, mut insets: crate::table::model::Insets) -> Result<crate::table::model::Insets, CommandError> {
        insets.linked = self.linked.unwrap_or(insets.linked);
        let values = [self.left, self.top, self.right, self.bottom];
        if self.linked.is_none() && values.iter().all(Option::is_none) {
            return Err(CommandError::invalid_params());
        }
        if values.iter().flatten().any(|value| !value.is_finite() || *value < 0.0) {
            return Err(CommandError::invalid_params());
        }
        let mut edits = values.iter().enumerate().filter_map(|(index, value)| value.map(|value| (index, value)));
        if let (Some((index, value)), None) = (edits.next(), edits.next()) {
            return Ok(insets.with_side(index, value));
        }
        if let Some(first) = values.iter().flatten().next().copied().filter(|_| insets.linked) {
            if values.iter().flatten().any(|value| *value != first) {
                return Err(CommandError::invalid_params());
            }
            (insets.left, insets.top, insets.right, insets.bottom) = (first, first, first, first);
        } else {
            insets.left = self.left.unwrap_or(insets.left);
            insets.top = self.top.unwrap_or(insets.top);
            insets.right = self.right.unwrap_or(insets.right);
            insets.bottom = self.bottom.unwrap_or(insets.bottom);
        }
        Ok(insets)
    }
}

fn handle_cell_format_set(session: &mut Session, command: &Command) -> CommandResult {
    use crate::table::resolve::{CELL_EDGE_BOTTOM, CELL_EDGE_LEFT, CELL_EDGE_RIGHT, CELL_EDGE_TOP, CELL_FILL, CELL_INITIAL_ADVANCE, CELL_INSETS, CELL_MINIMUM, CELL_ROTATION, CELL_VERTICAL_ALIGN};
    let params = &command.params;
    let object_id = params.get("object_id").or_else(|| params.get("id")).and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)?;
    let cell_ids: Vec<CellId> = match params.get("cells").or_else(|| params.get("cell")) {
        Some(serde_json::Value::Array(items)) => items.iter().map(parse_cell_id).collect::<Option<Vec<_>>>().ok_or_else(CommandError::invalid_params)?,
        Some(value) => vec![parse_cell_id(value).ok_or_else(CommandError::invalid_params)?],
        None => return Err(CommandError::invalid_params()),
    };
    if cell_ids.is_empty() {
        return Err(CommandError::invalid_params());
    }

    let fill = format_change(params, "fill", |v| crate::paint::paint_from_legacy_value(v).filter(crate::paint::validate_paint).map(Some))?;
    let insets = format_change(params, "insets", parse_insets)?;
    let inset_patch: Option<InsetPatch> = params.get("inset_patch").map(|value| {
        if params.get("insets").is_some() { return Err(CommandError::invalid_params()); }
        serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())
    }).transpose()?;
    let vertical_align = format_change(params, "vertical_align", |v| parse_enum(v, &[("top", "Top"), ("middle", "Center"), ("center", "Center"), ("centre", "Center"), ("bottom", "Bottom"), ("justify", "Justify")]))?;
    let rotation = format_change(params, "rotation", parse_rotation)?;
    let initial_advance = format_change(params, "initial_advance", |v| {
        parse_enum(v, &[("default", "Default"), ("ascent", "Ascent"), ("cap_height", "CapHeight"), ("x_height", "XHeight"), ("leading", "Leading"), ("fixed", "Fixed")])
    })?;
    let minimum = format_change(params, "minimum", |v| v.as_f64().map(|x| x as f32).filter(|x| x.is_finite() && *x >= 0.0))?;
    let selection_borders = params.get("borders").filter(|value| value.get("selection_mask").is_some()).map(|value| {
        let mask = value["selection_mask"].as_u64().filter(|mask| (1..=u64::from(Edge::ALL.0)).contains(mask)).ok_or_else(CommandError::invalid_params)?;
        if value.as_object().is_none_or(|map| map.len() != 2) { return Err(CommandError::invalid_params()); }
        let is_patch = value.get("edge_patch").is_some();
        let patch = match value.get("edge_patch") {
            Some(patch) => serde_json::from_value::<SelectionEdgePatch>(patch.clone()).map_err(|_| CommandError::invalid_params())?,
            None => {
                let edge = value.get("edge").and_then(parse_edge).flatten().ok_or_else(CommandError::invalid_params)?;
                SelectionEdgePatch { paint: Some(edge.paint), width_pt: Some(edge.width_pt), line_style: Some(edge.line_style) }
            }
        };
        if patch.width_pt.is_some_and(|width| !width.is_finite() || width < 0.0)
            || patch.paint.as_ref().is_some_and(|paint| !crate::paint::validate_paint(paint))
            || (patch.paint.is_none() && patch.width_pt.is_none() && patch.line_style.is_none())
        {
            return Err(CommandError::invalid_params());
        }
        Ok((Edge(mask as u8), patch, is_patch))
    }).transpose()?;
    let borders = if selection_borders.is_none() { edge_patch(params, "borders")? } else { None };

    let selected: std::collections::HashSet<_> = cell_ids.iter().copied().collect();
    let mut updated_insets = std::collections::HashMap::new();
    let mut resolved_edges = std::collections::HashMap::new();
    if inset_patch.is_some() || selection_borders.is_some() {
        let object = session.document.get_object(crate::ObjectId(object_id)).ok_or_else(CommandError::not_found)?;
        let crate::ObjectKind::Table(table) = &object.kind else { return Err(CommandError::invalid_params()); };
        for cell in table.cells.iter().filter(|cell| selected.contains(&cell.id)) {
            let format = crate::table::resolve_cell(&session.document, table, cell);
            if let Some(patch) = &inset_patch {
                updated_insets.insert(cell.id, patch.apply(format.insets)?);
            }
            if selection_borders.is_some() {
                resolved_edges.insert(cell.id, format.edges);
            }
        }
    }
    let mut after = session.document.clone();
    let object = after.objects.iter_mut().find(|o| o.id.0 == object_id).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let crate::object::ObjectKind::Table(table) = &mut object.kind else {
        return Err(CommandError::invalid_params());
    };
    if cell_ids.iter().any(|id| !table.cells.iter().any(|cell| cell.id == *id && cell.merged_into.is_none())) {
        return Err(CommandError::not_found());
    }

    let slots = selection_borders.as_ref().map(|_| selection_border_slots(table.cells.iter().filter(|cell| selected.contains(&cell.id))));
    let factory = CellFormat::default();
    let edge_bits = [CELL_EDGE_LEFT, CELL_EDGE_TOP, CELL_EDGE_RIGHT, CELL_EDGE_BOTTOM];
    for cell in table.cells.iter_mut().filter(|cell| selected.contains(&cell.id)) {
        let selected_sides = selection_borders.as_ref().zip(slots.as_ref())
            .map(|((scope, _, _), slots)| selection_border_sides(cell, slots, scope));
        let mask = &mut cell.mask.0;
        let local = &mut cell.local;
        fill.apply(CELL_FILL, &mut local.fill, mask, factory.fill.clone());
        insets.apply(CELL_INSETS, &mut local.insets, mask, factory.insets);
        if let Some(insets) = updated_insets.get(&cell.id) {
            local.insets = *insets;
            *mask |= CELL_INSETS;
        }
        vertical_align.apply(CELL_VERTICAL_ALIGN, &mut local.vertical_align, mask, factory.vertical_align);
        rotation.apply(CELL_ROTATION, &mut local.rotation, mask, factory.rotation);
        initial_advance.apply(CELL_INITIAL_ADVANCE, &mut local.initial_advance, mask, factory.initial_advance);
        minimum.apply(CELL_MINIMUM, &mut local.minimum_pt, mask, factory.minimum_pt);
        if let Some(sides) = &borders {
            apply_edges(sides, &mut local.edges, mask, edge_bits);
        }
        if let Some((sides, (_, patch, is_patch))) = selected_sides.zip(selection_borders.as_ref()) {
            let mut current = resolved_edges.remove(&cell.id).unwrap_or_default();
            for (index, side) in sides.into_iter().enumerate() {
                if let Some(owner) = side {
                    let edge = current[index].take();
                    let visible = edge.as_ref().is_some_and(|edge| edge.width_pt > 0.0 && !matches!(edge.paint, crate::paint::Paint::None));
                    if *is_patch && !owner && !visible {
                        continue;
                    }
                    local.edges[index] = Some(if owner || *is_patch { patch.apply(edge) } else {
                        crate::table::model::EdgeStyle { paint: crate::paint::Paint::None, width_pt: 0.0, line_style: Default::default() }
                    });
                    *mask |= edge_bits[index];
                }
            }
        }
    }
    let ids: Vec<String> = cell_ids.iter().map(|id| format!("c{}", id.0)).collect();
    crate::commands::document_change::commit(session, &command.name, after, session.selection.ids.clone(), serde_json::json!({"ok": true, "cells": ids}))
}

#[cfg(test)]
mod cell_format_set_contracts {
    use super::*;
    use crate::object::{Object, ObjectId, ObjectKind};
    use crate::paint::{Colour, Paint};
    use crate::table::model::{CellFormatMask, FrameFormatMask, StoryId, VerticalAlign};
    use crate::table::resolve::{CELL_EDGE_TOP, CELL_FILL, CELL_INSETS, CELL_VERTICAL_ALIGN};

    fn session_with_table() -> Session {
        let mut alloc = CellIdAllocator { high_water: 0 };
        let mut cells = Vec::new();
        for row in 0..2u32 {
            for col in 0..3u32 {
                cells.push(TableCell { id: alloc.next(), row, col, row_span: 1, col_span: 1, merged_into: None, story: StoryId(u64::from(row * 3 + col)), local: CellFormat::default(), mask: CellFormatMask::default() });
            }
        }
        let table = TableNode { alloc, col_widths_pt: vec![30.0; 3], row_heights_pt: vec![20.0; 2], cells, header_rows: 0, footer_rows: 0, format: None, frame_local: TableFrameFormat::default(), frame_mask: FrameFormatMask::default() };
        let mut session = Session::new(crate::Document::default());
        session.document.objects.push(Object { id: ObjectId(7), kind: ObjectKind::Table(table), ..Object::default() });
        session
    }

    fn cell(session: &Session, id: u64) -> TableCell {
        match &session.document.objects[0].kind {
            ObjectKind::Table(table) => table.cells.iter().find(|c| c.id.0 == id).unwrap().clone(),
            _ => panic!(),
        }
    }

    #[test]
    fn set_c5_format_only_touches_c5_and_undoes_in_one_step() {
        let mut session = session_with_table();
        let steps = session.undo.applied_steps();
        let command = Command::with_params(Command::TABLE_CELL_FORMAT_SET, serde_json::json!({
            "object_id": 7,
            "cells": ["c5"],
            "fill": "#ffff00",
            "insets": {"left": 2.0, "top": 2.0, "right": 2.0, "bottom": 2.0, "linked": true},
            "vertical_align": "middle",
            "borders": {"top": {"paint": "red", "width_pt": 1.0}}
        }));
        assert!(session.run(&command).is_ok());
        assert_eq!(session.undo.applied_steps(), steps + 1);
        let c5 = cell(&session, 5);
        assert_eq!(c5.local.fill, Some(Paint::Solid { colour: Colour::new(255, 255, 0, 255) }));
        assert_eq!(c5.local.insets.left, 2.0);
        assert_eq!(c5.local.vertical_align, VerticalAlign::Center);
        let top = c5.local.edges[1].clone().unwrap();
        assert_eq!(top.paint, Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
        assert_eq!(top.width_pt, 1.0);
        assert_eq!(c5.mask.0, CELL_FILL | CELL_INSETS | CELL_VERTICAL_ALIGN | CELL_EDGE_TOP);
        for other in [1u64, 2, 3, 4, 6] {
            let c = cell(&session, other);
            assert_eq!(c.mask.0, 0);
            assert!(c.local.fill.is_none());
        }
        let query = crate::table::table_query(&session.document, ObjectId(7), Some(CellId(5)));
        assert!(query.to_string().contains("\"solid\""));
        assert!(session.run(&Command::new("edit.undo")).is_ok());
        assert_eq!(cell(&session, 5).mask.0, 0);
        assert!(cell(&session, 5).local.fill.is_none());
        assert!(session.run(&Command::new("edit.redo")).is_ok());
        let redone = cell(&session, 5);
        assert_eq!(redone.local.fill, Some(Paint::Solid { colour: Colour::new(255, 255, 0, 255) }));
        assert_eq!(redone.mask.0, CELL_FILL | CELL_INSETS | CELL_VERTICAL_ALIGN | CELL_EDGE_TOP);
        assert!(session.run(&Command::new("edit.undo")).is_ok());
        assert!(session.run(&Command::new("edit.undo")).is_err());
    }

    #[test]
    fn null_clears_bit_and_bad_values_change_nothing() {
        let mut session = session_with_table();
        assert!(session.run(&Command::with_params(Command::TABLE_CELL_FORMAT_SET, serde_json::json!({"object_id": 7, "cells": [5], "fill": "#00ff00", "minimum": 4.0}))).is_ok());
        assert!(session.run(&Command::with_params(Command::TABLE_CELL_FORMAT_SET, serde_json::json!({"object_id": 7, "cells": [5], "fill": null}))).is_ok());
        let c5 = cell(&session, 5);
        assert!(c5.local.fill.is_none());
        assert_eq!(c5.mask.0 & CELL_FILL, 0);
        assert_eq!(c5.local.minimum_pt, 4.0);
        let steps = session.undo.applied_steps();
        assert!(session.run(&Command::with_params(Command::TABLE_CELL_FORMAT_SET, serde_json::json!({"object_id": 7, "cells": [5, 6], "fill": "#0000ff", "minimum": -1.0}))).is_err());
        assert!(session.run(&Command::with_params(Command::TABLE_CELL_FORMAT_SET, serde_json::json!({"object_id": 7, "cells": [5, 99], "fill": "#0000ff"}))).is_err());
        assert_eq!(session.undo.applied_steps(), steps);
        assert!(cell(&session, 6).local.fill.is_none());
    }

    #[test]
    fn style_table_define_undoes_and_redoes() {
        let mut session = session_with_table();
        let definition = serde_json::json!({
            "id": 0,
            "name": "draft",
            "frame": TableFrameFormat::default(),
            "cell_formats": [[0, "body", CellFormat::default()]],
            "pattern": {"rows": 1, "cols": 1, "top_band_rows": 0, "bottom_band_rows": 0, "left_band_cols": 0, "right_band_cols": 0, "slots": [0]}
        });
        assert!(session.run(&Command::with_params("style.table.define", serde_json::json!({"name": "Grid", "op": "create", "definition": definition}))).is_ok());
        assert_eq!(session.document.table_formats.len(), 1);
        assert!(session.run(&Command::new("edit.undo")).is_ok());
        assert!(session.document.table_formats.is_empty());
        assert!(session.run(&Command::new("edit.redo")).is_ok());
        assert_eq!(session.document.table_formats[0].name, "Grid");
    }

    #[test]
    fn restore_rejects_bad_snapshot_and_keeps_document() {
        let mut session = session_with_table();
        session.undo.push(crate::undo::Change { name: "probe".to_string(), undo: Command::with_params("core.feature.restore", serde_json::json!({"document": 5})), redo: Command::with_params("core.feature.restore", serde_json::json!({})) });
        assert!(session.run(&Command::new("edit.undo")).is_err());
        assert_eq!(session.document.objects.len(), 1);
    }
}
