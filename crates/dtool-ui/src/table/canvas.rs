use dtool_core::scene::Scene;
use dtool_core::table::{cell_at, table_grid, CellRect};
use dtool_core::table::model::{CellId, TableNode};
use dtool_core::table::{merge, selection};
use dtool_core::{Object, ObjectId, ObjectKind, Session, StoryId, ViewState};
use dtool_protocol::{Command, CommandError};
use dtool_tools::dtool_text::{frame, table_layout, FontLibrary};
use egui::{Pos2, Rect, Response, Ui};
use serde_json::json;

use crate::canvas::frame_text_input as text_input;

struct CellHit {
    table: ObjectId,
    cell: CellId,
    story: StoryId,
    local: [f32; 2],
}

fn hit(session: &Session, scene: &Scene, point: [f32; 2]) -> Option<CellHit> {
    scene.quads.iter().rev().find_map(|(id, _)| {
        let object = session.document.get_object(*id)?;
        let ObjectKind::Table(table) = &object.kind else { return None; };
        if object.locked || !object.pickable { return None; }
        let local = object_local(object, point)?;
        let cell = CellId(cell_at(&table_grid(table), local)?);
        let story = table.cells.iter().find(|entry| entry.id == cell)?.story;
        Some(CellHit { table: *id, cell, story, local })
    })
}

fn select_cells(session: &mut Session, table: ObjectId, cells: &[CellId]) -> Result<(), CommandError> {
    session.run(&Command::with_params("table.selection.set", json!({
        "id": table.0, "cells": cells
    })))?;
    Ok(())
}

fn select_range(session: &mut Session, id: ObjectId, start: CellId, end: CellId) -> Result<(), CommandError> {
    let Some(ObjectKind::Table(table)) = session.document.get_object(id).map(|object| &object.kind) else {
        return Ok(());
    };
    if let Some(cells) = merge::range_cells(table, start, end) {
        select_cells(session, id, &cells)?;
    }
    Ok(())
}

fn edit_cell(session: &mut Session, table: ObjectId, story: StoryId, caret: usize) -> Result<(), CommandError> {
    session.run(&Command::with_params("text.select", json!({
        "id": table.0, "story": story.0, "range": { "start": caret, "end": caret }
    })))?;
    Ok(())
}

fn active_cell(session: &Session) -> Option<(&Object, &TableNode, CellId, StoryId)> {
    let selection = dtool_core::table::selection::current(session);
    if selection.tracks.is_some() { return None; }
    let &[cell] = selection.cells.as_slice() else { return None; };
    let object = session.document.get_object(selection.table?)?;
    let ObjectKind::Table(table) = &object.kind else { return None; };
    if object.locked || !object.pickable { return None; }
    let story = table.cells.iter().find(|entry| entry.id == cell)?.story;
    Some((object, table, cell, story))
}

pub(crate) fn begin_selected_cell_edit(session: &mut Session) -> Result<bool, CommandError> {
    if session.active_tool != "tool.table" || session.text_edit.is_some() { return Ok(false); }
    let Some((object, _, _, story)) = active_cell(session) else { return Ok(false); };
    let caret = session.document.stories.iter().find(|item| item.id == story).map_or(0, |item| item.text.chars().count());
    edit_cell(session, object.id, story, caret)?;
    Ok(true)
}

pub(crate) fn edit_at(session: &mut Session, scene: &Scene, point: [f32; 2], object: Option<ObjectId>) -> Result<bool, CommandError> {
    let Some(target) = hit(session, scene, point).filter(|hit| object.is_none_or(|id| hit.table == id)) else { return Ok(false); };
    text_input::finish_text_input(session);
    session.run(&Command::with_params("ui.tool.set", json!({"id": "tool.table"})))?;
    select_cells(session, target.table, &[target.cell])?;
    edit_cell(session, target.table, target.story, caret_at(session, &target))?;
    Ok(true)
}

pub(crate) fn step_edit_cell(session: &mut Session, backward: bool) -> Result<bool, CommandError> {
    let Some((object, table, cell, _, _)) = active_text(session) else { return Ok(false); };
    let mut cells: Vec<_> = table.cells.iter().filter(|entry| entry.merged_into.is_none()).collect();
    cells.sort_unstable_by_key(|entry| (entry.row, entry.col));
    let Some(index) = cells.iter().position(|entry| entry.id == cell) else { return Ok(false); };
    let next = if backward { index.checked_sub(1) } else { Some(index + 1) };
    if let Some(target) = next.and_then(|index| cells.get(index)) {
        let (id, cell, story) = (object.id, target.id, target.story);
        text_input::finish_text_input(session);
        select_cells(session, id, &[cell])?;
        edit_cell(session, id, story, 0)?;
    }
    Ok(true)
}

pub(crate) fn starts_cell_text(input: &egui::InputState) -> bool {
    input.events.iter().any(|event| match event {
        egui::Event::Text(text) => !text.is_empty(),
        egui::Event::Ime(egui::ImeEvent::Preedit { .. } | egui::ImeEvent::Commit(_)) => true,
        _ => false,
    })
}

fn active_text(session: &Session) -> Option<(&Object, &TableNode, CellId, usize, usize)> {
    let edit = session.text_edit.as_ref()?;
    let object = session.document.get_object(edit.object?)?;
    let ObjectKind::Table(table) = &object.kind else { return None; };
    let cell = table.cells.iter().find(|entry| entry.story == edit.story && entry.merged_into.is_none())?;
    Some((object, table, cell.id, edit.caret, edit.anchor))
}

fn object_point(object: &Object, point: [f32; 2]) -> [f32; 2] {
    let matrix = dtool_core::geometry::object_matrix(object);
    dtool_core::geometry::apply_matrix(&matrix, [point[0] + object.pos[0], point[1] + object.pos[1]])
}

fn screen(rect: Rect, view: &ViewState, point: [f32; 2]) -> Pos2 {
    let [x, y] = dtool_core::doc_to_view(view, point);
    rect.min + egui::vec2(x, y)
}

pub(crate) fn paint_baseline_guides(ui: &Ui, rect: Rect, session: &Session, view: &ViewState) {
    if view.canvas_flags.preview_mode { return; }
    let global = session.document.grid.show_baseline;
    let painter = ui.painter_at(rect);
    for object in session.document.objects.iter().filter(|object| global || session.selection.ids.contains(&object.id)) {
        let ObjectKind::Table(table) = &object.kind else { continue; };
        if !dtool_core::scene::is_drawn_with_flags(&session.document, &view.canvas_flags, object.id) { continue; }
        let frame = dtool_core::table::resolve_frame(&session.document, table);
        if !frame.baseline.as_ref().is_some_and(|grid| grid.enabled) { continue; }
        let Some(guide) = dtool_core::table::baseline::guide(&session.document, &frame, Some(object)) else { continue; };
        let colour = crate::panels::table::baseline_colour(&guide.color);
        let height = object.size[1].abs();
        let width = object.size[0].abs();
        let step = guide.spacing * (3.0_f32 / (guide.spacing * view.zoom.max(1e-4))).ceil().max(1.0);
        let first = guide.start + (-guide.start / step).ceil().max(0.0) * step;
        for index in 0..1024 {
            let y = first + index as f32 * step;
            if y > height || !y.is_finite() { break; }
            let ends = [[0.0, y], [width, y]].map(|point| screen(rect, view, object_point(object, point)));
            painter.line_segment(ends, egui::Stroke::new(1.0, colour));
        }
    }
}

fn caret_at(session: &Session, hit: &CellHit) -> usize {
    let Some(object) = session.document.get_object(hit.table) else { return 0; };
    let ObjectKind::Table(table) = &object.kind else { return 0; };
    let library = FontLibrary::shared();
    let Some((text_box, lines)) = table_layout::cell_text_layout(&library, &session.document, table, hit.cell) else { return 0; };
    let [x, y] = text_box.to_layout(hit.local);
    frame::caret_at_point(&lines, [x - text_box.rect[0], y])
}


pub(crate) fn caret_line_at(session: &Session, index: usize) -> Option<[[f32; 2]; 2]> {
    let (object, table, cell, _, _) = active_text(session)?;
    let library = FontLibrary::shared();
    let (bounds, lines) = table_layout::cell_text_layout(&library, &session.document, table, cell)?;
    let line = lines.iter().rfind(|line| line.range.start <= index && index <= line.range.end);
    let (offset, baseline) = line.map_or((0.0, bounds.empty_baseline), |line| (frame::caret_x(line, index), line.baseline));
    Some([baseline - bounds.size * 0.8, baseline + bounds.size * 0.2].map(|y| {
        object_point(object, bounds.to_cell([bounds.rect[0] + offset, y]))
    }))
}

pub(crate) fn paint_text_range(
    ui: &Ui, rect: Rect, session: &Session, view: &ViewState, mark: text_input::TextRangeMark,
) {
    let Some((object, table, cell, caret, anchor)) = active_text(session) else { return; };
    let library = FontLibrary::shared();
    let Some((bounds, lines)) = table_layout::cell_text_layout(&library, &session.document, table, cell) else { return; };
    let painter = ui.painter_at(rect);
    for line in lines {
        let start = line.range.start.max(anchor.min(caret));
        let end = line.range.end.min(anchor.max(caret));
        if start >= end { continue; }
        let x0 = bounds.rect[0] + frame::caret_x(&line, start);
        let x1 = bounds.rect[0] + frame::caret_x(&line, end);
        let y0 = line.baseline - bounds.size * 0.8;
        let y1 = line.baseline + bounds.size * 0.2;
        let points = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
            .map(|point| screen(rect, view, object_point(object, bounds.to_cell(point))));
        mark.paint(&painter, points);
    }
}

fn selected_table(session: &Session) -> Option<&Object> {
    let selection = dtool_core::table::selection::current(session);
    let object = session.document.get_object(selection.table.or_else(|| session.selection.ids.first().copied())?)?;
    (matches!(object.kind, ObjectKind::Table(_)) && dtool_core::scene::is_drawn_with_flags(&session.document, &session.view.canvas_flags, object.id)).then_some(object)
}

const HEADER_SIZE: f32 = 18.0;

#[derive(Clone, Copy)]
enum Header {
    Row(usize),
    Column(usize),
}

impl Header {
    fn index(self) -> u32 {
        match self { Self::Row(index) | Self::Column(index) => index as u32 }
    }

    fn rows(self) -> bool { matches!(self, Self::Row(_)) }
}

fn selected_headers(selection: &selection::TableSelection, rows: bool) -> Option<&[u32]> {
    use selection::TrackSelection;
    match (&selection.tracks, rows) {
        (Some(TrackSelection::Rows(indices)), true) | (Some(TrackSelection::Columns(indices)), false) => Some(indices),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct TrackDrag {
    table: ObjectId,
    axis: usize,
    index: usize,
    start: f32,
    edge: f32,
    size: f32,
    version: u64,
}

impl TrackDrag {
    fn size_at(self, doc: &dtool_core::Document, point: [f32; 2]) -> Option<f32> {
        if doc.version != self.version { return None; }
        let object = doc.get_object(self.table)?;
        let local = object_local(object, point)?;
        let min_size = dtool_core::units::units_per_point(doc.units, doc.dpi).min(self.size);
        Some((self.size + local[self.axis] - self.start).max(min_size))
    }
}

#[derive(Clone, Copy)]
struct CountDrag {
    table: ObjectId,
    axis: usize,
    start: f32,
    version: u64,
}

impl CountDrag {
    fn delta(self, doc: &dtool_core::Document, point: [f32; 2]) -> Option<i32> {
        if doc.version != self.version { return None; }
        let object = doc.get_object(self.table)?;
        let ObjectKind::Table(table) = &object.kind else { return None; };
        let local = object_local(object, point)?;
        let tracks = if self.axis == 0 { &table.col_widths_pt } else { &table.row_heights_pt };
        let step = *tracks.last()?;
        let count = tracks.len() as i32;
        let distance = local[self.axis] - self.start;
        if distance >= 0.0 {
            Some(((distance / step).round() as i32).min(dtool_core::table::commands::MAX_ROWS_COLS as i32 - count))
        } else {
            let mut removed = 0;
            let mut offset = -distance;
            for &length in tracks.iter().rev().take(tracks.len().saturating_sub(1)) {
                if offset < length * 0.5 { break; }
                offset -= length;
                removed += 1;
            }
            Some(-removed)
        }
    }

    fn command(self, session: &Session, delta: i32) -> Option<Command> {
        let ObjectKind::Table(table) = &session.document.get_object(self.table)?.kind else { return None; };
        let (axis, field, count) = if self.axis == 0 {
            ("column", "col", table.col_widths_pt.len())
        } else {
            ("row", "row", table.row_heights_pt.len())
        };
        match delta {
            1.. => {
                let mut params = json!({"id": self.table.0, "count": delta, "match_neighbor": true, "inherit_format": true});
                params[field] = json!(count);
                Some(Command::with_params(&format!("table.{axis}.insert"), params))
            }
            ..=-1 => {
                let mut params = json!({"id": self.table.0});
                params[if self.axis == 0 { "cols" } else { "rows" }] =
                    json!(((count - delta.unsigned_abs() as usize)..count).collect::<Vec<_>>());
                Some(Command::with_params(&format!("table.{axis}.delete"), params))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum HeaderHit {
    Select(ObjectId, Header),
    Resize(TrackDrag),
    Count(CountDrag),
}

fn object_local(object: &Object, point: [f32; 2]) -> Option<[f32; 2]> {
    let inverse = dtool_core::geometry::invert_matrix(&dtool_core::geometry::object_matrix(object))?;
    let [x, y] = dtool_core::geometry::apply_matrix(&inverse, point);
    Some([x - object.pos[0], y - object.pos[1]])
}

fn header_hit(session: &Session, view: &ViewState, point: [f32; 2]) -> Option<HeaderHit> {
    if view.canvas_flags.preview_mode { return None; }
    let object = selected_table(session)?;
    let ObjectKind::Table(table) = &object.kind else { return None; };
    if object.locked || !object.pickable { return None; }
    let local = object_local(object, point)?;
    let grid = table_grid(table);
    let zoom = view.zoom.max(1e-4);
    let margin = HEADER_SIZE / zoom;
    let (axis, edges, sizes) = if (-margin..0.0).contains(&local[1]) {
        (0, &grid.col_x, &table.col_widths_pt)
    } else if (-margin..0.0).contains(&local[0]) {
        (1, &grid.row_y, &table.row_heights_pt)
    } else {
        return None;
    };
    let at = local[axis];
    let end = *edges.last()?;
    if (end..end + margin).contains(&at) {
        return Some(HeaderHit::Count(CountDrag { table: object.id, axis, start: at, version: session.document.version }));
    }
    let boundary = edges.partition_point(|edge| *edge < at);
    let near = [boundary.saturating_sub(1), boundary].into_iter()
        .filter(|index| *index > 0 && *index < edges.len())
        .min_by(|a, b| (edges[*a] - at).abs().total_cmp(&(edges[*b] - at).abs()));
    if let Some(edge) = near.filter(|index| (edges[*index] - at).abs() <= 4.0 / zoom) {
        let index = edge - 1;
        return Some(HeaderHit::Resize(TrackDrag {
            table: object.id, axis, index, start: at, edge: edges[edge],
            size: sizes[index], version: session.document.version,
        }));
    }
    let index = edges.windows(2).position(|pair| pair[0] <= at && at < pair[1])?;
    let header = if axis == 0 { Header::Column(index) } else { Header::Row(index) };
    Some(HeaderHit::Select(object.id, header))
}

#[derive(Clone, Copy)]
pub(crate) struct HeaderMenu {
    table: ObjectId,
    header: Header,
    version: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum TableMenu {
    Header(HeaderMenu),
    Cells { table: ObjectId, cell: CellId, version: u64 },
}

pub(crate) fn menu_target(session: &Session, rect: Rect, pointer: Pos2) -> Option<TableMenu> {
    if session.active_tool != "tool.table" { return None; }
    let view = &session.view;
    let point = dtool_core::view_to_doc(view, [pointer.x - rect.min.x, pointer.y - rect.min.y]);
    let Some(header) = header_hit(session, view, point) else {
        let object = selected_table(session)?;
        if object.locked || !object.pickable { return None; }
        let ObjectKind::Table(table) = &object.kind else { return None; };
        let cell = CellId(cell_at(&table_grid(table), object_local(object, point)?)?);
        return Some(TableMenu::Cells { table: object.id, cell, version: session.document.version });
    };
    let (table, header) = match header {
        HeaderHit::Select(id, header) => (id, header),
        HeaderHit::Resize(drag) => {
            let header = if drag.axis == 0 { Header::Column(drag.index) } else { Header::Row(drag.index) };
            (drag.table, header)
        }
        HeaderHit::Count(_) => return None,
    };
    Some(TableMenu::Header(HeaderMenu { table, header, version: session.document.version }))
}

pub(crate) fn show_menu(ui: &mut Ui, session: &mut Session, target: TableMenu) -> Option<Result<(), CommandError>> {
    match target {
        TableMenu::Header(header) => header_menu(ui, session, header),
        TableMenu::Cells { table, cell, version } => {
            let selection = selection::current(session);
            let cells = if selection.table == Some(table) && selection.cells.contains(&cell) { selection.cells } else { vec![cell] };
            for (name, label) in [
                (merge::MERGE, "menu.text.table.merge_cells"),
                (merge::UNMERGE, "menu.text.table.separate_cells"),
            ] {
                let command = Command::with_params(name, json!({"id": table.0, "cells": cells}));
                let enabled = version == session.document.version && session.can_execute(&command);
                if ui.add_enabled(enabled, egui::Button::new(dtool_strings::text(label))).clicked() {
                    ui.close();
                    text_input::finish_text_input(session);
                    return Some(session.run(&command).map(|_| ()));
                }
            }
            None
        }
    }
}

fn header_menu(ui: &mut Ui, session: &mut Session, target: HeaderMenu) -> Option<Result<(), CommandError>> {
    let object = session.document.get_object(target.table)?;
    let ObjectKind::Table(table) = &object.kind else { return None; };
    let (axis, field, list, count, before, after, delete, distribute, distribute_label) = match target.header {
        Header::Row(_) => ("row", "row", "rows", table.row_heights_pt.len(), "ctx.table.insert_above", "ctx.table.insert_below", "menu.text.table.delete_row_s", "table.rows.distribute", "menu.text.table.evenly_distribute_rows"),
        Header::Column(_) => ("column", "col", "cols", table.col_widths_pt.len(), "ctx.table.insert_left", "ctx.table.insert_right", "menu.text.table.delete_column_s", "table.columns.distribute", "menu.text.table.evenly_distribute_columns"),
    };
    let index = target.header.index() as usize;
    let valid = !object.locked && index < count && target.version == session.document.version;
    let can_insert = valid && count < dtool_core::table::commands::MAX_ROWS_COLS as usize;
    let selection = selection::current(session);
    let single = [index as u32];
    let tracks = selected_headers(&selection, target.header.rows())
        .filter(|indices| selection.table == Some(target.table) && indices.contains(&single[0])).unwrap_or(&single);
    let mut delete_params = json!({ "id": target.table.0, "select": true });
    delete_params[list] = json!(tracks);
    let mut distribute_params = json!({ "id": target.table.0 });
    distribute_params[list] = json!(tracks);
    let autofit = Command::with_params(&format!("table.{axis}.autofit"), distribute_params.clone());
    let autofit_label = if target.header.rows() { "menu.text.table.autofit_rows" } else { "menu.text.table.autofit_columns" };
    let ascending = super::menu::sort_command(target.table, !target.header.rows(), index as u32, false);
    let descending = super::menu::sort_command(target.table, !target.header.rows(), index as u32, true);
    let insert = |at| {
        let mut params = json!({ "id": target.table.0, "select": true, "match_neighbor": true });
        params[field] = json!(at);
        Command::with_params(&format!("table.{axis}.insert"), params)
    };
    for (key, enabled, command) in [
        (before, can_insert, insert(index)),
        (after, can_insert, insert(index + 1)),
        (delete, valid && tracks.len() < count, Command::with_params(&format!("table.{axis}.delete"), delete_params)),
        (distribute_label, valid && tracks.len() > 1, Command::with_params(distribute, distribute_params)),
        (autofit_label, valid && session.can_execute(&autofit), autofit),
        ("table.sort.ascending", valid && session.can_execute(&ascending), ascending),
        ("table.sort.descending", valid && session.can_execute(&descending), descending),
    ] {
        if ui.add_enabled(enabled, egui::Button::new(dtool_strings::text(key))).clicked() {
            ui.close();
            text_input::finish_text_input(session);
            return Some(session.run(&command).map(|_| ()));
        }
    }
    None
}

fn paint_track_preview(ui: &Ui, response: &Response, session: &Session, view: &ViewState, drag: TrackDrag, size: f32) {
    let Some(object) = session.document.get_object(drag.table) else { return; };
    let edge = drag.edge + size - drag.size;
    let margin = HEADER_SIZE / view.zoom.max(1e-4);
    let ends = if drag.axis == 0 { [[edge, -margin], [edge, object.size[1].abs()]] }
        else { [[-margin, edge], [object.size[0].abs(), edge]] };
    ui.painter_at(response.rect).line_segment(
        ends.map(|point| screen(response.rect, view, object_point(object, point))),
        egui::Stroke::new(1.5, dtool_design::color("accent")),
    );
}

fn resize_cursor(axis: usize) -> egui::CursorIcon {
    if axis == 0 { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical }
}
fn column_name(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(char::from(b'A' + (index % 26) as u8));
        if index < 26 { break; }
        index = index / 26 - 1;
    }
    name.into_iter().rev().collect()
}

fn paint_grid(ui: &Ui, response: &Response, session: &Session, view: &ViewState) {
    if view.canvas_flags.preview_mode { return; }
    let Some(object) = selected_table(session) else { return; };
    let ObjectKind::Table(table) = &object.kind else { return; };
    let selection = dtool_core::table::selection::current(session);
    let grid = table_grid(table);
    let painter = ui.painter_at(response.rect);
    let accent = dtool_design::color("accent");
    let points = |[l, t, r, b]: [f32; 4]| {
        [[l, t], [r, t], [r, b], [l, b]].map(|point| screen(response.rect, view, object_point(object, point)))
    };
    let selected: Vec<&CellRect> = grid.rects.iter().filter(|cell| selection.cells.contains(&CellId(cell.id))).collect();
    for cell in &selected {
        painter.add(egui::Shape::convex_polygon(points(cell.rect).to_vec(), accent.gamma_multiply(0.12), egui::Stroke::NONE));
    }
    // Shared edges are drawn once, including the segments next to merged cells.
    let mut edges = std::collections::HashMap::new();
    for cell in &grid.rects {
        let active = selection.cells.contains(&CellId(cell.id));
        let row_end = cell.row + cell.row_span;
        let col_end = cell.col + cell.col_span;
        for col in cell.col..col_end {
            *edges.entry((true, col, cell.row)).or_insert(false) |= active;
            *edges.entry((true, col, row_end)).or_insert(false) |= active;
        }
        for row in cell.row..row_end {
            *edges.entry((false, cell.col, row)).or_insert(false) |= active;
            *edges.entry((false, col_end, row)).or_insert(false) |= active;
        }
    }
    let grid_stroke = egui::Stroke::new(1.0, accent.gamma_multiply(0.45));
    let selected_stroke = egui::Stroke::new(1.5, accent);
    for active in [false, true] {
        for (&(horizontal, col, row), &selected) in &edges {
            if active != selected { continue; }
            let (col, row) = (col as usize, row as usize);
            let segment = [
                [grid.col_x[col], grid.row_y[row]],
                [grid.col_x[col + usize::from(horizontal)], grid.row_y[row + usize::from(!horizontal)]],
            ];
            let segment = segment.map(|point| screen(response.rect, view, object_point(object, point)));
            painter.line_segment(segment, if active { selected_stroke } else { grid_stroke });
        }
    }
    if object.locked || !object.pickable { return; }
    let margin = HEADER_SIZE / view.zoom.max(1e-4);
    let header = |rect, label: String, active| {
        let corners = points(rect);
        let bg = if active { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.inactive.bg_fill };
        painter.add(egui::Shape::convex_polygon(corners.to_vec(), bg, egui::Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color)));
        painter.text(corners[0].lerp(corners[2], 0.5), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(10.0), ui.visuals().text_color());
    };
    let columns = selection.track_indices(table, false);
    let rows = selection.track_indices(table, true);
    for (i, edge) in grid.col_x.windows(2).enumerate() {
        let active = columns.binary_search(&(i as u32)).is_ok();
        header([edge[0], -margin, edge[1], 0.0], column_name(i), active);
    }
    for (i, edge) in grid.row_y.windows(2).enumerate() {
        let active = rows.binary_search(&(i as u32)).is_ok();
        header([-margin, edge[0], 0.0, edge[1]], (i + 1).to_string(), active);
    }
    if let (Some(&right), Some(&bottom)) = (grid.col_x.last(), grid.row_y.last()) {
        header([right, -margin, right + margin, 0.0], "↔".into(), false);
        header([-margin, bottom, 0.0, bottom + margin], "↕".into(), false);
    }
}

fn table_count(session: &Session, option: &str) -> u32 {
    session.tool_options.number("tool.table", option).unwrap_or(3.0) as u32
}

fn autofit_handles(ui: &mut Ui, canvas: &Response, session: &Session, view: &ViewState) -> (bool, Option<Command>) {
    let Some(object) = selected_table(session).filter(|object| !view.canvas_flags.preview_mode && !object.locked && object.pickable) else {
        return (false, None);
    };
    let ObjectKind::Table(table) = &object.kind else { return (false, None); };
    let grid = table_grid(table);
    let (Some(&width), Some(&height)) = (grid.col_x.last(), grid.row_y.last()) else { return (false, None); };
    let offset = 9.0 / view.zoom.max(1e-4);
    let mut consumed = false;
    for (axis, point, size, tooltip) in [
        ("column", [width + offset, height / 2.0], egui::vec2(5.0, 13.0), "menu.text.table.autofit_columns"),
        ("row", [width / 2.0, height + offset], egui::vec2(13.0, 5.0), "menu.text.table.autofit_rows"),
    ] {
        let center = screen(canvas.rect, view, object_point(object, point));
        if !canvas.rect.contains(center) { continue; }
        let mark = Rect::from_center_size(center, size);
        let handle = ui.interact(mark.expand(3.0), canvas.id.with(("table-autofit", object.id, axis)), egui::Sense::click())
            .on_hover_text(dtool_strings::text(tooltip));
        if handle.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
        let fill = if handle.hovered() { ui.visuals().widgets.hovered.fg_stroke.color } else { ui.visuals().widgets.inactive.fg_stroke.color };
        ui.painter_at(canvas.rect).rect_filled(mark, 2.0, fill);
        if handle.double_clicked() {
            return (true, Some(Command::with_params(&format!("table.{axis}.autofit"), json!({"id": object.id.0}))));
        }
        consumed |= handle.is_pointer_button_down_on() || handle.clicked();
    }
    (consumed, None)
}

pub fn handle(ui: &mut Ui, response: &Response, session: &mut Session, scene: &Scene, view: &ViewState, context: Option<u64>) -> Result<(), CommandError> {
    let press_id = response.id.with(("table-press", context));
    let anchor_id = response.id.with(("table-anchor", context));
    let header_anchor_id = response.id.with(("table-header-anchor", context));
    let resize_id = response.id.with(("table-resize", context));
    let count_id = response.id.with(("table-count", context));
    type CellAnchor = (ObjectId, CellId);
    type TablePress = (Pos2, bool, Option<CellAnchor>);
    let keyboard = ui.memory(|memory| memory.focused().is_none_or(|id| id == response.id));
    if keyboard && !egui::Popup::is_any_open(ui.ctx()) && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        ui.data_mut(|data| {
            data.remove::<TablePress>(press_id);
            data.remove::<TrackDrag>(resize_id);
            data.remove::<CountDrag>(count_id);
        });
        text_input::finish_text_input(session);
        ui.ctx().request_repaint();
        paint_grid(ui, response, session, view);
        return Ok(());
    }

    let to_doc = |pos: Pos2| dtool_core::view_to_doc(view, [pos.x - response.rect.min.x, pos.y - response.rect.min.y]);
    if let Some(pos) = response.hover_pos() {
        let point = to_doc(pos);
        let cursor = match header_hit(session, view, point) {
            Some(HeaderHit::Resize(drag)) => resize_cursor(drag.axis),
            Some(HeaderHit::Count(drag)) => resize_cursor(drag.axis),
            Some(HeaderHit::Select(..)) => egui::CursorIcon::PointingHand,
            None if hit(session, scene, point).is_some() => egui::CursorIcon::Text,
            None => egui::CursorIcon::Crosshair,
        };
        ui.ctx().set_cursor_icon(cursor);
    }
    let (on_handle, autofit) = autofit_handles(ui, response, session, view);
    if on_handle {
        if let Some(command) = autofit {
            text_input::finish_text_input(session);
            session.run(&command)?;
            response.request_focus();
            ui.ctx().request_repaint();
        }
        paint_grid(ui, response, session, view);
        return Ok(());
    }
    let (pressed, released) = crate::input::primary_button_positions(ui);
    if let Some(press) = pressed.filter(|point| response.interact_rect.contains(*point)) {
        let start = to_doc(press);
        let header = header_hit(session, view, start);
        let cell = if header.is_none() { hit(session, scene, start) } else { None };
        let new_table = header.is_none() && cell.is_none();
        let range = cell.filter(|hit| session.text_edit.as_ref().is_none_or(|edit| edit.story != hit.story))
            .map(|hit| (hit.table, hit.cell));
        ui.data_mut(|data| {
            data.remove::<TrackDrag>(resize_id);
            data.insert_temp(press_id, (press, new_table, range));
            data.remove::<CountDrag>(count_id);
            match header {
                Some(HeaderHit::Resize(drag)) => { data.insert_temp(resize_id, drag); }
                Some(HeaderHit::Count(drag)) => { data.insert_temp(count_id, drag); }
                _ => {}
            }
        });
    }
    if let Some(drag) = ui.data(|data| data.get_temp::<CountDrag>(count_id)) {
        let pointer = released.or_else(|| ui.input(|input| input.pointer.latest_pos()));
        let delta = pointer.and_then(|pos| drag.delta(&session.document, to_doc(pos))).unwrap_or(0);
        if released.is_some() || !ui.input(|input| input.pointer.primary_down()) {
            ui.data_mut(|data| {
                data.remove::<CountDrag>(count_id);
                data.remove::<TablePress>(press_id);
            });
            if let Some(command) = released.and_then(|_| drag.command(session, delta)) {
                text_input::finish_text_input(session);
                session.run(&command)?;
                response.request_focus();
            }
        } else if let Some(pointer) = pointer {
            ui.ctx().set_cursor_icon(resize_cursor(drag.axis));
            let Some(object) = session.document.get_object(drag.table) else { return Ok(()); };
            let ObjectKind::Table(table) = &object.kind else { return Ok(()); };
            let (tracks, other) = if drag.axis == 0 {
                (&table.col_widths_pt, &table.row_heights_pt)
            } else {
                (&table.row_heights_pt, &table.col_widths_pt)
            };
            let painter = ui.painter_at(response.rect);
            let accent = dtool_design::color("accent");
            let extent = tracks.iter().sum::<f32>();
            let cross = other.iter().sum::<f32>();
            let line = |along, start, end| {
                [start, end].map(|value| {
                    let local = if drag.axis == 0 { [along, value] } else { [value, along] };
                    screen(response.rect, view, object_point(object, local))
                })
            };
            let count = (tracks.len() as i32 + delta) as usize;
            if delta > 0 && let Some(&step) = tracks.last() {
                let ink = egui::Stroke::new(1.0, accent);
                let end = extent + step * delta as f32;
                for index in 0..=delta {
                    painter.line_segment(line(extent + index as f32 * step, 0.0, cross), ink);
                }
                let mut cursor = 0.0;
                for track in std::iter::once(&0.0).chain(other) {
                    cursor += track;
                    painter.line_segment([line(extent, cursor, cursor)[0], line(end, cursor, cursor)[0]], ink);
                }
            } else if delta < 0 {
                let edge = tracks[..count].iter().sum();
                let [a, b] = line(edge, 0.0, cross);
                let [c, d] = line(extent, 0.0, cross);
                painter.add(egui::Shape::convex_polygon(vec![a, b, d, c], accent.gamma_multiply(0.20), egui::Stroke::new(1.0, accent)));
            }
            painter.text(pointer + egui::vec2(10.0, 12.0), egui::Align2::LEFT_TOP,
                count.to_string(), egui::FontId::proportional(12.0), accent);
        }
        paint_grid(ui, response, session, view);
        ui.ctx().request_repaint();
        return Ok(());
    }
    let press = ui.data(|data| data.get_temp::<TablePress>(press_id));
    if let Some(drag) = ui.data(|data| data.get_temp::<TrackDrag>(resize_id)) {
        let pointer = released.or_else(|| ui.input(|input| input.pointer.latest_pos()));
        let size = pointer.and_then(|pos| drag.size_at(&session.document, to_doc(pos)));
        if released.is_some() || !ui.input(|input| input.pointer.primary_down()) {
            ui.data_mut(|data| {
                data.remove::<TrackDrag>(resize_id);
                data.remove::<TablePress>(press_id);
            });
            if let Some(size) = size.filter(|size| released.is_some() && (*size - drag.size).abs() * view.zoom >= 1.0) {
                text_input::finish_text_input(session);
                let axis = if drag.axis == 0 { "column" } else { "row" };
                session.run(&Command::with_params("table.track.resize", json!({
                    "id": drag.table.0, "axis": axis, "index": drag.index, "size": size
                })))?;
                response.request_focus();
                ui.ctx().request_repaint();
            }
            paint_grid(ui, response, session, view);
        } else {
            paint_grid(ui, response, session, view);
            if let Some(size) = size {
                ui.ctx().set_cursor_icon(resize_cursor(drag.axis));
                paint_track_preview(ui, response, session, view, drag, size);
            }
        }
        return Ok(());
    }
    let editing = active_text(session).is_some();
    if editing {
        text_input::text_input(ui, session, response, view);
    }
    if let Some(pos) = released.or_else(|| response.interact_pointer_pos()) {
        let snap = |point| crate::canvas_tools::snap_point(session, to_doc(point), &[]).0;
        if let Some((start, true, _)) = press.filter(|_| response.dragged() && released.is_none()) {
            let start = snap(start);
            let end = snap(pos);
            let painter = ui.painter_at(response.rect);
            let stroke = egui::Stroke::new(1.0, dtool_design::color("accent"));
            for (axis, option) in [dtool_core::context_toolbar::TABLE_COLUMNS, dtool_core::context_toolbar::TABLE_ROWS].into_iter().enumerate() {
                let count = table_count(session, option).max(1);
                for index in 0..=count {
                    let value = start[axis] + (end[axis] - start[axis]) * index as f32 / count as f32;
                    let mut a = start;
                    let mut b = end;
                    a[axis] = value;
                    b[axis] = value;
                    painter.line_segment([screen(response.rect, view, a), screen(response.rect, view, b)], stroke);
                }
            }
        }
        let create = press.filter(|(start, blank, _)| {
            let delta = pos - *start;
            *blank && (response.drag_stopped() || released.is_some()) && delta.x.abs() >= 8.0 && delta.y.abs() >= 8.0
        });
        let range = press.and_then(|(start, _, anchor)| anchor.filter(|_| {
            (response.dragged() || released.is_some()) && (pos - start).length_sq() >= 25.0
        }));
        if let Some((table, start)) = range {
            if let Some(end) = hit(session, scene, to_doc(pos)).filter(|end| end.table == table) {
                text_input::finish_text_input(session);
                select_range(session, table, start, end.cell)?;
                ui.data_mut(|data| data.insert_temp(anchor_id, (table, start)));
                response.request_focus();
                ui.ctx().request_repaint();
            }
        } else if let Some((start, _, _)) = create {
            let start = snap(start);
            let end = snap(pos);
            let size = [(end[0] - start[0]).abs(), (end[1] - start[1]).abs()];
            if size.contains(&0.0) { return Ok(()); }
            text_input::finish_text_input(session);
            session.run(&Command::with_params("table.create", json!({
                "rows": table_count(session, dtool_core::context_toolbar::TABLE_ROWS),
                "cols": table_count(session, dtool_core::context_toolbar::TABLE_COLUMNS),
                "bounds": size, "pos": [start[0].min(end[0]), start[1].min(end[1])], "visible_grid": true
            })))?;
            response.request_focus();
            ui.ctx().request_repaint();
        } else if response.clicked() || response.double_clicked() {
            if let Some(HeaderHit::Select(id, header)) = header_hit(session, view, to_doc(pos)) {
                text_input::finish_text_input(session);
                ui.data_mut(|data| data.remove::<CellAnchor>(anchor_id));
                let (shift, toggle) = ui.input(|input| (input.modifiers.shift, input.modifiers.command));
                type HeaderAnchor = (ObjectId, u64, Header);
                let version = session.document.version;
                let selection = selection::current(session);
                let index = header.index();
                let selected = selected_headers(&selection, header.rows()).filter(|_| selection.table == Some(id));
                let start = ui.data(|data| data.get_temp::<HeaderAnchor>(header_anchor_id))
                    .filter(|(table, saved, start)| *table == id && *saved == version && start.rows() == header.rows() && selected.is_some())
                    .map(|(_, _, start)| start.index());
                let indices = if let Some(start) = start.filter(|_| shift) {
                    (start.min(index)..=start.max(index)).collect()
                } else {
                    ui.data_mut(|data| data.insert_temp(header_anchor_id, (id, version, header)));
                    if toggle {
                        let mut indices = selected.unwrap_or_default().to_vec();
                        if let Ok(at) = indices.binary_search(&index) { indices.remove(at); }
                        else { indices.push(index); }
                        indices
                    } else { vec![index] }
                };
                let request = match header {
                    Header::Row(_) => json!({ "id": id.0, "mode": "rows", "rows": indices }),
                    Header::Column(_) => json!({ "id": id.0, "mode": "columns", "columns": indices }),
                };
                session.run(&Command::with_params("table.selection.set", request))?;
                response.request_focus();
            } else if let Some(target) = hit(session, scene, to_doc(pos)) {
                let modifiers = ui.input(|input| input.modifiers);
                let same_story = session.text_edit.as_ref().is_some_and(|edit| edit.story == target.story);
                if same_story {
                    edit_cell(session, target.table, target.story, caret_at(session, &target))?;
                } else {
                    text_input::finish_text_input(session);
                    let current = selection::current(session);
                    let cells = if current.table == Some(target.table) { current.cells } else { Vec::new() };
                    if modifiers.shift && !cells.is_empty() {
                        let anchor = ui.data(|data| data.get_temp::<CellAnchor>(anchor_id))
                            .filter(|(table, cell)| *table == target.table && cells.contains(cell))
                            .map_or(cells[0], |(_, cell)| cell);
                        select_range(session, target.table, anchor, target.cell)?;
                    } else if modifiers.command {
                        let mut cells = cells;
                        if let Some(index) = cells.iter().position(|cell| *cell == target.cell) {
                            cells.remove(index);
                        } else {
                            cells.push(target.cell);
                        }
                        select_cells(session, target.table, &cells)?;
                    } else {
                        select_cells(session, target.table, &[target.cell])?;
                        ui.data_mut(|data| data.insert_temp(anchor_id, (target.table, target.cell)));
                        if response.double_clicked() {
                            edit_cell(session, target.table, target.story, caret_at(session, &target))?;
                        }
                    }
                }
                response.request_focus();
            } else {
                text_input::finish_text_input(session);
                session.run(&Command::with_params("selection.set", json!({ "ids": [] })))?;
            }
            ui.ctx().request_repaint();
        } else if response.dragged() {
            if let Some((start, true, _)) = press {
                dtool_design::canvas_frame_outline(ui, Rect::from_two_pos(start, pos));
            }
        }
    }
    if released.is_some() {
        ui.data_mut(|data| data.remove::<TablePress>(press_id));
    }
    if keyboard && !editing && session.text_edit.is_none() {
        let (enter, typing) = ui.input(|input| (
            input.key_pressed(egui::Key::Enter),
            starts_cell_text(input),
        ));
        if (enter || typing) && begin_selected_cell_edit(session)? {
            response.request_focus();
            if typing { text_input::text_input(ui, session, response, view); }
            ui.ctx().request_repaint();
        }
    }
    paint_grid(ui, response, session, view);
    Ok(())
}
