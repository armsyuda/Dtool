use crate::command::{resolve_target_ids, selection_bounds, transform_members, Session};
use crate::commands::curve_edit::{commit_contours, editable_curve, world_contours};
use crate::curve::PointId;
use crate::data_merge::layout::{object_page_index, spread_ranges};
use std::{collections::{HashMap, HashSet}, ops::Range};
use crate::object::ObjectId;
use dtool_protocol::{Command, CommandError};
use serde::Deserialize;

pub const ALIGN_PREFIX: &str = "layer.arrange.align.";
pub const DISTRIBUTE_PREFIX: &str = "layer.arrange.distribute.";
pub const ALIGN_EDGES: [&str; 6] = ["left", "centre", "right", "top", "middle", "bottom"];
pub const DISTRIBUTE_AXES: [&str; 2] = ["h", "v"];
pub const PATH_POINTS_ALIGN: &str = "path.points.align";

pub fn align_deltas(rects: &[[f32; 4]], edge: &str, target: [f32; 4]) -> Vec<[f32; 2]> {
    rects
        .iter()
        .map(|r| match edge {
            "left" => [target[0] - r[0], 0.0],
            "centre" => [target[0] + target[2] / 2.0 - (r[0] + r[2] / 2.0), 0.0],
            "right" => [target[0] + target[2] - (r[0] + r[2]), 0.0],
            "top" => [0.0, target[1] - r[1]],
            "middle" => [0.0, target[1] + target[3] / 2.0 - (r[1] + r[3] / 2.0)],
            _ => [0.0, target[1] + target[3] - (r[1] + r[3])],
        })
        .collect()
}

pub fn union_rects(rects: &[[f32; 4]]) -> Option<[f32; 4]> {
    let first = *rects.first()?;
    let mut min_x = first[0];
    let mut min_y = first[1];
    let mut max_x = first[0] + first[2];
    let mut max_y = first[1] + first[3];
    for rect in &rects[1..] {
        min_x = min_x.min(rect[0]);
        min_y = min_y.min(rect[1]);
        max_x = max_x.max(rect[0] + rect[2]);
        max_y = max_y.max(rect[1] + rect[3]);
    }
    Some([min_x, min_y, max_x - min_x, max_y - min_y])
}

fn distribute_ordered(rects: &[[f32; 4]], horizontal: bool, span: Option<(f32, f32)>) -> Vec<[f32; 2]> {
    if rects.len() < 2 {
        return vec![[0.0, 0.0]; rects.len()];
    }
    let axis = if horizontal { 0 } else { 1 };
    let (start, end) = match span {
        Some(span) => span,
        None => {
            let first = rects[0];
            let last = rects[rects.len() - 1];
            (first[axis], last[axis] + last[axis + 2])
        }
    };
    let total: f32 = rects.iter().map(|rect| rect[axis + 2]).sum();
    let gaps = (rects.len() - 1) as f32;
    let gap = (end - start - total) / gaps;
    let mut cursor = start;
    rects
        .iter()
        .map(|rect| {
            let mut delta = [0.0, 0.0];
            delta[axis] = cursor - rect[axis];
            cursor += rect[axis + 2] + gap;
            delta
        })
        .collect()
}

pub fn distribute_fixed(rects: &[[f32; 4]], horizontal: bool, start: f32, gap: f32) -> Vec<[f32; 2]> {
    let axis = if horizontal { 0 } else { 1 };
    let mut cursor = start;
    rects
        .iter()
        .map(|rect| {
            let mut delta = [0.0, 0.0];
            delta[axis] = cursor - rect[axis];
            cursor += rect[axis + 2] + gap;
            delta
        })
        .collect()
}

pub fn distribute_deltas(rects: &[[f32; 4]], horizontal: bool, span: Option<(f32, f32)>) -> Vec<[f32; 2]> {
    let axis = if horizontal { 0 } else { 1 };
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by(|&a, &b| rects[a][axis].total_cmp(&rects[b][axis]).then(a.cmp(&b)));
    let ordered: Vec<[f32; 4]> = order.iter().map(|&index| rects[index]).collect();
    let shifts = distribute_ordered(&ordered, horizontal, span);
    let mut out = vec![[0.0, 0.0]; rects.len()];
    for (index, delta) in order.into_iter().zip(shifts) {
        out[index] = delta;
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelativeTo {
    #[default]
    Selection,
    Spread,
    Key,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlignArgs {
    #[serde(default)]
    pub ids: Option<Vec<u64>>,
    #[serde(default)]
    pub relative_to: Option<RelativeTo>,
    #[serde(default)]
    pub key_id: Option<u64>,
    #[serde(default)]
    pub spacing: Option<f32>,
}

enum Op {
    Align(&'static str),
    Distribute(bool),
}

fn op(name: &str) -> Option<Op> {
    if let Some(edge) = name.strip_prefix(ALIGN_PREFIX) {
        return ALIGN_EDGES.iter().find(|e| **e == edge).map(|e| Op::Align(e));
    }
    match name.strip_prefix(DISTRIBUTE_PREFIX)? {
        "h" => Some(Op::Distribute(true)),
        "v" => Some(Op::Distribute(false)),
        _ => None,
    }
}

pub fn is_align_command(name: &str) -> bool {
    op(name).is_some()
}

fn parse(params: &serde_json::Value) -> Result<AlignArgs, CommandError> {
    if params.is_null() {
        return Ok(AlignArgs::default());
    }
    serde_json::from_value(params.clone()).map_err(|_| CommandError::invalid_params())
}

struct LayoutSpread {
    pages: Range<usize>,
    rects: Vec<[f32; 4]>,
    bounds: [f32; 4],
}

fn layout_spread(session: &Session, args: &AlignArgs, ids: &[ObjectId]) -> Result<Option<LayoutSpread>, CommandError> {
    let doc = &session.document;
    if args.relative_to != Some(RelativeTo::Spread) || doc.layout_artboard_origins.is_none() {
        return Ok(None);
    }

    let first_page = object_page_index(doc, ids[0]);
    let pages = spread_ranges(doc).into_iter().find(|range| range.contains(&first_page))
        .filter(|range| range.end <= doc.pages.len())
        .ok_or_else(CommandError::invalid_params)?;
    for &id in ids {
        let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
        if !pages.contains(&object_page_index(doc, id))
            || crate::artboard::is_artboard(object)
            || super::masters::owns_source(doc, id)
            || doc.master_pages.source_of(id).is_some()
            || doc.master_pages.assignments.iter().any(|assignment| assignment.group == Some(id))
        {
            return Err(CommandError::invalid_params());
        }
    }
    let rects = crate::scene::page_rects(doc);
    let bounds = union_rects(&rects[pages.clone()]).ok_or_else(CommandError::invalid_params)?;
    Ok(Some(LayoutSpread { pages, rects, bounds }))
}

fn spread_rect(session: &Session, layout: Option<&LayoutSpread>) -> [f32; 4] {
    layout.map_or_else(|| {
        let size = session.document.pages.first().map(|p| p.size).unwrap_or([0.0, 0.0]);
        [0.0, 0.0, size[0], size[1]]
    }, |spread| spread.bounds)
}

/// Spread members use absolute document coordinates even when the layer tree has a page wrapper.
fn spread_updates(
    session: &Session, spread: &LayoutSpread, moves: &[(ObjectId, [f32; 2])],
) -> Result<Vec<crate::Object>, CommandError> {
    let doc = &session.document;
    let mut updates = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for &(id, delta) in moves {
        let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
        let source = object_page_index(doc, id);
        let [x, y, w, h] = bounds(session, id)?;
        let point = [x + w * 0.5 + delta[0], y + h * 0.5 + delta[1]];
        let destination = spread.pages.clone().find(|&index| {
            let [left, top, width, height] = spread.rects[index];
            point[0] >= left && point[0] < left + width && point[1] >= top && point[1] < top + height
        }).filter(|&index| index != source);
        let destination = destination.map(|index| {
            let from = doc.linked_artboard(doc.pages[source].id).map(|board| board.id);
            let page_id = doc.pages[index].id;
            let to = doc.linked_artboard(page_id);
            if (object.parent_id.is_some() && object.parent_id != from) || to.is_some_and(|board| board.locked) {
                return Err(CommandError::invalid_params());
            }
            Ok((page_id, to.map(|board| board.id)))
        }).transpose()?;

        for member in doc.subtree_ids(id) {
            if !visited.insert(member) {
                return Err(CommandError::invalid_params());
            }
            let mut layer = doc.get_object(member).ok_or_else(CommandError::not_found)?.clone();
            layer.pos = [layer.pos[0] + delta[0], layer.pos[1] + delta[1]];
            if layer.pos.iter().any(|value| !value.is_finite()) {
                return Err(CommandError::invalid_params());
            }
            if let Some((page_id, parent_id)) = destination {
                layer.page_id = Some(page_id);
                if member == id {
                    layer.parent_id = parent_id;
                }
            }
            updates.push(layer);
        }
    }
    updates.sort_by_key(|layer| layer.id.0);
    Ok(updates)
}

fn alignment_members(doc: &crate::Document, ids: &[ObjectId]) -> Result<HashMap<ObjectId, Vec<ObjectId>>, CommandError> {
    let members = transform_members(doc, ids);
    let mut targets: HashSet<_> = ids.iter().copied().collect();
    // Overlapping selections can assign the same layer two positions, or move the key as a child.
    if targets.len() != ids.len() || members.values().flatten().any(|&id| !targets.insert(id)) {
        return Err(CommandError::invalid_params());
    }
    Ok(members)
}

fn translated_objects(
    doc: &crate::Document, ids: &[ObjectId], moves: &[(ObjectId, [f32; 2])],
) -> Result<Vec<crate::Object>, CommandError> {
    let members = alignment_members(doc, ids)?;
    let mut objects = Vec::new();
    for &(root, [dx, dy]) in moves {
        for id in std::iter::once(&root).chain(members.get(&root).into_iter().flatten()) {
            let mut object = doc.get_object(*id).ok_or_else(CommandError::not_found)?.clone();
            object.pos = [object.pos[0] + dx, object.pos[1] + dy];
            if !object.pos.iter().all(|coordinate| coordinate.is_finite()) {
                return Err(CommandError::invalid_params());
            }
            objects.push(object);
        }
    }
    Ok(objects)
}

fn bounds(session: &Session, id: ObjectId) -> Result<[f32; 4], CommandError> {
    selection_bounds(&session.document, &[id]).ok_or_else(CommandError::not_found)
}

fn deltas(
    session: &Session, op: &Op, args: &AlignArgs, ids: &[ObjectId], layout: Option<&LayoutSpread>,
) -> Result<Vec<(ObjectId, [f32; 2])>, CommandError> {
    let rects: Vec<(ObjectId, [f32; 4])> = ids.iter().map(|id| bounds(session, *id).map(|r| (*id, r))).collect::<Result<_, _>>()?;
    match op {
        Op::Align(edge) => {
            let target_mode = args.relative_to.unwrap_or_else(|| {
                if session.selection.explicit_key().is_some_and(|id| ids.contains(&id)) {
                    RelativeTo::Key
                } else {
                    RelativeTo::Selection
                }
            });
            let (target, fixed) = match target_mode {
                RelativeTo::Selection => {
                    if ids.len() < 2 {
                        return Err(CommandError::invalid_params());
                    }
                    (selection_bounds(&session.document, ids).ok_or_else(CommandError::invalid_params)?, None)
                }
                RelativeTo::Spread => (spread_rect(session, layout), None),
                RelativeTo::Key => {
                    let selected_key = session.selection.key().filter(|key| ids.contains(key));
                    let key = args.key_id.map(ObjectId).or(selected_key).unwrap_or(ids[0]);
                    if ids.len() < 2 || !ids.contains(&key) {
                        return Err(CommandError::invalid_params());
                    }
                    (bounds(session, key)?, Some(key))
                }
            };
            let list: Vec<[f32; 4]> = rects.iter().map(|(_, r)| *r).collect();
            let shifts = align_deltas(&list, edge, target);
            Ok(rects
                .into_iter()
                .zip(shifts)
                .filter(|((id, _), _)| Some(*id) != fixed)
                .map(|((id, _), d)| (id, d))
                .collect())
        }
        Op::Distribute(horizontal) => {
            let axis = if *horizontal { 0 } else { 1 };
            let mut sorted = rects;
            sorted.sort_by(|a, b| a.1[axis].total_cmp(&b.1[axis]).then(a.0 .0.cmp(&b.0 .0)));
            if let Some(gap) = args.spacing {
                if !gap.is_finite() || sorted.len() < 2 {
                    return Err(CommandError::invalid_params());
                }
                let start = match args.relative_to {
                    Some(RelativeTo::Spread) => spread_rect(session, layout)[axis],
                    _ => sorted[0].1[axis],
                };
                let list: Vec<[f32; 4]> = sorted.iter().map(|(_, r)| *r).collect();
                let shifts = distribute_fixed(&list, *horizontal, start, gap);
                return Ok(sorted.into_iter().zip(shifts).map(|((id, _), d)| (id, d)).collect());
            }
            let span = match args.relative_to {
                Some(RelativeTo::Spread) => {
                    if sorted.len() < 2 {
                        return Err(CommandError::invalid_params());
                    }
                    let spread = spread_rect(session, layout);
                    Some((spread[axis], spread[axis] + spread[axis + 2]))
                }
                _ => {
                    if sorted.len() < 3 {
                        return Err(CommandError::invalid_params());
                    }
                    None
                }
            };
            let list: Vec<[f32; 4]> = sorted.iter().map(|(_, r)| *r).collect();
            let shifts = distribute_ordered(&list, *horizontal, span);
            Ok(sorted.into_iter().zip(shifts).map(|((id, _), d)| (id, d)).collect())
        }
    }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name == PATH_POINTS_ALIGN {
        return Some(plan_points(session, command).is_ok());
    }
    let op = op(&command.name)?;
    Some((|| {
        if !session.has_document {
            return false;
        }
        let Ok(args) = parse(&command.params) else { return false };
        let Ok(ids) = resolve_target_ids(&session.document, &args.ids, &session.selection) else { return false };
        let Ok(layout) = layout_spread(session, &args, &ids) else { return false };
        (layout.is_some() || alignment_members(&session.document, &ids).is_ok())
            && deltas(session, &op, &args, &ids, layout.as_ref()).is_ok()
    })())
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    if command.name == PATH_POINTS_ALIGN {
        return Some(align_points(session, command));
    }
    let op = op(&command.name)?;
    Some((|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        let args = parse(&command.params)?;
        let ids = resolve_target_ids(&session.document, &args.ids, &session.selection)?;
        let layout = layout_spread(session, &args, &ids)?;
        let moves: Vec<(ObjectId, [f32; 2])> = deltas(session, &op, &args, &ids, layout.as_ref())?
            .into_iter()
            .filter(|(_, d)| d[0].abs() > 1e-4 || d[1].abs() > 1e-4)
            .collect();
        let moved: Vec<ObjectId> = moves.iter().map(|(id, _)| *id).collect();
        let new = if let Some(spread) = &layout {
            spread_updates(session, spread, &moves)?
        } else {
            translated_objects(&session.document, &ids, &moves)?
        };
        if !new.is_empty() {
            let affected: Vec<_> = new.iter().map(|layer| layer.id).collect();
            let old = session.snapshot_objects(&affected);
            for layer in &new {
                if let Some(current) = session.document.find_object_mut(layer.id.0) {
                    *current = layer.clone();
                }
            }
            let redo = Command {
                name: "object.restore".to_string(),
                params: serde_json::json!({ "objects": new }),
            };
            session.push_restore_undo(&command.name, redo, old);
        }
        let changed: Vec<u64> = moved.iter().map(|id| id.0).collect();
        Ok(serde_json::json!({
            "doc_version": session.generation,
            "changes": [{ "name": command.name, "ids": changed }]
        }))
    })())
}

fn parse_curve_id(params: &serde_json::Value) -> Result<ObjectId, CommandError> {
    let value = params.get("target").or_else(|| params.get("id")).ok_or_else(CommandError::invalid_params)?;
    let id = value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.strip_prefix("node:").unwrap_or(text).parse().ok()))
        .ok_or_else(CommandError::invalid_params)?;
    Ok(ObjectId(id))
}

fn parse_point_ids(params: &serde_json::Value) -> Result<Vec<PointId>, CommandError> {
    let items = params
        .get("point_ids")
        .or_else(|| params.get("points"))
        .or_else(|| params.get("nodes"))
        .and_then(serde_json::Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(CommandError::invalid_params)?;
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        let id = PointId(item.as_u64().ok_or_else(CommandError::invalid_params)?);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn plan_points(session: &Session, command: &Command) -> Result<(ObjectId, Vec<crate::curve::Contour>), CommandError> {
    let id = parse_curve_id(&command.params)?;
    let object = editable_curve(session, id)?;
    let mut contours = world_contours(object);
    let ids = parse_point_ids(&command.params)?;
    if ids.len() < 2 {
        return Err(CommandError::invalid_params());
    }
    let edge = command
        .params
        .get("align")
        .and_then(serde_json::Value::as_str)
        .filter(|edge| ALIGN_EDGES.contains(edge))
        .ok_or_else(CommandError::invalid_params)?;
    let relative = match command.params.get("relative_to").and_then(serde_json::Value::as_str) {
        None | Some("selection") => RelativeTo::Selection,
        Some("key") => RelativeTo::Key,
        _ => return Err(CommandError::invalid_params()),
    };
    let mut located = Vec::with_capacity(ids.len());
    for point_id in &ids {
        let mut found = None;
        for (contour_index, contour) in contours.iter().enumerate() {
            if let Some(point_index) = contour.points.iter().position(|point| point.id == *point_id) {
                found = Some((contour_index, point_index));
                break;
            }
        }
        let Some(location) = found else {
            return Err(CommandError::not_found());
        };
        located.push((*point_id, location));
    }
    let rects: Vec<[f32; 4]> = located
        .iter()
        .map(|(_, (contour_index, point_index))| {
            let pos = contours[*contour_index].points[*point_index].pos;
            [pos[0] as f32, pos[1] as f32, 0.0, 0.0]
        })
        .collect();
    let (target, skip) = match relative {
        RelativeTo::Selection => (union_rects(&rects).ok_or_else(CommandError::invalid_params)?, None),
        RelativeTo::Key => {
            let key = command
                .params
                .get("key_point")
                .and_then(serde_json::Value::as_u64)
                .map(PointId)
                .or_else(|| ids.first().copied())
                .ok_or_else(CommandError::invalid_params)?;
            let index = ids.iter().position(|id| *id == key).ok_or_else(CommandError::invalid_params)?;
            (rects[index], Some(index))
        }
        RelativeTo::Spread => return Err(CommandError::invalid_params()),
    };
    let shifts = align_deltas(&rects, edge, target);
    for (index, ((_, (contour_index, point_index)), delta)) in located.iter().zip(shifts).enumerate() {
        if Some(index) == skip {
            continue;
        }
        let point = &mut contours[*contour_index].points[*point_index];
        let shift = [f64::from(delta[0]), f64::from(delta[1])];
        if !shift[0].is_finite() || !shift[1].is_finite() {
            return Err(CommandError::invalid_params());
        }
        point.pos = [point.pos[0] + shift[0], point.pos[1] + shift[1]];
        point.handle_in = point.handle_in.map(|handle| [handle[0] + shift[0], handle[1] + shift[1]]);
        point.handle_out = point.handle_out.map(|handle| [handle[0] + shift[0], handle[1] + shift[1]]);
    }
    Ok((id, contours))
}

fn align_points(session: &mut Session, command: &Command) -> Result<serde_json::Value, CommandError> {
    let (id, contours) = plan_points(session, command)?;
    commit_contours(session, id, contours, PATH_POINTS_ALIGN, serde_json::json!({}))
}
