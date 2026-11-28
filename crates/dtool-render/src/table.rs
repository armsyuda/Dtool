use std::collections::HashMap;

use dtool_core::document::Document;
use dtool_core::object::{Object, ObjectKind};
use dtool_core::table::geometry::{table_grid, CellRect, TableGrid};
use dtool_core::table::model::{EdgeStyle, Insets, StoryId, TableCell};
use dtool_core::table::{resolve_cell, resolve_frame};

use dtool_core::paint::{project_table_paint, range_fill, Paint, PaintRole, StrokeAlign};

use crate::display_list::{DisplayItem, DisplayList, TableEdgePattern, IDENTITY_MATRIX};

pub fn emit_table(
    list: &mut DisplayList,
    doc: &Document,
    object: &Object,
    flags: &dtool_core::CanvasFlags,
) {
    let table = match &object.kind {
        ObjectKind::Table(t) => t,
        _ => return,
    };

    let grid = table_grid(table);
    let frame = resolve_frame(doc, table);
    let outline_only = matches!(
        flags.view_mode,
        dtool_core::ViewMode::Outline | dtool_core::ViewMode::Wireframe
    );
    let opacity = object.opacity;
    let units_per_point = dtool_core::units::units_per_point(doc.units, doc.dpi);
    let origin = object.pos;
    let frame_local = [0.0, 0.0, object.size[0], object.size[1]];
    let frame_paint = if outline_only { None } else { visible(project_table_paint(PaintRole::PictureFrame, Some(&frame), None, None, None)) };

    list.items.push(DisplayItem {
        object_id: object.id,
        bounds: [object.pos[0], object.pos[1], object.size[0], object.size[1]],
        fill: frame_paint,
        opacity,
        outline_only,
        role: "TableFrame".to_string(),
        matrix: IDENTITY_MATRIX,
        clip: None,
        points: Vec::new(),
        table_edge: None,
        label: None,
    });

    if let Some(bounds) = band_bounds(origin, &grid, 0, table.header_rows as u32) {
        list.items.push(DisplayItem {
            object_id: object.id,
            bounds,
            fill: None,
            opacity,
            outline_only,
            role: "TableHeader".to_string(),
            matrix: IDENTITY_MATRIX,
            clip: None,
            points: Vec::new(),
            table_edge: None,
            label: None,
        });
    }

    let footer_start = (grid.row_y.len().saturating_sub(1) as u32).saturating_sub(table.footer_rows as u32);
    if let Some(bounds) = band_bounds(origin, &grid, footer_start, table.footer_rows as u32) {
        list.items.push(DisplayItem {
            object_id: object.id,
            bounds,
            fill: None,
            opacity,
            outline_only,
            role: "TableFooter".to_string(),
            matrix: IDENTITY_MATRIX,
            clip: None,
            points: Vec::new(),
            table_edge: None,
            label: None,
        });
    }

    let mut cells: Vec<&TableCell> = table.cells.iter().filter(|cell| cell.merged_into.is_none()).collect();
    cells.sort_by_key(|cell| (cell.row, cell.col));
    let mut placed = Vec::with_capacity(cells.len());
    for cell in cells {
        let Some(cell_rect) = grid.rects.iter().find(|r| r.id == cell.id.0) else {
            continue;
        };
        let format = resolve_cell(doc, table, cell);
        let insets = dtool_core::table::resolve::text_insets(table, cell, format.insets, &frame);
        let content = cell_metrics(cell, cell_rect, &insets);
        let bounds = offset_xywh(origin, content);
        list.items.push(DisplayItem {
            object_id: object.id,
            bounds: offset_xywh(origin, cell_rect.rect),
            fill: if outline_only { None } else { visible(project_table_paint(PaintRole::Cell, None, Some(&format), None, None)) },
            opacity,
            outline_only,
            role: "TableCell".to_string(),
            matrix: IDENTITY_MATRIX,
            clip: None,
            points: Vec::new(),
            table_edge: None,
            label: None,
        });
        placed.push((cell, cell_rect.rect, format, bounds));
    }

    let mut trailing_edges = [HashMap::new(), HashMap::new()];
    for (cell, _, format, _) in &placed {
        let row_end = cell.row + cell.row_span;
        let col_end = cell.col + cell.col_span;
        if let Some(edge) = &format.edges[2] {
            trailing_edges[0].extend((cell.row..row_end).map(|row| ((col_end, row), edge)));
        }
        if let Some(edge) = &format.edges[3] {
            trailing_edges[1].extend((cell.col..col_end).map(|col| ((row_end, col), edge)));
        }
    }
    for (cell, rect, format, _) in &placed {
        let perimeter = [
            cell.col == 0,
            cell.row == 0,
            (cell.col + cell.col_span) as usize == table.col_widths_pt.len(),
            (cell.row + cell.row_span) as usize == table.row_heights_pt.len(),
        ];
        for (side, edge) in format.edges.iter().enumerate() {
            let Some(style) = edge else { continue };
            // The frame draws an identical perimeter once, and owns a patterned
            // boundary so the cell grid does not fill the gaps.
            if perimeter[side] && style.align == StrokeAlign::Inside && frame.edges[side].as_ref().is_some_and(|edge| {
                edge == style
                    || (!edge.line_style.is_solid() && edge.width_pt > 0.0 && !matches!(edge.paint, Paint::None))
            }) {
                continue;
            }
            let width = style.width_pt * units_per_point;
            let mut emit = |mut segment: [f32; 4]| {
                if side % 2 == 1 && style.line_style.is_solid() {
                    for (vertical, endpoint, sign) in [(0, 0, -1.0), (2, 2, 1.0)] {
                        if segment[endpoint] == rect[endpoint]
                            && let Some(neighbour) = format.edges[vertical].as_ref()
                            && neighbour.paint == style.paint && neighbour.line_style.is_solid()
                        {
                            segment[endpoint] += sign * outward_width(neighbour.align, neighbour.width_pt * units_per_point);
                        }
                    }
                }
                emit_edge(list, object.id, origin, segment, side, style, style.align, opacity, width);
            };
            if side >= 2 {
                emit(*rect);
                continue;
            }
            // The above/left cell already paints a matching shared border. Check by
            // track so a merged cell can share an edge with several smaller ones.
            let (boundary, start, end, tracks) = if side == 0 {
                (cell.col, cell.row, cell.row + cell.row_span, &grid.row_y)
            } else {
                (cell.row, cell.col, cell.col + cell.col_span, &grid.col_x)
            };
            let axis = 1 - side;
            let mut first = start;
            for last in start..=end {
                let shared = last < end && trailing_edges[side].get(&(boundary, last))
                    .is_some_and(|other| *other == style);
                if shared || last == end {
                    if first < last {
                        let mut segment = *rect;
                        segment[axis] = tracks[first as usize];
                        segment[axis + 2] = tracks[last as usize];
                        emit(segment);
                    }
                    first = last + 1;
                }
            }
        }
    }

    emit_edges(list, object, origin, frame_local, &frame.edges, opacity, units_per_point);

    if !outline_only {
        for (cell, _, _, bounds) in &placed {
            let Some(text) = story_text(doc, cell.story) else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            let glyph = glyph_paint(doc, cell.story);
            list.items.push(DisplayItem {
                object_id: object.id,
                bounds: *bounds,
                fill: visible(project_table_paint(PaintRole::Glyph, None, None, None, glyph.as_ref())),
                opacity,
                outline_only,
                role: "TableGlyph".to_string(),
                matrix: IDENTITY_MATRIX,
                clip: None,
                points: Vec::new(),
                table_edge: None,
                label: None,
            });
        }
    }
}

fn visible(paint: Paint) -> Option<Paint> {
    match paint {
        Paint::None => None,
        other => Some(other),
    }
}

fn glyph_paint(doc: &Document, id: StoryId) -> Option<Paint> {
    doc.stories
        .iter()
        .find(|story| story.id == id)
        .or_else(|| doc.stories.get(id.0 as usize))
        .and_then(|story| story.ranges.iter().find_map(range_fill))
}
fn story_text(doc: &Document, id: StoryId) -> Option<&str> {
    doc.stories
        .iter()
        .find(|story| story.id == id)
        .map(|story| story.text.as_str())
        .or_else(|| doc.stories.get(id.0 as usize).map(|story| story.text.as_str()))
}

fn inset_rect(rect: [f32; 4], insets: &Insets) -> [f32; 4] {
    let x0 = rect[0] + insets.left;
    let y0 = rect[1] + insets.top;
    let x1 = (rect[2] - insets.right).max(x0);
    let y1 = (rect[3] - insets.bottom).max(y0);
    [x0, y0, x1, y1]
}

fn offset_xywh(origin: [f32; 2], rect: [f32; 4]) -> [f32; 4] {
    [
        origin[0] + rect[0],
        origin[1] + rect[1],
        (rect[2] - rect[0]).max(0.0),
        (rect[3] - rect[1]).max(0.0),
    ]
}

fn band_bounds(origin: [f32; 2], grid: &TableGrid, start_row: u32, row_count: u32) -> Option<[f32; 4]> {
    if row_count == 0 || grid.col_x.len() < 2 || grid.row_y.len() < 2 {
        return None;
    }
    let start = start_row as usize;
    let end = start + row_count as usize;
    if start >= grid.row_y.len().saturating_sub(1) || end > grid.row_y.len().saturating_sub(1) {
        return None;
    }
    let x0 = *grid.col_x.first()?;
    let x1 = *grid.col_x.last()?;
    let y0 = grid.row_y[start];
    let y1 = grid.row_y[end];
    Some(offset_xywh(origin, [x0, y0, x1, y1]))
}

fn emit_edges(
    list: &mut DisplayList,
    object: &Object,
    origin: [f32; 2],
    rect: [f32; 4],
    edges: &[Option<EdgeStyle>; 4],
    opacity: f32,
    units_per_point: f32,
) {
    for (side, edge) in edges.iter().enumerate() {
        if let Some(style) = edge {
            emit_edge(list, object.id, origin, rect, side, style, StrokeAlign::Inside, opacity, style.width_pt * units_per_point);
        }
    }
}

fn emit_edge(list: &mut DisplayList, object_id: dtool_core::ObjectId, origin: [f32; 2], rect: [f32; 4], side: usize, style: &EdgeStyle, align: StrokeAlign, opacity: f32, width: f32) {
    list.items.push(DisplayItem {
        object_id,
        bounds: edge_bounds(origin, rect, side, width, align),
        fill: visible(project_table_paint(PaintRole::Edge, None, None, Some(style), None)),
        opacity,
        outline_only: false,
        role: "TableEdge".to_string(),
        matrix: IDENTITY_MATRIX,
        clip: None,
        points: Vec::new(),
        table_edge: (!style.line_style.is_solid()).then_some(TableEdgePattern {
            style: style.line_style,
            horizontal: side % 2 == 1,
        }),
        label: None,
    });
}

fn outward_width(align: StrokeAlign, width: f32) -> f32 {
    match align {
        StrokeAlign::Inside => 0.0,
        StrokeAlign::Centre => width * 0.5,
        StrokeAlign::Outside => width,
    }
}

fn edge_bounds(origin: [f32; 2], rect: [f32; 4], side: usize, width_pt: f32, align: StrokeAlign) -> [f32; 4] {
    let x0 = rect[0];
    let y0 = rect[1];
    let x1 = rect[2];
    let y1 = rect[3];
    let width = width_pt.max(0.0);
    let outward = outward_width(align, width);
    let inward = width - outward;
    let local = match side {
        0 => [x0 - outward, y0, x0 + inward, y1],
        1 => [x0, y0 - outward, x1, y0 + inward],
        2 => [x1 - inward, y0, x1 + outward, y1],
        _ => [x0, y1 - inward, x1, y1 + outward],
    };
    offset_xywh(origin, local)
}

fn cell_metrics(cell: &TableCell, cell_rect: &CellRect, insets: &Insets) -> [f32; 4] {
    let _ = (cell.row, cell.col, cell.row_span, cell.col_span);
    let _ = cell_rect.row;
    inset_rect(cell_rect.rect, insets)
}

#[cfg(test)]
mod role_emission_contracts {
    use dtool_core::object::{Object, ObjectKind};
    use dtool_core::paint::{Colour, Paint};
    use dtool_core::table::model::*;
    use dtool_core::{CanvasFlags, Document, Story};

    fn table_doc() -> Document {
        let mut alloc = CellIdAllocator { high_water: 0 };
        let mut cells = Vec::new();
        for col in 0..2u32 {
            cells.push(TableCell { id: alloc.next(), row: 0, col, row_span: 1, col_span: 1, merged_into: None, story: StoryId(u64::from(col) + 1), local: CellFormat::default(), mask: CellFormatMask::default() });
        }
        let mut frame = TableFrameFormat::default();
        let red_edge = EdgeStyle { paint: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, width_pt: 1.0, line_style: Default::default(), align: Default::default() };
        frame.edges = [Some(red_edge.clone()), Some(red_edge.clone()), Some(red_edge.clone()), Some(red_edge)];
        let table = TableNode { alloc, col_widths_pt: vec![40.0, 40.0], row_heights_pt: vec![20.0], cells, header_rows: 0, footer_rows: 0, format: None, frame_local: frame, frame_mask: FrameFormatMask(dtool_core::table::resolve::FRAME_EDGES.iter().fold(0, |mask, bit| mask | bit)), text_defaults: None };
        let mut doc = Document::default();
        doc.objects.push(Object { kind: ObjectKind::Table(table), pos: [10.0, 10.0], size: [80.0, 20.0], opacity: 1.0, ..Object::default() });
        doc.objects.push(Object { pos: [0.0, 0.0], size: [5.0, 5.0], opacity: 1.0, fill: Paint::Solid { colour: Colour::new(0, 255, 0, 255) }, ..Object::default() });
        for id in 1..=2u64 {
            doc.stories.push(Story { id: StoryId(id), text: "ab".to_string(), ranges: vec![serde_json::json!({"start": 0, "end": 2, "fill": "black"})], ..Story::default() });
        }
        doc
    }

    #[test]
    fn table_roles_emit_in_order_with_distinct_paints() {
        let doc = table_doc();
        let list = crate::build_display_list(&doc, &CanvasFlags::default());
        let roles: Vec<&str> = list.items.iter().map(|item| item.role.as_str()).collect();
        let first = |role: &str| roles.iter().position(|r| *r == role).unwrap();
        let last = |role: &str| roles.iter().rposition(|r| *r == role).unwrap();
        assert_eq!(roles[0], "TableFrame");
        assert!(last("TableCell") < first("TableEdge"));
        assert!(last("TableEdge") < first("TableGlyph"));
        assert_eq!(roles.iter().filter(|r| **r == "TableGlyph").count(), 2);
        let edge = list.items.iter().find(|item| item.role == "TableEdge").unwrap();
        let glyph = list.items.iter().find(|item| item.role == "TableGlyph").unwrap();
        assert_eq!(edge.fill, Some(Paint::Solid { colour: Colour::new(255, 0, 0, 255) }));
        assert_eq!(glyph.fill, Some(Paint::Solid { colour: Colour::new(0, 0, 0, 255) }));
        assert_ne!(edge.fill, glyph.fill);
        assert_eq!(list.items.last().unwrap().role, "Default");
    }
}
