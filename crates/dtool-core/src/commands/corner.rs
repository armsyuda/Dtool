use crate::command::Session;
use crate::curve::{
    Contour, CornerKind, CornerSpec, Curve, PointId, corner_eligible, corner_handle_position,
    effective_radius, max_corner_radius,
};
use crate::object::{Object, ObjectId, ObjectKind};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;

pub const TOOL: &str = "tool.corner";
pub const CORNER_SET: &str = "corner.set";
pub const CORNER_QUERY: &str = "corner.query";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Anchor {
    pub contour: usize,
    pub id: PointId,
}

pub fn source(object: &Object) -> Option<Cow<'_, Curve>> {
    if let Some(curve) = object.curve() {
        return Some(Cow::Borrowed(curve));
    }
    if !matches!(object.kind, ObjectKind::Shape) || crate::commands::qr::payload(object).is_some() {
        return None;
    }
    let path = crate::outline::base_object_outline(object, crate::boolean::DEFAULT_TOLERANCE)?;
    Some(Cow::Owned(Curve::from_kurbo(&path).with_source(object.id.0)))
}

pub fn has_corners(object: &Object) -> bool {
    object.curve().is_some_and(|curve| curve.contours().any(|contour| {
        contour.points.iter().enumerate().any(|(index, _)| effective_radius(contour, index) > 0.0)
    }))
}

pub fn selected(session: &Session) -> Option<&Object> {
    let [id] = session.selection.ids.as_slice() else { return None; };
    session.document.get_object(*id)
}

pub fn editable(session: &Session, object: &Object) -> bool {
    !session.document.is_object_locked(object)
        && (object.curve().is_some() || crate::commands::symbols::object_link(&session.document, object.id).is_none())
}

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

fn target<'a>(session: &'a Session, params: &Value) -> Result<&'a Object, CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let id = params.get("target").or_else(|| params.get("id")).ok_or_else(CommandError::invalid_params)?;
    let id = id.as_u64().or_else(|| id.as_str().and_then(|text| text.strip_prefix("node:").unwrap_or(text).parse().ok()))
        .ok_or_else(CommandError::invalid_params)?;
    session.document.get_object(ObjectId(id)).ok_or_else(CommandError::not_found)
}

fn locate(contours: &[Contour], anchor: Anchor) -> Result<usize, CommandError> {
    contours.get(anchor.contour).and_then(|contour| contour.points.iter().position(|point| point.id == anchor.id))
        .ok_or_else(CommandError::not_found)
}

fn requested_anchors(contours: &[Contour], params: &Value) -> Result<Option<Vec<Anchor>>, CommandError> {
    let Some(items) = params.get("points").or_else(|| params.get("nodes")) else { return Ok(None); };
    let items = items.as_array().filter(|items| !items.is_empty()).ok_or_else(CommandError::invalid_params)?;
    let mut anchors = Vec::with_capacity(items.len());
    for item in items {
        let anchor = if let Some(id) = item.as_u64() {
            let id = PointId(id);
            let contour = contours.iter().position(|contour| contour.points.iter().any(|point| point.id == id))
                .ok_or_else(CommandError::not_found)?;
            Anchor { contour, id }
        } else {
            serde_json::from_value(item.clone()).map_err(|_| CommandError::invalid_params())?
        };
        locate(contours, anchor)?;
        if !anchors.contains(&anchor) { anchors.push(anchor); }
    }
    Ok(Some(anchors))
}

#[derive(Deserialize)]
struct Options {
    kind: Option<CornerKind>,
    #[serde(default)]
    preserve_kind: bool,
    radius: f64,
    #[serde(default)]
    relative: bool,
}

pub fn command(id: ObjectId, points: &[Anchor], kind: Option<CornerKind>, radius: f64, relative: bool) -> Command {
    Command::with_params(CORNER_SET, json!({"id":id,"points":points,"kind":kind,"preserve_kind":kind.is_none(),"radius":radius,"relative":relative}))
}

struct CornerChange {
    object: Object,
    curve: Curve,
    points: Vec<Value>,
    changed: bool,
}

fn plan_set(session: &Session, command: &Command) -> Result<CornerChange, CommandError> {
    let object = target(session, &command.params)?;
    if session.document.is_object_locked(object) { return Err(CommandError::new("LOCKED")); }
    if !editable(session, object) { return Err(CommandError::new("UNSUPPORTED_SHAPE")); }
    let mut curve = source(object).ok_or_else(CommandError::invalid_params)?.into_owned();
    let originals = curve.contours().cloned().collect::<Vec<_>>();
    let anchors = requested_anchors(&originals, &command.params)?.ok_or_else(CommandError::invalid_params)?;
    let options: Options = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
    if !options.radius.is_finite() || options.radius < 0.0 || options.relative && options.radius > 100.0 {
        return Err(CommandError::invalid_params());
    }
    let mut pending = vec![Vec::new(); originals.len()];
    for anchor in &anchors {
        pending[anchor.contour].push(anchor.id);
    }
    let mut points = Vec::with_capacity(anchors.len());
    let mut changed = false;
    for anchor in anchors {
        let index = locate(&originals, anchor)?;
        let original = &originals[anchor.contour];
        if !corner_eligible(original, index) { return Err(CommandError::invalid_params()); }
        let maximum = max_corner_radius(original, index, &pending[anchor.contour]);
        let kind = options.kind.or_else(|| options.preserve_kind.then(|| original.points[index].corner).flatten().map(|spec| spec.kind))
            .unwrap_or(CornerKind::Rounded);
        let spec = (kind != CornerKind::None).then(|| CornerSpec {
            kind,
            radius: if options.relative { maximum * options.radius / 100.0 } else { options.radius.min(maximum) },
        });
        let contour = if anchor.contour == 0 { &mut curve.contour } else { &mut curve.holes[anchor.contour - 1] };
        changed |= contour.points[index].corner != spec;
        contour.points[index].corner = spec;
        points.push(json!({"id":anchor.id,"contour":anchor.contour,"kind":kind,"radius":spec.map_or(0.0, |spec| spec.radius)}));
    }
    Ok(CornerChange { object: object.clone(), curve, points, changed })
}

fn set(session: &mut Session, command: &Command) -> CommandResult {
    let CornerChange { object, curve, points, changed } = plan_set(session, command)?;
    let id = object.id;
    if !changed { return Ok(json!({"id":id,"changed":false,"doc_version":session.generation,"points":points})); }
    let mut next = object.clone();
    next.kind = ObjectKind::Curve(curve);
    next.shape = None;
    let reply = json!({"id":id,"changed":true,"points":points});
    if let Some(result) = crate::commands::symbols::curves::apply_edit(session, &object, &next, CORNER_SET, reply.clone()) {
        return result;
    }
    let before = crate::commands::arrange::snapshot(session);
    *session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)? = next;
    crate::commands::arrange::commit_snapshot(session, CORNER_SET, before, reply)
}

fn plan_query(session: &Session, command: &Command) -> Result<(ObjectId, Vec<Contour>, Vec<Anchor>), CommandError> {
    let object = target(session, &command.params)?;
    let curve = source(object).ok_or_else(CommandError::invalid_params)?;
    let contours = curve.contours().map(|contour| crate::commands::curve_edit::offset_contour(contour, object.pos.map(f64::from)))
        .collect::<Vec<_>>();
    let anchors = requested_anchors(&contours, &command.params)?.unwrap_or_else(|| {
        contours.iter().enumerate().flat_map(|(contour, path)| path.points.iter().map(move |point| Anchor { contour, id: point.id })).collect()
    });
    Ok((object.id, contours, anchors))
}

fn query(session: &Session, command: &Command) -> CommandResult {
    let (id, contours, anchors) = plan_query(session, command)?;
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    let matrix = crate::geometry::object_matrix(object);
    let mut points = Vec::with_capacity(anchors.len());
    for anchor in anchors {
        let index = locate(&contours, anchor)?;
        let contour = &contours[anchor.contour];
        let point = &contour.points[index];
        let kind = point.corner.map_or(CornerKind::None, |spec| spec.kind);
        let radius = point.corner.map_or(0.0, |spec| spec.radius);
        let handle = corner_handle_position(contour, index).map(|position| {
            if matrix == crate::geometry::IDENTITY { position }
            else { crate::geometry::apply_matrix(&matrix, position.map(|coordinate| coordinate as f32)).map(f64::from) }
        });
        points.push(json!({
            "id": anchor.id, "contour": anchor.contour, "eligible": corner_eligible(contour, index),
            "kind": kind, "radius": radius, "effective_radius": effective_radius(contour, index), "handle": handle
        }));
    }
    Ok(json!({"id":id,"points":points}))
}
