use crate::command::Session;
use crate::commands::curve_edit::{commit_contours_with_transform, editable_curve, editable_curve_impl, preserve_transform, world_contours};
use crate::commands::curve_nodes::{segment, segment_count, CURVE_NODES_CONVERT, CURVE_NODES_DELETE};
use crate::curve::{Contour, NodeKind, PathPoint, PointId};
use crate::object::ObjectId;
use dtool_protocol::{Command, CommandError, CommandResult};
use kurbo::{ParamCurve, ParamCurveArclen, ParamCurveDeriv};
use serde_json::{json, Value};

pub const POINTS_TRANSFORM: &str = "path.points.transform";
pub const POINT_MOVE: &str = "path.point.move";
pub const POINT_HANDLE: &str = "path.point.handle";
pub const SELECTION_SET: &str = "ui.path.selection.set";
pub const POINT_DELETE: &str = "path.point.delete";
pub const POINT_TYPE: &str = "path.point.type";
pub const SEGMENT_EDIT: &str = "path.segment.edit";
pub const POINTS_BRIDGE: &str = "path.points.bridge";
pub const REVERSE: &str = "path.reverse";
pub const SMOOTH: &str = "path.smooth";
pub const NAMES: [&str; 10] = [POINTS_TRANSFORM, POINT_MOVE, POINT_HANDLE, SELECTION_SET, POINT_DELETE, POINT_TYPE, SEGMENT_EDIT, POINTS_BRIDGE, REVERSE, SMOOTH];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    None,
    Angle,
    Full,
}

struct Plan {
    id: ObjectId,
    contours: Vec<Contour>,
    extra: Value,
    preserve_transform: bool,
}

pub fn handles(command: &Command) -> bool {
    NAMES.contains(&command.name.as_str())
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !handles(command) {
        return None;
    }
    Some(match command.name.as_str() {
        SELECTION_SET => selection(session, &command.params).is_ok(),
        POINT_TYPE => delegate(session, command).is_ok_and(|alias| crate::commands::curve_nodes::can_execute(session, &alias).unwrap_or(false)),
        POINT_DELETE if !keep_shape(&command.params).unwrap_or(false) => delegate(session, command).is_ok_and(|alias| crate::commands::curve_nodes::can_execute(session, &alias).unwrap_or(false)),
        _ => plan(session, command).is_ok(),
    })
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !handles(command) {
        return None;
    }
    Some(execute(session, command))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    run(session, command).map(|result| result.map(|_| ()))
}

fn alias(session: &mut Session, command: &Command) -> CommandResult {
    let alias = delegate(session, command)?;
    crate::commands::curve_nodes::run(session, &alias).unwrap_or_else(|| Err(CommandError::not_implemented()))
}

fn execute(session: &mut Session, command: &Command) -> CommandResult {
    let keep = keep_shape(&command.params)?;
    match command.name.as_str() {
        SELECTION_SET => selection(session, &command.params),
        POINT_TYPE => alias(session, command),
        POINT_DELETE if !keep => alias(session, command),
        _ => {
            let plan = plan(session, command)?;
            commit_contours_with_transform(session, plan.id, plan.contours, &command.name, plan.extra, plan.preserve_transform)
        }
    }
}

fn object_id(params: &Value) -> Result<ObjectId, CommandError> {
    params.get("id").and_then(Value::as_u64).map(ObjectId).ok_or_else(CommandError::invalid_params)
}

fn point_id(value: &Value) -> Option<PointId> {
    value.as_u64().or_else(|| value.as_str().map(|t| t.strip_prefix("node:").unwrap_or(t)).and_then(|t| t.trim().parse().ok())).map(PointId)
}

fn point_ids(params: &Value) -> Result<Vec<PointId>, CommandError> {
    let items: Vec<Value> = match (params.get("points"), params.get("point")) {
        (Some(Value::Array(items)), None) => items.clone(),
        (None, Some(single)) => vec![single.clone()],
        _ => return Err(CommandError::invalid_params()),
    };
    let mut ids = Vec::with_capacity(items.len());
    for item in &items {
        let id = point_id(item).ok_or_else(CommandError::invalid_params)?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err(CommandError::invalid_params());
    }
    Ok(ids)
}

fn number(params: &Value, key: &str, default: Option<f64>) -> Result<f64, CommandError> {
    let value = match params.get(key) {
        None | Some(Value::Null) => default.ok_or_else(CommandError::invalid_params)?,
        Some(value) => value.as_f64().ok_or_else(CommandError::invalid_params)?,
    };
    if value.is_finite() { Ok(value) } else { Err(CommandError::invalid_params()) }
}

fn pair(params: &Value, key: &str) -> Result<[f64; 2], CommandError> {
    let items = params.get(key).and_then(Value::as_array).filter(|v| v.len() == 2).ok_or_else(CommandError::invalid_params)?;
    let p = [items[0].as_f64().ok_or_else(CommandError::invalid_params)?, items[1].as_f64().ok_or_else(CommandError::invalid_params)?];
    if p.iter().all(|v| v.is_finite()) { Ok(p) } else { Err(CommandError::invalid_params()) }
}

fn keep_shape(params: &Value) -> Result<bool, CommandError> {
    match params.get("keep_shape") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(CommandError::invalid_params()),
    }
}

fn locate(contours: &[Contour], id: PointId) -> Option<(usize, usize)> {
    contours.iter().enumerate().find_map(|(c, contour)| contour.points.iter().position(|p| p.id == id).map(|i| (c, i)))
}

fn subpath(params: &Value, contours: &[Contour]) -> Result<usize, CommandError> {
    let index = match params.get("subpath").or_else(|| params.get("contour")) {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().ok_or_else(CommandError::invalid_params)? as usize,
    };
    if index < contours.len() { Ok(index) } else { Err(CommandError::invalid_params()) }
}

fn delegate(session: &Session, command: &Command) -> Result<Command, CommandError> {
    let params = &command.params;
    let id = object_id(params)?;
    let contours = world_contours(editable_curve(session, id)?);
    let ids = point_ids(params)?;
    let (index, _) = locate(&contours, ids[0]).ok_or_else(CommandError::not_found)?;
    let nodes: Vec<u64> = ids.iter().map(|p| p.0).collect();
    Ok(match command.name.as_str() {
        POINT_TYPE => {
            let kind = params.get("type").or_else(|| params.get("kind")).cloned().ok_or_else(CommandError::invalid_params)?;
            Command::with_params(CURVE_NODES_CONVERT, json!({"id": id.0, "contour": index, "nodes": nodes, "kind": kind}))
        }
        _ => Command::with_params(CURVE_NODES_DELETE, json!({"id": id.0, "contour": index, "nodes": nodes})),
    })
}

fn selection(session: &Session, params: &Value) -> CommandResult {
    let id = object_id(params)?;
    let contours = world_contours(editable_curve(session, id)?);
    let ids = match params.get("points") {
        Some(Value::Array(items)) if items.is_empty() => Vec::new(),
        _ => point_ids(params)?,
    };
    if ids.iter().any(|p| locate(&contours, *p).is_none()) {
        return Err(CommandError::not_found());
    }
    Ok(json!({"id": id.0, "points": ids.iter().map(|p| p.0).collect::<Vec<_>>(), "changed": false, "doc_version": session.generation}))
}

fn plan(session: &Session, command: &Command) -> Result<Plan, CommandError> {
    let params = &command.params;
    let id = object_id(params)?;
    let preserve_transform = command.name == REVERSE && preserve_transform(params)?;
    let mut contours = world_contours(editable_curve_impl(session, id, preserve_transform)?);
    let extra = match command.name.as_str() {
        POINTS_TRANSFORM | POINT_MOVE => transform(&mut contours, params)?,
        POINT_HANDLE => handle(&mut contours, params)?,
        POINT_DELETE => delete_keep_shape(&mut contours, params)?,
        SEGMENT_EDIT => segment_edit(&mut contours, params)?,
        POINTS_BRIDGE => bridge(&mut contours, params)?,
        REVERSE => {
            let index = subpath(params, &contours)?;
            reverse(&mut contours[index]);
            json!({"subpath": index, "points": contours[index].points.iter().map(|p| p.id.0).collect::<Vec<_>>()})
        }
        SMOOTH => smooth(&mut contours, params)?,
        _ => return Err(CommandError::invalid_params()),
    };
    Ok(Plan { id, contours, extra, preserve_transform })
}

fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn scale(a: [f64; 2], s: f64) -> [f64; 2] {
    [a[0] * s, a[1] * s]
}

fn length(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

fn unit(a: [f64; 2]) -> Option<[f64; 2]> {
    let l = length(a);
    (l > 1e-9 && l.is_finite()).then(|| scale(a, 1.0 / l))
}

fn shift(point: &mut PathPoint, delta: [f64; 2]) {
    point.pos = add(point.pos, delta);
    point.handle_in = point.handle_in.map(|h| add(h, delta));
    point.handle_out = point.handle_out.map(|h| add(h, delta));
}

fn transform(contours: &mut [Contour], params: &Value) -> Result<Value, CommandError> {
    let ids = point_ids(params)?;
    let mut found = Vec::with_capacity(ids.len());
    for id in &ids {
        found.push(locate(contours, *id).ok_or_else(CommandError::not_found)?);
    }
    let delta = match params.get("pos") {
        Some(_) if ids.len() == 1 => {
            let (c, i) = found[0];
            sub(pair(params, "pos")?, contours[c].points[i].pos)
        }
        Some(_) => return Err(CommandError::invalid_params()),
        None => [number(params, "dx", Some(0.0))?, number(params, "dy", Some(0.0))?],
    };
    for (c, i) in &found {
        shift(&mut contours[*c].points[*i], delta);
    }
    Ok(json!({"points": ids.iter().map(|p| p.0).collect::<Vec<_>>(), "delta": delta}))
}

fn mirror(params: &Value) -> Result<Mirror, CommandError> {
    match params.get("mirror") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(Mirror::None),
        Some(Value::Bool(true)) => Ok(Mirror::Full),
        Some(Value::String(text)) => match text.as_str() {
            "none" => Ok(Mirror::None),
            "angle" => Ok(Mirror::Angle),
            "full" => Ok(Mirror::Full),
            _ => Err(CommandError::invalid_params()),
        },
        _ => Err(CommandError::invalid_params()),
    }
}

fn handle(contours: &mut [Contour], params: &Value) -> Result<Value, CommandError> {
    let ids = point_ids(params)?;
    if ids.len() != 1 {
        return Err(CommandError::invalid_params());
    }
    let (c, i) = locate(contours, ids[0]).ok_or_else(CommandError::not_found)?;
    let out = match params.get("side").and_then(Value::as_str) {
        Some("out") => true,
        Some("in") => false,
        _ => return Err(CommandError::invalid_params()),
    };
    let pos = pair(params, "pos")?;
    let mode = mirror(params)?;
    let point = &mut contours[c].points[i];
    let arm = sub(pos, point.pos);
    let opposite = if out { point.handle_in } else { point.handle_out };
    let mirrored = match mode {
        Mirror::None => opposite,
        Mirror::Full => Some(sub(point.pos, arm)),
        Mirror::Angle => match (unit(arm), opposite) {
            (Some(direction), Some(other)) => Some(sub(point.pos, scale(direction, length(sub(other, point.pos))))),
            (Some(direction), None) => Some(sub(point.pos, scale(direction, length(arm)))),
            (None, other) => other,
        },
    };
    if out {
        point.handle_out = Some(pos);
        point.handle_in = mirrored;
    } else {
        point.handle_in = Some(pos);
        point.handle_out = mirrored;
    }
    point.kind = if mode == Mirror::None { NodeKind::Sharp } else { NodeKind::Smooth };
    let mode = match mode {
        Mirror::None => "none",
        Mirror::Angle => "angle",
        Mirror::Full => "full",
    };
    Ok(json!({"point": ids[0].0, "side": if out { "out" } else { "in" }, "mirror": mode}))
}

fn span_length(contour: &Contour, from: usize) -> f64 {
    (from..from + 2).filter_map(|s| segment(contour, s % segment_count(contour).max(1))).map(|bez| bez.arclen(1e-6)).sum()
}

fn delete_keep_shape(contours: &mut [Contour], params: &Value) -> Result<Value, CommandError> {
    let ids = point_ids(params)?;
    for id in &ids {
        let (c, i) = locate(contours, *id).ok_or_else(CommandError::not_found)?;
        let contour = &mut contours[c];
        let n = contour.points.len();
        if n <= 2 {
            return Err(CommandError::invalid_params());
        }
        let interior = contour.closed || (i > 0 && i + 1 < n);
        if interior {
            let prev = (i + n - 1) % n;
            let next = (i + 1) % n;
            let total = span_length(contour, prev);
            let before = segment(contour, prev).ok_or_else(CommandError::invalid_params)?;
            let after = segment(contour, i).ok_or_else(CommandError::invalid_params)?;
            let start = before.deriv().eval(0.0);
            let end = after.deriv().eval(1.0);
            let a = contour.points[prev].pos;
            let b = contour.points[next].pos;
            let chord = sub(b, a);
            let out_dir = unit([start.x, start.y]).or_else(|| unit(chord)).ok_or_else(CommandError::invalid_params)?;
            let in_dir = unit([end.x, end.y]).or_else(|| unit(chord)).ok_or_else(CommandError::invalid_params)?;
            let reach = total / 3.0;
            contour.points[prev].handle_out = Some(add(a, scale(out_dir, reach)));
            contour.points[next].handle_in = Some(sub(b, scale(in_dir, reach)));
        }
        contour.points.remove(i);
        if !contour.closed {
            if let Some(first) = contour.points.first_mut() {
                first.handle_in = None;
            }
            if let Some(last) = contour.points.last_mut() {
                last.handle_out = None;
            }
        }
    }
    Ok(json!({"points": ids.iter().map(|p| p.0).collect::<Vec<_>>(), "keep_shape": true}))
}

fn segment_edit(contours: &mut [Contour], params: &Value) -> Result<Value, CommandError> {
    let index = subpath(params, contours)?;
    let contour = &mut contours[index];
    let s = params.get("segment").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)? as usize;
    let bez = segment(contour, s).ok_or_else(CommandError::invalid_params)?;
    let t = number(params, "t", Some(0.5))?;
    if !(t > 0.0 && t < 1.0) {
        return Err(CommandError::invalid_params());
    }
    let target = pair(params, "pos")?;
    let current = bez.eval(t);
    let delta = sub(target, [current.x, current.y]);
    let weight = 3.0 * (1.0 - t) * t;
    let step = scale(delta, 1.0 / weight);
    let n = contour.points.len();
    let next = (s + 1) % n;
    contour.points[s].handle_out = Some(add([bez.p1.x, bez.p1.y], step));
    contour.points[next].handle_in = Some(add([bez.p2.x, bez.p2.y], step));
    Ok(json!({"subpath": index, "segment": s, "t": t, "pos": target}))
}

fn reverse(contour: &mut Contour) {
    contour.points.reverse();
    for point in &mut contour.points {
        std::mem::swap(&mut point.handle_in, &mut point.handle_out);
    }
}

fn bridge(contours: &mut Vec<Contour>, params: &Value) -> Result<Value, CommandError> {
    let ids = point_ids(params)?;
    if ids.len() != 2 {
        return Err(CommandError::invalid_params());
    }
    let (ca, ia) = locate(contours, ids[0]).ok_or_else(CommandError::not_found)?;
    let (cb, ib) = locate(contours, ids[1]).ok_or_else(CommandError::not_found)?;
    let end = |c: usize, i: usize| !contours[c].closed && (i == 0 || i + 1 == contours[c].points.len());
    if !end(ca, ia) || !end(cb, ib) {
        return Err(CommandError::new("NOT_AN_ENDPOINT"));
    }
    if ca == cb {
        if ia == ib || contours[ca].points.len() < 3 {
            return Err(CommandError::invalid_params());
        }
        contours[ca].closed = true;
        return Ok(json!({"subpath": ca, "closed": true, "points": [ids[0].0, ids[1].0]}));
    }
    let mut first = contours[ca].clone();
    let mut second = contours[cb].clone();
    if ia == 0 && first.points.len() > 1 {
        reverse(&mut first);
    }
    if ib + 1 == second.points.len() && second.points.len() > 1 {
        reverse(&mut second);
    }
    if let Some(last) = first.points.last_mut() {
        last.handle_out = None;
    }
    if let Some(head) = second.points.first_mut() {
        head.handle_in = None;
    }
    first.points.extend(second.points);
    first.closed = false;
    let (keep, drop) = if ca < cb { (ca, cb) } else { (cb, ca) };
    contours[keep] = first;
    contours.remove(drop);
    Ok(json!({"subpath": keep, "closed": false, "removed_subpath": drop, "points": [ids[0].0, ids[1].0]}))
}

fn smooth(contours: &mut [Contour], params: &Value) -> Result<Value, CommandError> {
    let index = subpath(params, contours)?;
    let amount = number(params, "amount", Some(1.0))?;
    if !(0.0..=1.0).contains(&amount) {
        return Err(CommandError::invalid_params());
    }
    let contour = &mut contours[index];
    let n = contour.points.len();
    if n < 2 {
        return Err(CommandError::invalid_params());
    }
    let positions: Vec<[f64; 2]> = contour.points.iter().map(|p| p.pos).collect();
    let closed = contour.closed;
    for (i, point) in contour.points.iter_mut().enumerate() {
        let pos = point.pos;
        let prev = if i > 0 { positions[i - 1] } else if closed { positions[n - 1] } else { positions[i] };
        let next = if i + 1 < n { positions[i + 1] } else if closed { positions[0] } else { positions[i] };
        let tangent = scale(sub(next, prev), 1.0 / 6.0);
        let target_in = sub(pos, tangent);
        let target_out = add(pos, tangent);
        let blend = |current: Option<[f64; 2]>, target: [f64; 2]| {
            let from = current.unwrap_or(pos);
            add(from, scale(sub(target, from), amount))
        };
        let open_start = !closed && i == 0;
        let open_end = !closed && i + 1 == n;
        point.handle_in = if open_start { point.handle_in } else { Some(blend(point.handle_in, target_in)) };
        point.handle_out = if open_end { point.handle_out } else { Some(blend(point.handle_out, target_out)) };
        if amount > 0.0 {
            point.kind = NodeKind::Smooth;
        }
    }
    Ok(json!({"subpath": index, "amount": amount}))
}
