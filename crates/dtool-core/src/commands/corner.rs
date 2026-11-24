use crate::command::Session;
use crate::commands::curve_edit::{commit_contours, editable_curve, world_contours};
use crate::curve::{
    corner_eligible, corner_handle_position, effective_radius, max_corner_radius, CornerKind, CornerSpec, PathPoint, PointId,
};
use crate::object::ObjectId;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const CORNER_SET: &str = "corner.set";
pub const CORNER_QUERY: &str = "corner.query";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        CORNER_SET => Some(plan_set(session, command).is_ok()),
        CORNER_QUERY => Some(plan_query(session, command).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        CORNER_SET => Some(set(session, command)),
        CORNER_QUERY => Some(query(session, command)),
        _ => None,
    }
}

fn parse_object_id(params: &Value) -> Result<ObjectId, CommandError> {
    let value = params.get("target").or_else(|| params.get("id")).ok_or_else(CommandError::invalid_params)?;
    let id = value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.strip_prefix("node:").unwrap_or(text).parse().ok()))
        .ok_or_else(CommandError::invalid_params)?;
    Ok(ObjectId(id))
}

fn parse_point_ids(params: &Value) -> Result<Vec<PointId>, CommandError> {
    let items = params
        .get("points")
        .or_else(|| params.get("nodes"))
        .and_then(Value::as_array)
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

fn parse_kind(params: &Value) -> Result<CornerKind, CommandError> {
    match params.get("kind").and_then(Value::as_str) {
        Some("none") => Ok(CornerKind::None),
        Some("rounded") => Ok(CornerKind::Rounded),
        Some("straight") => Ok(CornerKind::Straight),
        Some("concave") => Ok(CornerKind::Concave),
        Some("cutout") => Ok(CornerKind::Cutout),
        None => Ok(CornerKind::Rounded),
        _ => Err(CommandError::invalid_params()),
    }
}

fn locate_point(contours: &[crate::curve::Contour], id: PointId) -> Result<(usize, usize), CommandError> {
    for (contour_index, contour) in contours.iter().enumerate() {
        if let Some(point_index) = contour.points.iter().position(|point| point.id == id) {
            return Ok((contour_index, point_index));
        }
    }
    Err(CommandError::not_found())
}

fn plan_set(session: &Session, command: &Command) -> Result<(ObjectId, Vec<crate::curve::Contour>, Vec<(PointId, CornerSpec)>), CommandError> {
    let id = parse_object_id(&command.params)?;
    let object = editable_curve(session, id)?;
    let mut contours = world_contours(object);
    let ids = parse_point_ids(&command.params)?;
    let kind = parse_kind(&command.params)?;
    let radius = command
        .params
        .get("radius")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(CommandError::invalid_params)?;
    let mut locations = Vec::with_capacity(ids.len());
    for point_id in &ids {
        let (contour_index, point_index) = locate_point(&contours, *point_id)?;
        if !corner_eligible(&contours[contour_index], point_index) {
            return Err(CommandError::invalid_params());
        }
        locations.push((contour_index, point_index, *point_id));
    }
    let pending: Vec<PointId> = locations.iter().map(|(_, _, point_id)| *point_id).collect();
    let mut applied = Vec::with_capacity(locations.len());
    for (contour_index, point_index, point_id) in locations {
        let clamped = radius.min(max_corner_radius(&contours[contour_index], point_index, &pending));
        let spec = if kind == CornerKind::None {
            None
        } else {
            Some(CornerSpec { kind, radius: clamped })
        };
        contours[contour_index].points[point_index].corner = spec;
        applied.push((point_id, spec.unwrap_or(CornerSpec { kind: CornerKind::None, radius: 0.0 })));
    }
    Ok((id, contours, applied))
}

fn set(session: &mut Session, command: &Command) -> CommandResult {
    let (id, contours, applied) = plan_set(session, command)?;
    let points = applied
        .iter()
        .map(|(point_id, spec)| json!({ "id": point_id.0, "kind": spec.kind, "radius": spec.radius }))
        .collect::<Vec<_>>();
    commit_contours(session, id, contours, CORNER_SET, json!({ "points": points }))
}

fn plan_query(session: &Session, command: &Command) -> Result<(ObjectId, Vec<crate::curve::Contour>, Vec<PointId>), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let id = parse_object_id(&command.params)?;
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if object.curve().is_none() {
        return Err(CommandError::invalid_params());
    }
    let contours = world_contours(object);
    let ids = if command.params.get("points").or_else(|| command.params.get("nodes")).is_some() {
        parse_point_ids(&command.params)?
    } else {
        contours.iter().flat_map(|contour| contour.points.iter().map(|point| point.id)).collect()
    };
    for point_id in &ids {
        locate_point(&contours, *point_id)?;
    }
    Ok((id, contours, ids))
}

fn query(session: &Session, command: &Command) -> CommandResult {
    let (id, contours, ids) = plan_query(session, command)?;
    let mut points = Vec::with_capacity(ids.len());
    for point_id in ids {
        let (contour_index, point_index) = locate_point(&contours, point_id)?;
        let contour = &contours[contour_index];
        let point: &PathPoint = &contour.points[point_index];
        let eligible = corner_eligible(contour, point_index);
        let kind = point.corner.map(|spec| spec.kind).unwrap_or(CornerKind::None);
        let radius = point.corner.map(|spec| spec.radius).unwrap_or(0.0);
        let effective = effective_radius(contour, point_index);
        let handle = corner_handle_position(contour, point_index);
        points.push(json!({
            "id": point_id.0,
            "eligible": eligible,
            "kind": kind,
            "radius": radius,
            "effective_radius": effective,
            "handle": handle
        }));
    }
    Ok(json!({ "id": id.0, "points": points }))
}
