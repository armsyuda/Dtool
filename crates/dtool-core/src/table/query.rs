#![allow(dead_code)]

use serde_json::{json, Value};
use std::collections::HashMap;
use crate::document::Document;
use crate::object::ObjectId;
use crate::table::model::{CellId, EdgeStyle, StoryId};
use crate::table::resolve::{resolve_cell, resolve_frame};
use crate::table::geometry::table_grid;

fn column_letter(col: u32) -> String {
    let mut n = col + 1;
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        let c = (n % 26) as u8 + b'A';
        s.push(c as char);
        n /= 26;
    }
    s.chars().rev().collect()
}

fn cell_address(row: u32, col: u32) -> String {
    format!("{}{}", column_letter(col), row + 1)
}

fn edge_value(edge: &Option<EdgeStyle>) -> Value {
    json!(edge)
}

fn story_text(doc: &Document, story_id: StoryId) -> String {
    doc.stories
        .iter()
        .find(|s| s.id == story_id)
        .map(|s| s.text.clone())
        .unwrap_or_default()
}

fn footer_rows(row_count: usize, footer: u16) -> Vec<u16> {
    let n = row_count as u16;
    let start = n.saturating_sub(footer);
    (start..n).collect()
}

fn owner_id(cell: &crate::table::model::TableCell) -> CellId {
    cell.merged_into.unwrap_or(cell.id)
}

fn cell_rect(grid: &crate::table::geometry::TableGrid, id: CellId) -> Option<[f32; 4]> {
    grid.rects.iter().find(|r| r.id == id.0).map(|r| r.rect)
}

fn overflow_of(id: CellId, overflows: Option<&HashMap<CellId, bool>>) -> bool {
    overflows.and_then(|map| map.get(&id)).copied().unwrap_or(false)
}

pub fn table_query(doc: &Document, object_id: ObjectId, cell_filter: Option<CellId>) -> Value {
    table_query_with_overflows(doc, object_id, cell_filter, None)
}

pub fn table_query_with_overflows(
    doc: &Document,
    object_id: ObjectId,
    cell_filter: Option<CellId>,
    overflows: Option<&HashMap<CellId, bool>>,
) -> Value {
    let obj = match doc.objects.iter().find(|o| o.id == object_id) {
        Some(o) => o,
        None => return json!({}),
    };

    let table = match &obj.kind {
        crate::object::ObjectKind::Table(t) => t,
        _ => return json!({}),
    };

    let grid = table_grid(table);
    let frame_fmt = resolve_frame(doc, table);
    let generation = doc.version;
    let frame_baseline = frame_fmt.baseline.as_ref().map(|b| {
        json!({
            "enabled": b.enabled,
            "start_pt": b.start_pt,
            "relative_to": format!("{:?}", b.relative_to),
            "spacing_pt": b.spacing_pt,
            "color": b.color
        })
    });

    let cells_json: Vec<Value> = table.cells.iter().filter_map(|cell| {
        if let Some(filter) = cell_filter {
            if cell.id != filter {
                return None;
            }
        }

        let fmt = resolve_cell(doc, table, cell);
        let owner = owner_id(cell);
        let rect = cell_rect(&grid, owner);
        let text = story_text(doc, cell.story);
        let overflow = overflow_of(owner, overflows);

        let mut cell_json = json!({
            "id": format!("c{}", cell.id.0),
            "address": cell_address(cell.row, cell.col),
            "row": cell.row,
            "col": cell.col,
            "row_span": cell.row_span,
            "col_span": cell.col_span,
            "merged_into": cell.merged_into.map(|id| format!("c{}", id.0)),
            "story_id": cell.story.0,
            "text": text,
            "format": {
                "fill": fmt.fill,
                "edges": [
                    edge_value(&fmt.edges[0]),
                    edge_value(&fmt.edges[1]),
                    edge_value(&fmt.edges[2]),
                    edge_value(&fmt.edges[3])
                ],
                "insets": {
                    "left": fmt.insets.left,
                    "top": fmt.insets.top,
                    "right": fmt.insets.right,
                    "bottom": fmt.insets.bottom,
                    "linked": fmt.insets.linked
                },
                "vertical_align": format!("{:?}", fmt.vertical_align),
                "rotation": format!("{:?}", fmt.rotation),
                "initial_advance": format!("{:?}", fmt.initial_advance),
                "minimum_pt": fmt.minimum_pt,
                "paragraph_style": fmt.paragraph_style
            },
            "overflow": overflow
        });
        if let Some(rect) = rect {
            cell_json["rect"] = json!(rect);
        }
        Some(cell_json)
    }).collect();

    json!({
        "rows": table.row_heights_pt.len(),
        "cols": table.col_widths_pt.len(),
        "col_widths_pt": table.col_widths_pt,
        "row_heights_pt": table.row_heights_pt,
        "header_rows": (0..table.header_rows).collect::<Vec<u16>>(),
        "footer_rows": footer_rows(table.row_heights_pt.len(), table.footer_rows),
        "high_water": table.alloc.high_water,
        "format_id": table.format,
        "generation": generation,
        "frame": {
            "fill": frame_fmt.fill,
            "edges": [
                edge_value(&frame_fmt.edges[0]),
                edge_value(&frame_fmt.edges[1]),
                edge_value(&frame_fmt.edges[2]),
                edge_value(&frame_fmt.edges[3])
            ],
            "insets": {
                "left": frame_fmt.insets.left,
                "top": frame_fmt.insets.top,
                "right": frame_fmt.insets.right,
                "bottom": frame_fmt.insets.bottom,
                "linked": frame_fmt.insets.linked
            },
            "ignore_baseline_grid": frame_fmt.ignore_baseline_grid,
            "baseline": frame_baseline
        },
        "cells": cells_json
    })
}

pub fn attach_layout_overflow(query: &mut Value, overflows: &HashMap<CellId, bool>) {
    let Some(cells) = query.get_mut("cells").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for cell in cells {
        let owner = cell
            .get("merged_into")
            .and_then(|v| v.as_str())
            .and_then(|s| s.strip_prefix('c')?.parse::<u64>().ok())
            .or_else(|| {
                cell.get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.strip_prefix('c')?.parse::<u64>().ok())
            });
        if let Some(id) = owner {
            if let Some(flag) = overflows.get(&CellId(id)) {
                cell["overflow"] = json!(*flag);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellLine {
    pub range: [usize; 2],
    pub baseline: f32,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub glyphs: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellLines {
    pub content_rect: [f32; 4],
    pub rotation_deg: f32,
    pub overflow: bool,
    pub lines: Vec<CellLine>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableLines {
    pub font_generation: u64,
    pub cells: HashMap<CellId, CellLines>,
}

pub type TableLinesProvider = fn(&Document, ObjectId) -> Option<TableLines>;

static TABLE_LINES: std::sync::OnceLock<TableLinesProvider> = std::sync::OnceLock::new();

pub fn register_table_lines(provider: TableLinesProvider) -> bool {
    TABLE_LINES.set(provider).is_ok() || TABLE_LINES.get().is_some_and(|current| *current as usize == provider as usize)
}

pub fn table_lines(doc: &Document, object_id: ObjectId) -> Option<TableLines> {
    TABLE_LINES.get().and_then(|provider| provider(doc, object_id))
}

pub fn cell_lines_value(cell: &CellLines) -> Value {
    json!({
        "content_rect": cell.content_rect,
        "rotation_deg": cell.rotation_deg,
        "overflow": cell.overflow,
        "lines": cell.lines.iter().map(|line| json!({
            "range": line.range,
            "baseline": line.baseline,
            "width": line.width,
            "ascent": line.ascent,
            "descent": line.descent,
            "glyphs": line.glyphs
        })).collect::<Vec<Value>>()
    })
}

fn query_cell_id(value: Option<&Value>) -> Option<u64> {
    value.and_then(|v| v.as_str()).and_then(|s| s.strip_prefix('c')?.parse::<u64>().ok())
}

pub fn attach_layout_lines(query: &mut Value, lines: &TableLines) {
    let Some(object) = query.as_object_mut() else {
        return;
    };
    if !object.contains_key("cells") {
        return;
    }
    object.insert("font_generation".to_string(), json!(lines.font_generation));
    let Some(cells) = object.get_mut("cells").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for cell in cells {
        let id = query_cell_id(cell.get("id"));
        let owner = query_cell_id(cell.get("merged_into"));
        let layout = owner.or(id).and_then(|key| lines.cells.get(&CellId(key)));
        if let Some(layout) = layout {
            cell["overflow"] = json!(layout.overflow);
        }
        cell["layout"] = match (owner, layout) {
            (None, Some(layout)) => cell_lines_value(layout),
            _ => Value::Null,
        };
    }
}

pub fn table_query_with_lines(doc: &Document, object_id: ObjectId, cell_filter: Option<CellId>, lines: Option<&TableLines>) -> Value {
    let mut value = table_query(doc, object_id, cell_filter);
    if let Some(lines) = lines {
        attach_layout_lines(&mut value, lines);
    }
    value
}
