#[path = "corner.rs"]
pub mod corner;

use crate::command::Session;
use crate::commands::curve_edit::{commit_contours_with_transform, contour_index, editable_curve_impl, preserve_transform, world_contours};
use crate::curve::{Contour, NodeKind, PathPoint, PointId};
use crate::object::ObjectId;
use dtool_protocol::{Command, CommandError, CommandResult};
use kurbo::{CubicBez, Line, ParamCurve, ParamCurveNearest, Point};
use serde_json::{json, Value};

pub const CURVE_NODES_INSERT: &str = "curve.nodes.insert";
pub const CURVE_NODES_DELETE: &str = "curve.nodes.delete";
pub const CURVE_NODES_CONVERT: &str = "curve.nodes.convert";
pub const SPLIT_EPSILON: f64 = 1e-4;
pub const HANDLE_FRACTION: f64 = 1.0 / 3.0;
pub const SMART_FRACTION: f64 = 1.0 / 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conversion {
    Sharp,
    Smooth,
    Symmetric,
    Smart,
}

struct Plan {
    id: ObjectId,
    contours: Vec<Contour>,
    extra: Value,
    preserve_transform: bool,
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if let Some(value) = corner::can_execute(session, command) {
        return Some(value);
    }
    match command.name.as_str() {
        CURVE_NODES_INSERT | CURVE_NODES_DELETE | CURVE_NODES_CONVERT => Some(plan(session, command).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if let Some(result) = corner::run(session, command) {
        return Some(result);
    }
    match command.name.as_str() {
        CURVE_NODES_INSERT | CURVE_NODES_DELETE | CURVE_NODES_CONVERT => Some(plan(session, command).and_then(|p| {
            commit_contours_with_transform(session, p.id, p.contours, &command.name, p.extra, p.preserve_transform)
        })),
        _ => None,
    }
}

fn plan(session: &Session, command: &Command) -> Result<Plan, CommandError> {
    let params = &command.params;
    let id = ObjectId(params.get("id").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?);
    let preserve_transform = preserve_transform(params)?;
    let mut contours = world_contours(editable_curve_impl(session, id, preserve_transform)?);
    let index = contour_index(params, "contour", contours.len())?;
    let extra = match command.name.as_str() {
        CURVE_NODES_INSERT => {
            let node = insert(&mut contours[index], params)?;
            json!({ "contour": index, "node": node.0 })
        }
        CURVE_NODES_DELETE => {
            let ids = node_ids(params)?;
            delete(&mut contours, index, &ids)?;
            json!({ "contour": index, "nodes": ids.iter().map(|i| i.0).collect::<Vec<_>>() })
        }
        _ => {
            let ids = node_ids(params)?;
            let kind = match params.get("kind").and_then(Value::as_str) {
                Some("sharp") => Conversion::Sharp,
                Some("smooth") => Conversion::Smooth,
                Some("symmetric") => Conversion::Symmetric,
                Some("smart") => Conversion::Smart,
                _ => return Err(CommandError::invalid_params()),
            };
            convert(&mut contours[index], &ids, kind)?;
            json!({ "contour": index, "nodes": ids.iter().map(|i| i.0).collect::<Vec<_>>() })
        }
    };
    Ok(Plan { id, contours, extra, preserve_transform })
}

fn node_ids(params: &Value) -> Result<Vec<PointId>, CommandError> {
    let items = params.get("nodes").and_then(Value::as_array).filter(|items| !items.is_empty()).ok_or_else(CommandError::invalid_params)?;
    let mut ids: Vec<PointId> = Vec::with_capacity(items.len());
    for item in items {
        let id = PointId(item.as_u64().ok_or_else(CommandError::invalid_params)?);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn position(contour: &Contour, id: PointId) -> Result<usize, CommandError> {
    contour.points.iter().position(|p| p.id == id).ok_or_else(CommandError::not_found)
}

fn pt(p: [f64; 2]) -> Point {
    Point::new(p[0], p[1])
}

fn arr(p: Point) -> [f64; 2] {
    [p.x, p.y]
}

pub fn segment_count(contour: &Contour) -> usize {
    let n = contour.points.len();
    if contour.closed && n > 1 { n } else { n.saturating_sub(1) }
}

pub fn segment(contour: &Contour, index: usize) -> Option<CubicBez> {
    if index >= segment_count(contour) {
        return None;
    }
    let a = &contour.points[index];
    let b = &contour.points[(index + 1) % contour.points.len()];
    Some(CubicBez::new(pt(a.pos), pt(a.handle_out.unwrap_or(a.pos)), pt(b.handle_in.unwrap_or(b.pos)), pt(b.pos)))
}

pub fn nearest_segment(contour: &Contour, point: [f64; 2]) -> Option<(usize, f64, f64)> {
    (0..segment_count(contour))
        .filter_map(|i| segment(contour, i).map(|bez| (i, bez.nearest(pt(point), 1e-6))))
        .map(|(i, near)| (i, near.t, near.distance_sq.sqrt()))
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Find a point on one segment within the given distance, using the control hull to skip distant segments.
pub fn segment_point_within(contour: &Contour, index: usize, point: [f64; 2], radius: f64) -> Option<[f64; 2]> {
    let bez = segment(contour, index)?;
    let [x, y] = point;
    let control = [bez.p0, bez.p1, bez.p2, bez.p3];
    if control.iter().all(|p| p.x < x - radius) || control.iter().all(|p| p.x > x + radius)
        || control.iter().all(|p| p.y < y - radius) || control.iter().all(|p| p.y > y + radius)
    {
        return None;
    }
    let p = pt(point);
    let (distance_sq, target) = if bez.p0 == bez.p1 && bez.p2 == bez.p3 {
        let line = Line::new(bez.p0, bez.p3);
        let near = line.nearest(p, 1e-6);
        (near.distance_sq, line.eval(near.t))
    } else {
        let near = bez.nearest(p, 1e-6);
        (near.distance_sq, bez.eval(near.t))
    };
    (distance_sq <= radius * radius).then(|| arr(target))
}

fn insert(contour: &mut Contour, params: &Value) -> Result<PointId, CommandError> {
    let (seg, t) = match params.get("point") {
        Some(value) => {
            let items = value.as_array().filter(|v| v.len() == 2).ok_or_else(CommandError::invalid_params)?;
            let p = [items[0].as_f64().ok_or_else(CommandError::invalid_params)?, items[1].as_f64().ok_or_else(CommandError::invalid_params)?];
            let (seg, t, _) = nearest_segment(contour, p).ok_or_else(CommandError::invalid_params)?;
            let next = (seg + 1) % contour.points.len();
            let (a, b) = (&contour.points[seg], &contour.points[next]);
            if a.handle_out.is_none() && b.handle_in.is_none() {
                let d = sub(b.pos, a.pos);
                let l = d[0] * d[0] + d[1] * d[1];
                (seg, if l > 0.0 { (((p[0] - a.pos[0]) * d[0] + (p[1] - a.pos[1]) * d[1]) / l).clamp(0.0, 1.0) } else { t })
            } else {
                (seg, t)
            }
        }
        None => (
            params.get("segment").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)? as usize,
            params.get("t").and_then(Value::as_f64).ok_or_else(CommandError::invalid_params)?,
        ),
    };
    if !(t.is_finite() && t > SPLIT_EPSILON && t < 1.0 - SPLIT_EPSILON) {
        return Err(CommandError::invalid_params());
    }
    let bez = segment(contour, seg).ok_or_else(CommandError::invalid_params)?;
    let n = contour.points.len();
    let next = (seg + 1) % n;
    let id = PointId(contour.points.iter().map(|p| p.id.0).max().unwrap_or(0) + 1);
    let straight = contour.points[seg].handle_out.is_none() && contour.points[next].handle_in.is_none();
    let node = if straight {
        let (a, b) = (contour.points[seg].pos, contour.points[next].pos);
        PathPoint::sharp(id, [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
    } else {
        let left = bez.subsegment(0.0..t);
        let right = bez.subsegment(t..1.0);
        contour.points[seg].handle_out = Some(arr(left.p1));
        contour.points[next].handle_in = Some(arr(right.p2));
        PathPoint { id, pos: arr(left.p3), handle_in: Some(arr(left.p2)), handle_out: Some(arr(right.p1)), kind: NodeKind::Smooth, corner: None }
    };
    contour.points.insert(seg + 1, node);
    Ok(id)
}

fn delete(contours: &mut Vec<Contour>, index: usize, ids: &[PointId]) -> Result<(), CommandError> {
    let contour = &mut contours[index];
    for id in ids {
        position(contour, *id)?;
    }
    contour.points.retain(|p| !ids.contains(&p.id));
    let left = contour.points.len();
    if index == 0 {
        if left < 2 {
            return Err(CommandError::invalid_params());
        }
        if left < 3 {
            contour.closed = false;
        }
    } else if left < 3 {
        contours.remove(index);
    }
    Ok(())
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(v: [f64; 2]) -> f64 {
    v[0].hypot(v[1])
}

fn along(p: [f64; 2], unit: [f64; 2], d: f64) -> [f64; 2] {
    [p[0] + unit[0] * d, p[1] + unit[1] * d]
}

fn convert(contour: &mut Contour, ids: &[PointId], kind: Conversion) -> Result<(), CommandError> {
    let indices = ids.iter().map(|id| position(contour, *id)).collect::<Result<Vec<_>, _>>()?;
    let n = contour.points.len();
    let closed = contour.closed && n > 2;
    for i in indices {
        let prev = if i > 0 { Some(contour.points[i - 1].pos) } else if closed { Some(contour.points[n - 1].pos) } else { None };
        let next = if i + 1 < n { Some(contour.points[i + 1].pos) } else if closed { Some(contour.points[0].pos) } else { None };
        let node = &mut contour.points[i];
        let pos = node.pos;
        match kind {
            Conversion::Sharp => {
                node.handle_in = None;
                node.handle_out = None;
                node.kind = NodeKind::Sharp;
            }
            Conversion::Smart => {
                let span = match (prev, next) {
                    (Some(p), Some(q)) => sub(q, p),
                    (None, Some(q)) => sub(q, pos),
                    (Some(p), None) => sub(pos, p),
                    (None, None) => [0.0, 0.0],
                };
                let d = [span[0] * SMART_FRACTION, span[1] * SMART_FRACTION];
                node.handle_in = prev.map(|_| sub(pos, d));
                node.handle_out = next.map(|_| [pos[0] + d[0], pos[1] + d[1]]);
                node.kind = NodeKind::Smart;
            }
            Conversion::Smooth | Conversion::Symmetric => {
                let dir = match (node.handle_in, node.handle_out, prev, next) {
                    (Some(a), Some(b), _, _) if len(sub(b, a)) > 0.0 => sub(b, a),
                    (_, _, Some(p), Some(q)) => sub(q, p),
                    (_, _, None, Some(q)) => sub(q, pos),
                    (_, _, Some(p), None) => sub(pos, p),
                    _ => [0.0, 0.0],
                };
                let l = len(dir);
                if l > 0.0 {
                    let unit = [dir[0] / l, dir[1] / l];
                    let li = node.handle_in.map(|h| len(sub(pos, h))).or(prev.map(|p| len(sub(pos, p)) * HANDLE_FRACTION));
                    let lo = node.handle_out.map(|h| len(sub(h, pos))).or(next.map(|q| len(sub(q, pos)) * HANDLE_FRACTION));
                    let (li, lo) = match (kind, li, lo) {
                        (Conversion::Symmetric, Some(a), Some(b)) => (Some((a + b) * 0.5), Some((a + b) * 0.5)),
                        _ => (li, lo),
                    };
                    node.handle_in = li.map(|d: f64| along(pos, unit, -d));
                    node.handle_out = lo.map(|d: f64| along(pos, unit, d));
                }
                node.kind = NodeKind::Smooth;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod contracts {
    use super::*;
    use crate::commands::curve_edit::CURVE_ADD;
    use crate::document::{Document, Units};

    fn session() -> Session {
        let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
        session.has_document = true;
        session
    }

    fn add(s: &mut Session, params: Value) -> ObjectId {
        ObjectId(s.run(&Command::with_params(CURVE_ADD, params)).unwrap()["id"].as_u64().unwrap())
    }

    fn primary(s: &Session, id: ObjectId) -> Contour {
        world_contours(s.document.get_object(id).unwrap()).remove(0)
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        len(sub(a, b)) < 1e-3
    }

    #[test]
    fn insert_straight_and_curved_with_undo() {
        let mut s = session();
        let id = add(&mut s, json!({ "points": [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]] }));
        let reply = s.run(&Command::with_params(CURVE_NODES_INSERT, json!({ "id": id.0, "segment": 0, "t": 0.25 }))).unwrap();
        assert_eq!(reply["node"], json!(4));
        let c = primary(&s, id);
        assert_eq!(c.points.len(), 4);
        assert!(close(c.points[1].pos, [25.0, 0.0]) && c.points[1].kind == NodeKind::Sharp && c.points[1].handle_in.is_none());
        s.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(primary(&s, id).points.len(), 3);
        let arc = add(&mut s, json!({ "points": [{ "pos": [0.0, 0.0], "handle_out": [0.0, -50.0] }, { "pos": [100.0, 0.0], "handle_in": [100.0, -50.0] }] }));
        s.run(&Command::with_params(CURVE_NODES_INSERT, json!({ "id": id.0, "point": [80.0, 3.0] }))).unwrap();
        assert!(close(primary(&s, id).points[1].pos, [80.0, 0.0]));
        s.run(&Command::with_params(CURVE_NODES_INSERT, json!({ "id": arc.0, "point": [50.0, -37.0] }))).unwrap();
        let c = primary(&s, arc);
        assert_eq!(c.points.len(), 3);
        assert!(close(c.points[1].pos, [50.0, -37.5]), "{:?}", c.points[1].pos);
        assert_eq!(c.points[1].kind, NodeKind::Smooth);
        assert!(close(c.points[0].handle_out.unwrap(), [0.0, -25.0]));
        assert!(close(c.points[2].handle_in.unwrap(), [100.0, -25.0]));
    }

    #[test]
    fn insert_rejects_bad_params() {
        let mut s = session();
        let id = add(&mut s, json!({ "points": [[0.0, 0.0], [100.0, 0.0]] }));
        let steps = s.undo.applied_steps();
        for params in [
            json!({ "id": id.0, "segment": 0, "t": 0.0 }),
            json!({ "id": id.0, "segment": 1, "t": 0.5 }),
            json!({ "id": id.0, "segment": 0 }),
            json!({ "id": id.0, "contour": 1, "segment": 0, "t": 0.5 }),
            json!({ "id": id.0, "point": [0.0, 0.0] }),
            json!({ "id": 99, "segment": 0, "t": 0.5 }),
        ] {
            let command = Command::with_params(CURVE_NODES_INSERT, params);
            assert!(!s.can_execute(&command));
            assert!(s.run(&command).is_err());
        }
        assert_eq!(s.undo.applied_steps(), steps);
        assert_eq!(primary(&s, id).points.len(), 2);
    }

    #[test]
    fn delete_nodes_keeps_object() {
        let mut s = session();
        let id = add(&mut s, json!({ "points": [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], "closed": true }));
        s.run(&Command::with_params(CURVE_NODES_DELETE, json!({ "id": id.0, "nodes": [4] }))).unwrap();
        assert!(primary(&s, id).closed);
        s.run(&Command::with_params(CURVE_NODES_DELETE, json!({ "id": id.0, "nodes": [3] }))).unwrap();
        let c = primary(&s, id);
        assert!(!c.closed && c.points.len() == 2);
        let steps = s.undo.applied_steps();
        assert!(s.run(&Command::with_params(CURVE_NODES_DELETE, json!({ "id": id.0, "nodes": [1] }))).is_err());
        assert!(s.run(&Command::with_params(CURVE_NODES_DELETE, json!({ "id": id.0, "nodes": [9] }))).is_err());
        assert_eq!(s.undo.applied_steps(), steps);
        assert!(s.document.get_object(id).is_some());
        s.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(primary(&s, id).points.len(), 3);
        let outer = primary(&s, id);
        let hole = json!({ "points": [[20.0, 20.0], [40.0, 20.0], [40.0, 40.0]], "closed": true });
        s.run(&Command::with_params(crate::commands::curve_edit::CURVE_NODES_SET, json!({ "id": id.0, "contours": [outer, hole] }))).unwrap();
        s.run(&Command::with_params(CURVE_NODES_DELETE, json!({ "id": id.0, "contour": 1, "nodes": [2] }))).unwrap();
        assert_eq!(world_contours(s.document.get_object(id).unwrap()).len(), 1);
    }

    #[test]
    fn convert_kinds() {
        let mut s = session();
        let id = add(&mut s, json!({ "points": [[0.0, 0.0], [50.0, 0.0], [100.0, 50.0]] }));
        let run = |s: &mut Session, kind: &str| s.run(&Command::with_params(CURVE_NODES_CONVERT, json!({ "id": id.0, "nodes": [2], "kind": kind }))).unwrap();
        run(&mut s, "smooth");
        let n = primary(&s, id).points[1].clone();
        assert_eq!(n.kind, NodeKind::Smooth);
        let (a, b) = (sub(n.pos, n.handle_in.unwrap()), sub(n.handle_out.unwrap(), n.pos));
        assert!((a[0] * b[1] - a[1] * b[0]).abs() < 1e-6 && a[0] > 0.0);
        assert!((len(a) - 50.0 / 3.0).abs() < 1e-3 && (len(b) - len([50.0, 50.0]) / 3.0).abs() < 1e-3);
        run(&mut s, "symmetric");
        let n = primary(&s, id).points[1].clone();
        assert!((len(sub(n.pos, n.handle_in.unwrap())) - len(sub(n.handle_out.unwrap(), n.pos))).abs() < 1e-6);
        run(&mut s, "smart");
        let n = primary(&s, id).points[1].clone();
        assert_eq!(n.kind, NodeKind::Smart);
        assert!(close(n.handle_out.unwrap(), [50.0 + 100.0 / 6.0, 50.0 / 6.0]));
        run(&mut s, "sharp");
        let n = primary(&s, id).points[1].clone();
        assert!(n.kind == NodeKind::Sharp && n.handle_in.is_none() && n.handle_out.is_none());
        let steps = s.undo.applied_steps();
        assert_eq!(run(&mut s, "sharp")["changed"], json!(false));
        assert_eq!(s.undo.applied_steps(), steps);
        assert!(s.run(&Command::with_params(CURVE_NODES_CONVERT, json!({ "id": id.0, "nodes": [2], "kind": "bogus" }))).is_err());
        assert!(s.run(&Command::with_params(CURVE_NODES_CONVERT, json!({ "id": id.0, "nodes": [], "kind": "sharp" }))).is_err());
    }
}
