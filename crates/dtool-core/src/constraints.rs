use crate::geometry::{compose, invert_matrix, object_matrix, rect_quad};
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, Session};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const SET: &str = "constraint.set";
const QUERY: &str = "constraint.query";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Constraint {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
    pub scale_w: bool,
    pub scale_h: bool,
    #[serde(default, skip_serializing_if = "Fit::is_none")]
    pub fit: Fit,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    #[default]
    None,
    Min,
    Max,
}

impl Fit {
    fn is_none(&self) -> bool { *self == Self::None }
}

fn axis(old: f32, new: f32, start: f32, size: f32, low: bool, high: bool, size_factor: f32) -> (f32, f32) {
    let factor = if old > 0.0 { new / old } else { 1.0 };
    let far = old - start - size;
    let extent = if low && high {
        (new - start - far).max(0.0)
    } else {
        size * size_factor
    };
    let origin = if low {
        start
    } else if high {
        new - far - extent
    } else {
        (start + size * 0.5) * factor - extent * 0.5
    };
    (origin, extent)
}

pub(crate) fn child_resize_map(parent: &Object, next: &Object, child: &Object) -> Option<[f32; 6]> {
    let rule = child.constraint?;
    let old = parent.size.map(f32::abs);
    let new = next.size.map(f32::abs);
    if old == new {
        return None;
    }
    let inverse = invert_matrix(&object_matrix(parent))?;
    let matrix = compose(&inverse, &object_matrix(child));
    let quad = rect_quad([child.pos[0], child.pos[1], child.size[0].abs(), child.size[1].abs()], &matrix);
    let bounds = |i| {
        let low = quad.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min);
        let high = quad.iter().map(|p| p[i]).fold(f32::NEG_INFINITY, f32::max);
        (low, high - low)
    };
    let (x, width) = bounds(0);
    let (y, height) = bounds(1);
    let factors = std::array::from_fn::<_, 2, _>(|i| if old[i] > 0.0 { new[i] / old[i] } else { 1.0 });
    let size_factors = match rule.fit {
        Fit::None => [if rule.scale_w { factors[0] } else { 1.0 }, if rule.scale_h { factors[1] } else { 1.0 }],
        Fit::Min => [factors[0].min(factors[1]); 2],
        Fit::Max => [factors[0].max(factors[1]); 2],
    };
    let (new_x, new_w) = axis(old[0], new[0], x - parent.pos[0], width, rule.left, rule.right, size_factors[0]);
    let (new_y, new_h) = axis(old[1], new[1], y - parent.pos[1], height, rule.top, rule.bottom, size_factors[1]);
    let ratio = |extent: f32, desired: f32| if extent > 0.0 { (desired / extent).max(1e-6) } else { 1.0 };
    let (sx, sy) = (ratio(width, new_w), ratio(height, new_h));
    let local = [sx, 0.0, 0.0, sy, next.pos[0] + new_x - sx * x, next.pos[1] + new_y - sy * y];
    let map = compose(&object_matrix(next), &compose(&local, &inverse));
    map.iter().all(|value| value.is_finite()).then_some(map)
}

pub fn target_parent(doc: &Document, id: ObjectId) -> Option<(&Object, &Object)> {
    let child = doc.get_object(id).filter(|object| !object.locked)?;
    let parent = doc.get_object(child.parent_id?).filter(|object| object.is_group())?;
    Some((child, parent))
}

fn target_ids(session: &Session, params: &Value) -> Result<Vec<ObjectId>, CommandError> {
    let ids = crate::commands::stroke::stroke_targets(params, &session.selection.ids)?;
    let explicit = ["id", "ids", "object_id", "node"].iter().any(|key| params.get(key).is_some());
    let mut unique = std::collections::HashSet::new();
    let mut targets = Vec::new();
    for id in ids {
        if target_parent(&session.document, id).is_some() {
            if unique.insert(id) {
                targets.push(id);
            }
        } else if explicit {
            return Err(CommandError::invalid_params());
        }
    }
    if targets.is_empty() { return Err(CommandError::invalid_params()); }
    Ok(targets)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConstraintPatch {
    left: Option<bool>,
    right: Option<bool>,
    top: Option<bool>,
    bottom: Option<bool>,
    scale_w: Option<bool>,
    scale_h: Option<bool>,
    fit: Option<Fit>,
}

pub(crate) enum Edit {
    Replace(Option<Constraint>),
    Patch(ConstraintPatch),
}

impl Edit {
    pub(crate) fn apply(&self, value: Option<Constraint>) -> Option<Constraint> {
        match self {
            Self::Replace(rule) => *rule,
            Self::Patch(patch) => {
                let current = value.unwrap_or_default();
                Some(Constraint {
                    left: patch.left.unwrap_or(current.left),
                    right: patch.right.unwrap_or(current.right),
                    top: patch.top.unwrap_or(current.top),
                    bottom: patch.bottom.unwrap_or(current.bottom),
                    scale_w: patch.scale_w.unwrap_or(current.scale_w),
                    scale_h: patch.scale_h.unwrap_or(current.scale_h),
                    fit: patch.fit.unwrap_or(current.fit),
                })
            }
        }
    }
}

fn set_plan(session: &Session, command: &Command) -> Result<(Vec<ObjectId>, Edit), CommandError> {
    let ids = target_ids(session, &command.params)?;
    let edit = match (command.params.get("constraint"), command.params.get("patch")) {
        (Some(value), None) => Edit::Replace(serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?),
        (None, Some(Value::Object(fields))) if !fields.is_empty() && fields.values().all(|value| !value.is_null()) => {
            Edit::Patch(serde_json::from_value(Value::Object(fields.clone())).map_err(|_| CommandError::invalid_params())?)
        }
        _ => return Err(CommandError::invalid_params()),
    };
    Ok((ids, edit))
}

pub fn status(session: &Session) -> Value {
    let layers: Vec<_> = session.selection.ids.iter().filter_map(|&id| {
        let (child, parent) = target_parent(&session.document, id)?;
        Some(json!({
            "id": id, "constraint": child.constraint, "parent": parent.id,
            "parent_frame": [parent.pos[0], parent.pos[1], parent.size[0].abs(), parent.size[1].abs()],
            "rotation_deg": parent.rotation_deg, "shear_deg": parent.shear_deg,
        }))
    }).collect();
    let mixed = layers.first().is_some_and(|first| layers.iter().any(|layer| layer["constraint"] != first["constraint"]));
    json!({ "available": !layers.is_empty(), "mixed": mixed, "layers": layers })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        SET | "constraints.set" => Some(set_plan(session, command).is_ok()),
        QUERY | "constraints.query" => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        SET | "constraints.set" => Some(set_plan(session, command).and_then(|(ids, edit)| {
            let mut updated = session.document.clone();
            let (changed, kept_override) = crate::commands::symbols::constraints::update(&mut updated, &ids, &edit, session.symbol_sync)?;
            if !changed {
                return Ok(json!({"ids": ids, "changes": [], "doc_version": session.generation}));
            }
            let warnings = crate::commands::symbols::warnings(kept_override);
            crate::commands::document_change::commit(session, SET, updated, session.selection.ids.clone(),
                json!({"ids": ids, "warnings": warnings}))
        })),
        QUERY | "constraints.query" => Some(Ok(status(session))),
        _ => None,
    }
}
