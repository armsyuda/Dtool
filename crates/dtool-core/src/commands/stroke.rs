use super::{param_id, symbols::{self, StrokeEdit}};
use crate::command::Session;
use crate::document::Document;
use crate::object::ObjectId;
use crate::paint::{validate_stroke, Arrow, Cap, Colour, Join, Paint, StrokeAlign, StrokeStyle};
use dtool_protocol::{Command, CommandError, CommandResult};

pub const STROKE_SET: &str = "stroke.set";
const RESET_PROFILE: &str = "stroke.profile.reset";
pub const STROKE_QUERY: &str = "stroke.query";
pub const STROKE_ARROWS_SWAP: &str = "stroke.arrows.swap";
pub const STROKE_ARROWS_LIST: &str = "stroke.arrows.list";

fn ours(name: &str) -> bool {
    matches!(name, STROKE_SET | STROKE_QUERY | STROKE_ARROWS_SWAP | STROKE_ARROWS_LIST | RESET_PROFILE)
}

fn target(params: &serde_json::Value) -> Result<ObjectId, CommandError> {
    let probe = Command { name: String::new(), params: params.clone() };
    param_id(&probe, &["object_id", "id", "node"]).ok_or_else(CommandError::invalid_params)
}

pub fn apply_stroke_set(doc: &mut Document, object_id: ObjectId, width: f32, dash: Vec<f32>) -> CommandResult {
    if !width.is_finite() || width < 0.0 || dash.iter().any(|d| !d.is_finite() || *d < 0.0) {
        return Err(CommandError::invalid_params());
    }
    let obj = doc.find_object_mut(object_id.0).ok_or_else(CommandError::not_found)?;
    obj.stroke.width = width;
    obj.stroke.dash = dash;
    if obj.stroke.paint == Paint::None && width > 0.0 {
        obj.stroke.paint = Paint::Solid { colour: Colour::new(0, 0, 0, 255) };
    }
    Ok(serde_json::json!({ "id": object_id.0, "stroke": obj.stroke }))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrokePatch {
    pub width: Option<f32>,
    pub dash: Option<Vec<f32>>,
    pub dash_offset: Option<f32>,
    pub cap: Option<Cap>,
    pub join: Option<Join>,
    pub align: Option<StrokeAlign>,
    pub start_arrow: Option<Option<Arrow>>,
    pub end_arrow: Option<Option<Arrow>>,
    pub scale_with_object: Option<bool>,
    pub pressure: Option<Vec<[f32; 2]>>,
    pub paint: Option<Paint>,
}

fn field<T: serde::de::DeserializeOwned>(params: &serde_json::Value, key: &str) -> Result<Option<T>, CommandError> {
    match params.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone()).map(Some).map_err(|_| CommandError::invalid_params()),
    }
}

fn arrow_field(params: &serde_json::Value, key: &str) -> Result<Option<Option<Arrow>>, CommandError> {
    match params.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(Some(None)),
        Some(value) => serde_json::from_value(value.clone()).map(|arrow| Some(Some(arrow))).map_err(|_| CommandError::invalid_params()),
    }
}

impl StrokePatch {
    pub fn from_params(params: &serde_json::Value) -> Result<Self, CommandError> {
        Ok(Self {
            width: field::<f64>(params, "width")?.map(|value| value as f32),
            dash: field(params, "dash")?,
            dash_offset: field::<f64>(params, "dash_offset")?.map(|value| value as f32),
            cap: field(params, "cap")?,
            join: field(params, "join")?,
            align: field(params, "align")?,
            start_arrow: arrow_field(params, "start_arrow")?,
            end_arrow: arrow_field(params, "end_arrow")?,
            scale_with_object: field(params, "scale_with_object")?,
            pressure: field(params, "pressure")?,
            paint: field(params, "paint")?,
        })
    }

    pub fn applied(&self, current: &StrokeStyle) -> Result<StrokeStyle, CommandError> {
        let mut next = current.clone();
        if let Some(width) = self.width {
            next.width = width;
        }
        if let Some(dash) = &self.dash {
            next.dash = dash.clone();
        }
        if let Some(offset) = self.dash_offset {
            next.dash_offset = offset;
        }
        if let Some(cap) = self.cap {
            next.cap = cap;
        }
        if let Some(join) = self.join {
            next.join = join;
        }
        if let Some(align) = self.align {
            next.align = align;
        }
        if let Some(arrow) = &self.start_arrow {
            next.start_arrow = arrow.clone();
        }
        if let Some(arrow) = &self.end_arrow {
            next.end_arrow = arrow.clone();
        }
        if let Some(scale) = self.scale_with_object {
            next.scale_with_object = scale;
        }
        if let Some(pressure) = &self.pressure {
            next.pressure = pressure.clone();
        }
        match &self.paint {
            Some(paint) => next.paint = paint.clone(),
            None if next.paint == Paint::None && next.width > 0.0 => next.paint = Paint::Solid { colour: Colour::new(0, 0, 0, 255) },
            None => {}
        }
        let arrows_ok = [&self.start_arrow, &self.end_arrow].iter().all(|patch| match patch {
            Some(Some(a)) => a.scale.is_finite() && a.scale > 0.0 && canonical_arrow(&a.name).is_some(),
            _ => true,
        });
        let pressure_ok = next.pressure.iter().all(|[x, y]| x.is_finite() && y.is_finite() && (0.0..=1.0).contains(x) && *y >= 0.0) && next.pressure.windows(2).all(|w| w[0][0] <= w[1][0]);
        let join_ok = !matches!(next.join, Join::Miter { limit } if !limit.is_finite() || limit < 1.0);
        if !validate_stroke(&next) || !arrows_ok || !pressure_ok || !join_ok {
            return Err(CommandError::invalid_params());
        }
        Ok(next)
    }
}

pub fn stroke_targets(params: &serde_json::Value, selection: &[ObjectId]) -> Result<Vec<ObjectId>, CommandError> {
    if let Some(list) = params.get("ids") {
        let ids: Vec<u64> = serde_json::from_value(list.clone()).map_err(|_| CommandError::invalid_params())?;
        if ids.is_empty() {
            return Err(CommandError::invalid_params());
        }
        return Ok(ids.into_iter().map(ObjectId).collect());
    }
    if let Ok(id) = target(params) {
        return Ok(vec![id]);
    }
    if selection.is_empty() {
        return Err(CommandError::invalid_params());
    }
    Ok(selection.to_vec())
}

fn editable_stroke(doc: &Document, id: ObjectId) -> Result<&StrokeStyle, CommandError> {
    let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    if doc.is_object_locked(object) {
        return Err(CommandError::new("LOCKED"));
    }
    Ok(&object.stroke)
}

pub fn apply_stroke_patch(doc: &mut Document, ids: &[ObjectId], patch: &StrokePatch) -> CommandResult {
    let mut next = Vec::with_capacity(ids.len());
    for id in ids {
        let current = editable_stroke(doc, *id)?;
        next.push(patch.applied(current)?);
    }
    let mut out = Vec::with_capacity(ids.len());
    for (id, stroke) in ids.iter().zip(next) {
        let obj = doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        obj.stroke = stroke;
        out.push(serde_json::json!({ "id": id.0, "stroke": obj.stroke }));
    }
    Ok(serde_json::json!({ "objects": out }))
}

pub fn apply_stroke_query(doc: &Document, object_id: ObjectId) -> CommandResult {
    let obj = doc.get_object(object_id).ok_or_else(CommandError::not_found)?;
    let mut out = serde_json::to_value(&obj.stroke).map_err(|_| CommandError::invalid_params())?;
    out["id"] = serde_json::json!(object_id.0);
    out["stroke"] = serde_json::to_value(&obj.stroke).map_err(|_| CommandError::invalid_params())?;
    Ok(out)
}

pub fn apply_stroke_arrows_swap(doc: &mut Document, object_id: ObjectId) -> CommandResult {
    editable_stroke(doc, object_id)?;
    let obj = doc.find_object_mut(object_id.0).ok_or_else(CommandError::not_found)?;
    std::mem::swap(&mut obj.stroke.start_arrow, &mut obj.stroke.end_arrow);
    Ok(serde_json::json!({ "id": object_id.0, "stroke": obj.stroke }))
}

pub const ARROW_NAMES: &[&str] = &["triangle", "open", "circle", "square", "diamond", "bar"];

#[derive(Debug, Clone, PartialEq)]
pub struct ArrowHead {
    pub points: Vec<[f32; 2]>,
    pub filled: bool,
    pub line_width: f32,
}

pub fn arrow_label_key(name: &str) -> String {
    format!("stroke.arrow_head.{}", canonical_arrow(name).unwrap_or(name))
}

pub fn canonical_arrow(name: &str) -> Option<&'static str> {
    let lower = name.trim().to_ascii_lowercase();
    let lower = if lower == "arrow" { "triangle".to_string() } else { lower };
    ARROW_NAMES.iter().copied().find(|known| *known == lower)
}

pub fn arrowhead(name: &str, tip: [f32; 2], from: [f32; 2], width: f32, scale: f32) -> Option<ArrowHead> {
    let name = canonical_arrow(name)?;
    if !scale.is_finite() || scale <= 0.0 || !width.is_finite() {
        return None;
    }
    let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
    let length = (dx * dx + dy * dy).sqrt();
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    let (ux, uy) = (dx / length, dy / length);
    let (nx, ny) = (-uy, ux);
    let size = width.max(1.0) * 3.0 * scale;
    let at = |along: f32, across: f32| [tip[0] + ux * along + nx * across, tip[1] + uy * along + ny * across];
    let filled = |points: Vec<[f32; 2]>| Some(ArrowHead { points, filled: true, line_width: 0.0 });
    match name {
        "triangle" => filled(vec![at(0.0, 0.0), at(-size, size * 0.5), at(-size, -size * 0.5)]),
        "open" => Some(ArrowHead { points: vec![at(-size, size * 0.5), at(0.0, 0.0), at(-size, -size * 0.5)], filled: false, line_width: width.max(1.0) }),
        "circle" => {
            let radius = size * 0.5;
            filled((0..24).map(|step| {
                let angle = step as f32 / 24.0 * std::f32::consts::TAU;
                at(-radius + radius * angle.cos(), radius * angle.sin())
            }).collect())
        }
        "square" => filled(vec![at(0.0, size * 0.5), at(0.0, -size * 0.5), at(-size, -size * 0.5), at(-size, size * 0.5)]),
        "diamond" => filled(vec![at(0.0, 0.0), at(-size * 0.5, size * 0.5), at(-size, 0.0), at(-size * 0.5, -size * 0.5)]),
        "bar" => {
            let half = width.max(1.0) * 0.5;
            filled(vec![at(half, size * 0.6), at(half, -size * 0.6), at(-half, -size * 0.6), at(-half, size * 0.6)])
        }
        _ => None,
    }
}

pub fn arrowheads(stroke: &StrokeStyle, points: &[[f32; 2]], closed: bool, width: f32) -> Vec<ArrowHead> {
    if closed || points.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(arrow) = &stroke.start_arrow {
        if let Some(from) = points.iter().skip(1).find(|point| **point != points[0]) {
            out.extend(arrowhead(&arrow.name, points[0], *from, width, arrow.scale));
        }
    }
    if let Some(arrow) = &stroke.end_arrow {
        let last = points[points.len() - 1];
        if let Some(from) = points.iter().rev().skip(1).find(|point| **point != last) {
            out.extend(arrowhead(&arrow.name, last, *from, width, arrow.scale));
        }
    }
    out
}

/// Stroke end decorations in the same coordinate space as the centreline.
/// Open decorations carry their own solid stroke style.
pub(crate) fn arrow_paths(path: &kurbo::BezPath, style: &StrokeStyle) -> Vec<(kurbo::BezPath, Option<StrokeStyle>)> {
    if style.start_arrow.is_none() && style.end_arrow.is_none() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    for (points, closed) in crate::outline::flatten_subpaths(path, crate::outline::OUTLINE_TOLERANCE) {
        let points: Vec<_> = points.into_iter().map(|point| point.map(|value| value as f32)).collect();
        for arrow in arrowheads(style, &points, closed, style.width) {
            let Some((&first, rest)) = arrow.points.split_first() else { continue; };
            let mut path = kurbo::BezPath::new();
            let point = |[x, y]: [f32; 2]| (f64::from(x), f64::from(y));
            path.move_to(point(first));
            for &next in rest { path.line_to(point(next)); }
            let stroke = if arrow.filled {
                path.close_path();
                None
            } else {
                Some(StrokeStyle {
                    width: arrow.line_width,
                    paint: style.paint.clone(),
                    cap: style.cap,
                    join: style.join.clone(),
                    ..Default::default()
                })
            };
            paths.push((path, stroke));
        }
    }
    paths
}

pub fn apply_stroke_arrows_list(_doc: &Document) -> CommandResult {
    let arrows: Vec<serde_json::Value> = ARROW_NAMES.iter().map(|name| serde_json::json!({ "name": name, "label_key": arrow_label_key(name) })).collect();
    Ok(serde_json::json!({ "arrows": arrows }))
}

pub fn can_execute_stroke_set(doc: &Document, object_id: ObjectId) -> bool {
    editable_stroke(doc, object_id).is_ok()
}

pub fn run_stroke_command(cmd_name: &str, doc: &mut Document, params: &serde_json::Value) -> CommandResult {
    match cmd_name {
        STROKE_SET => {
            let ids = stroke_targets(params, &[])?;
            apply_stroke_patch(doc, &ids, &StrokePatch::from_params(params)?)
        }
        STROKE_QUERY => apply_stroke_query(doc, target(params)?),
        STROKE_ARROWS_SWAP => apply_stroke_arrows_swap(doc, target(params)?),
        STROKE_ARROWS_LIST => apply_stroke_arrows_list(doc),
        _ => Err(CommandError::not_implemented()),
    }
}

fn reset_profile(session: &Session) -> Option<Command> {
    let ids: Vec<_> = session.selection.ids.iter().filter_map(|id| session.document.get_object(*id))
        .filter(|object| !session.document.is_object_locked(object) && !object.stroke.pressure.is_empty())
        .map(|object| object.id.0).collect();
    (!ids.is_empty()).then(|| Command::with_params(STROKE_SET, serde_json::json!({ "ids": ids, "pressure": [] })))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !ours(&command.name) {
        return None;
    }
    if command.name == RESET_PROFILE {
        return Some(reset_profile(session).is_some());
    }
    if command.name == STROKE_ARROWS_LIST {
        return Some(true);
    }
    if command.name == STROKE_SET {
        return Some(stroke_targets(&command.params, &session.selection.ids).is_ok_and(|ids| ids.iter().all(|id| can_execute_stroke_set(&session.document, *id))));
    }
    if command.name == STROKE_ARROWS_SWAP {
        return Some(stroke_targets(&command.params, &[]).is_ok_and(|ids| {
            ids.iter().all(|id| can_execute_stroke_set(&session.document, *id))
        }));
    }
    Some(target(&command.params).is_ok_and(|id| session.document.get_object(id).is_some()))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !ours(&command.name) {
        return None;
    }
    if command.name == RESET_PROFILE {
        let Some(command) = reset_profile(session) else {
            return Some(Err(CommandError::invalid_params()));
        };
        return run(session, &command);
    }
    if command.name == STROKE_QUERY || command.name == STROKE_ARROWS_LIST {
        return Some(run_stroke_command(&command.name, &mut session.document, &command.params));
    }
    if command.name == STROKE_SET {
        return Some((|| {
            let ids = stroke_targets(&command.params, &session.selection.ids)?;
            let patch = StrokePatch::from_params(&command.params)?;
            let old = session.snapshot_objects(&ids);
            if old.len() != ids.len() {
                return Err(CommandError::not_found());
            }
            let mut unchanged = true;
            for id in &ids {
                let current = editable_stroke(&session.document, *id)?;
                unchanged &= patch.applied(current)? == *current;
            }
            if unchanged {
                return Ok(serde_json::json!({ "doc_version": session.generation, "changes": [] }));
            }
            if ids.iter().any(|id| symbols::object_link(&session.document, *id).is_some()) {
                let changes = linked_changes(&old, &patch)?;
                return commit_symbol_strokes(session, command, &changes, StrokeEdit::Patch(&patch));
            }
            apply_stroke_patch(&mut session.document, &ids, &patch)?;
            let mut redo = command.clone();
            redo.params["ids"] = serde_json::json!(ids.iter().map(|id| id.0).collect::<Vec<_>>());
            session.push_restore_undo(&command.name, redo, old);
            Ok(serde_json::json!({
                "doc_version": session.generation,
                "changes": ids.iter().map(|id| serde_json::json!({ "name": command.name, "id": id.0 })).collect::<Vec<_>>()
            }))
        })());
    }
    Some((|| {
        let ids = stroke_targets(&command.params, &[])?;
        if command.params.get("ids").is_some() || ids.iter().any(|id| symbols::object_link(&session.document, *id).is_some()) {
            let changes = arrow_swaps(&session.document, &ids)?;
            return commit_symbol_strokes(session, command, &changes, StrokeEdit::SwapArrows);
        }
        let id = ids[0];
        let old = session.document.get_object(id).ok_or_else(CommandError::not_found)?.stroke.clone();
        run_stroke_command(&command.name, &mut session.document, &command.params)?;
        session.undo.push(crate::undo::Change {
            name: command.name.clone(),
            redo: command.clone(),
            undo: Command {
                name: "object.update".to_string(),
                params: serde_json::json!({ "id": id.0, "stroke": old }),
            },
        });
        session.generation += 1;
        session.document.version += 1;
        Ok(serde_json::json!({
            "doc_version": session.generation,
            "changes": [{ "name": command.name, "id": id.0 }]
        }))
    })())
}

fn arrow_swaps(doc: &Document, ids: &[ObjectId]) -> Result<Vec<(ObjectId, StrokeStyle, StrokeStyle)>, CommandError> {
    ids.iter().map(|id| {
        let before = editable_stroke(doc, *id)?.clone();
        let mut after = before.clone();
        std::mem::swap(&mut after.start_arrow, &mut after.end_arrow);
        Ok((*id, before, after))
    }).collect()
}

fn linked_changes(old: &[crate::Object], patch: &StrokePatch) -> Result<Vec<(ObjectId, StrokeStyle, StrokeStyle)>, CommandError> {
    old.iter().map(|object| {
        Ok((object.id, object.stroke.clone(), patch.applied(&object.stroke)?))
    }).collect()
}

fn commit_symbol_strokes(
    session: &mut Session, command: &Command, changes: &[(ObjectId, StrokeStyle, StrokeStyle)], edit: StrokeEdit<'_>,
) -> CommandResult {
    let mut document = session.document.clone();
    let kept_override = symbols::update_strokes(&mut document, changes, session.symbol_sync, edit)?;
    let changed_ids = changes.iter().filter(|(_, before, next)| before != next)
        .map(|(id, _, _)| serde_json::json!({"name": command.name, "id": id.0})).collect::<Vec<_>>();
    if changed_ids.is_empty() {
        return Ok(serde_json::json!({"doc_version": session.generation, "changes": []}));
    }
    super::document_change::commit(session, &command.name, document, session.selection.ids.clone(), serde_json::json!({
        "changes": changed_ids,
        "warnings": symbols::warnings(kept_override)
    }))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name == RESET_PROFILE {
        let Some(command) = reset_profile(session) else { return Some(Ok(())); };
        return apply(session, &command);
    }
    if command.name != STROKE_SET && command.name != STROKE_ARROWS_SWAP {
        return None;
    }
    Some((|| {
        if command.name == STROKE_SET {
            let ids = stroke_targets(&command.params, &[])?;
            if ids.iter().any(|id| symbols::object_link(&session.document, *id).is_some()) {
                let patch = StrokePatch::from_params(&command.params)?;
                let old = session.snapshot_objects(&ids);
                if old.len() != ids.len() {
                    return Err(CommandError::not_found());
                }
                for id in &ids {
                    editable_stroke(&session.document, *id)?;
                }
                return symbols::update_strokes(&mut session.document, &linked_changes(&old, &patch)?,
                    session.symbol_sync, StrokeEdit::Patch(&patch)).map(|_| ());
            }
        } else {
            let ids = stroke_targets(&command.params, &[])?;
            if command.params.get("ids").is_some() || ids.iter().any(|id| symbols::object_link(&session.document, *id).is_some()) {
                let changes = arrow_swaps(&session.document, &ids)?;
                return symbols::update_strokes(&mut session.document, &changes,
                    session.symbol_sync, StrokeEdit::SwapArrows).map(|_| ());
            }
        }
        run_stroke_command(&command.name, &mut session.document, &command.params).map(|_| ())
    })())
}
