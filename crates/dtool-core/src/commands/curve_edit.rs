use crate::command::Session;
use crate::commands::arrange::{commit_snapshot, snapshot};
use crate::curve::{Contour, Curve, NodeKind, PathPoint, PointId};
use crate::object::{Object, ObjectId, ObjectKind};
use crate::paint::{Colour, Paint, StrokeStyle};
use crate::selection::SelectOp;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const CURVE_ADD: &str = "curve.add";
pub const CURVE_NODES_SET: &str = "curve.nodes.set";
pub const DEFAULT_STROKE_WIDTH: f32 = 1.0;
pub const MIN_CURVE_EXTENT: f32 = 1.0;

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        CURVE_ADD => Some(session.has_document && plan_add(&command.params).is_ok()),
        CURVE_NODES_SET => Some(plan_nodes(session, &command.params).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        CURVE_ADD => Some(add(session, &command.params)),
        CURVE_NODES_SET => Some(nodes_set(session, &command.params)),
        _ => None,
    }
}

pub fn default_stroke() -> StrokeStyle {
    StrokeStyle { paint: Paint::Solid { colour: Colour::new(0, 0, 0, 255) }, width: DEFAULT_STROKE_WIDTH, ..StrokeStyle::default() }
}

fn finite_pair(value: &Value) -> Option<[f64; 2]> {
    let items = value.as_array().filter(|items| items.len() == 2)?;
    let pair = [items[0].as_f64()?, items[1].as_f64()?];
    pair.iter().all(|v| v.is_finite()).then_some(pair)
}

pub fn parse_point(value: &Value, index: usize) -> Result<PathPoint, CommandError> {
    let id = PointId(index as u64 + 1);
    if let Some(pos) = finite_pair(value) {
        return Ok(PathPoint::sharp(id, pos));
    }
    let mut fields = value.as_object().cloned().ok_or_else(CommandError::invalid_params)?;
    let has_handle = ["handle_in", "handle_out"].iter().any(|key| fields.get(*key).is_some_and(|v| !v.is_null()));
    fields.entry("id").or_insert(json!(id.0));
    fields.entry("kind").or_insert(json!(if has_handle { NodeKind::Smooth } else { NodeKind::Sharp }));
    let point: PathPoint = serde_json::from_value(Value::Object(fields)).map_err(|_| CommandError::invalid_params())?;
    let coords = [Some(point.pos), point.handle_in, point.handle_out];
    if coords.iter().flatten().flatten().any(|v| !v.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    Ok(point)
}

pub fn parse_contour(points: &Value, closed: bool) -> Result<Contour, CommandError> {
    let items = points.as_array().ok_or_else(CommandError::invalid_params)?;
    if items.len() < if closed { 3 } else { 2 } {
        return Err(CommandError::invalid_params());
    }
    let points = items.iter().enumerate().map(|(i, v)| parse_point(v, i)).collect::<Result<Vec<_>, _>>()?;
    for (i, point) in points.iter().enumerate() {
        if points[..i].iter().any(|other| other.id == point.id) {
            return Err(CommandError::invalid_params());
        }
    }
    Ok(Contour { points, closed })
}

fn closed_flag(value: Option<&Value>) -> Result<bool, CommandError> {
    match value {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v.as_bool().ok_or_else(CommandError::invalid_params),
    }
}

pub(super) fn preserve_transform(params: &Value) -> Result<bool, CommandError> {
    closed_flag(params.get("preserve_transform"))
}

fn offset_point(point: &mut PathPoint, delta: [f64; 2]) {
    let shift = |p: [f64; 2]| [p[0] + delta[0], p[1] + delta[1]];
    point.pos = shift(point.pos);
    point.handle_in = point.handle_in.map(shift);
    point.handle_out = point.handle_out.map(shift);
}

pub fn offset_contour(contour: &Contour, delta: [f64; 2]) -> Contour {
    let mut out = contour.clone();
    for point in &mut out.points {
        offset_point(point, delta);
    }
    out
}

fn contour_frame(bounds: [f32; 4]) -> Result<([f32; 2], [f32; 2]), CommandError> {
    let [left, top, right, bottom] = bounds;
    let size = [(right - left).max(MIN_CURVE_EXTENT), (bottom - top).max(MIN_CURVE_EXTENT)];
    if bounds.iter().chain(&size).any(|v| !v.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    Ok(([left, top], size))
}

pub fn placement(contour: &Contour) -> Result<([f32; 2], [f32; 2], Contour), CommandError> {
    let (pos, size) = contour_frame(contour.bounds())?;
    Ok((pos, size, offset_contour(contour, [-f64::from(pos[0]), -f64::from(pos[1])])))
}

pub fn world_contour(object: &Object) -> Option<Contour> {
    let curve = object.curve()?;
    Some(offset_contour(&curve.contour, [f64::from(object.pos[0]), f64::from(object.pos[1])]))
}

fn plan_add(params: &Value) -> Result<(Contour, Paint, StrokeStyle), CommandError> {
    let closed = closed_flag(params.get("closed"))?;
    let contour = parse_contour(params.get("points").ok_or_else(CommandError::invalid_params)?, closed)?;
    let fill = match params.get("fill") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?,
        None => Paint::None,
    };
    let stroke = match params.get("stroke") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?,
        None => default_stroke(),
    };
    Ok((contour, fill, stroke))
}

fn add(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let (contour, fill, stroke) = plan_add(params)?;
    let (pos, size, local) = placement(&contour)?;
    let before = snapshot(session);
    let mut object = Object { pos, size, fill, stroke, kind: ObjectKind::Curve(Curve::from_contour_and_holes(local, Vec::new())), ..Object::default() };
    let start = contour.points[0].pos.map(|coordinate| coordinate as f32);
    crate::artboard::edit::assign_new_object(&session.document, &mut object, start);
    let id = session.document.create_object(object);
    session.selection.set(vec![id], SelectOp::Replace);
    commit_snapshot(session, CURVE_ADD, before, json!({ "id": id.0 }))
}

pub fn editable_curve(session: &Session, id: ObjectId) -> Result<&Object, CommandError> {
    editable_curve_impl(session, id, false)
}

pub(super) fn editable_curve_impl(session: &Session, id: ObjectId, preserve_transform: bool) -> Result<&Object, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if object.locked {
        return Err(CommandError::new("LOCKED"));
    }
    if object.curve().is_none() || (!preserve_transform && (object.rotation_deg != 0.0 || object.shear_deg != 0.0)) {
        return Err(CommandError::invalid_params());
    }
    Ok(object)
}

pub fn world_contours(object: &Object) -> Vec<Contour> {
    let delta = [f64::from(object.pos[0]), f64::from(object.pos[1])];
    object.curve().map(|curve| std::iter::once(&curve.contour).chain(curve.holes.iter()).map(|c| offset_contour(c, delta)).collect()).unwrap_or_default()
}

pub fn map_contours(contours: &mut [Contour], matrix: [f32; 6]) {
    let [a, b, c, d, e, f] = matrix.map(f64::from);
    let map = |[x, y]: [f64; 2]| [a * x + c * y + e, b * x + d * y + f];
    for point in contours.iter_mut().flat_map(|contour| &mut contour.points) {
        point.pos = map(point.pos);
        point.handle_in = point.handle_in.map(map);
        point.handle_out = point.handle_out.map(map);
    }
}

pub fn localise(contours: &[Contour]) -> Result<([f32; 2], [f32; 2], Vec<Contour>), CommandError> {
    if contours.is_empty() {
        return Err(CommandError::invalid_params());
    }
    let mut bounds: Option<[f32; 4]> = None;
    for contour in contours.iter().filter(|contour| !contour.is_empty()) {
        let [x0, y0, x1, y1] = contour.bounds();
        if [x0, y0, x1, y1].iter().any(|v| !v.is_finite()) {
            return Err(CommandError::invalid_params());
        }
        bounds = Some(match bounds {
            Some([left, top, right, bottom]) => [left.min(x0), top.min(y0), right.max(x1), bottom.max(y1)],
            None => [x0, y0, x1, y1],
        });
    }
    let (pos, size) = contour_frame(bounds.unwrap_or_default())?;
    let delta = pos.map(|coordinate| -f64::from(coordinate));
    Ok((pos, size, contours.iter().map(|contour| offset_contour(contour, delta)).collect()))
}

pub fn parse_contours(list: &Value) -> Result<Vec<Contour>, CommandError> {
    let items = list.as_array().filter(|items| !items.is_empty()).ok_or_else(CommandError::invalid_params)?;
    items.iter().map(parse_contour_value).collect()
}

pub fn commit_contours(session: &mut Session, id: ObjectId, contours: Vec<Contour>, name: &str, extra: Value) -> CommandResult {
    commit_contours_with_transform(session, id, contours, name, extra, false)
}

pub fn edited_curve(object: &Object, contours: &[Contour], preserve_transform: bool) -> Result<Option<Object>, CommandError> {
    let (mut pos, mut size, local) = localise(contours)?;
    if preserve_transform || object.size.iter().any(|extent| *extent < 0.0) {
        use crate::geometry::{apply_matrix, object_matrix, IDENTITY};
        let matrix = object_matrix(object);
        if matrix != IDENTITY {
            let centre = std::array::from_fn(|i| pos[i] + size[i] * 0.5);
            let centre = apply_matrix(&matrix, centre);
            pos = std::array::from_fn(|i| centre[i] - size[i] * 0.5);
            size = std::array::from_fn(|i| size[i].copysign(object.size[i]));
            if !pos.iter().all(|value| value.is_finite()) {
                return Err(CommandError::invalid_params());
            }
        }
    }
    let same = object.pos == pos && object.size == size && object.curve().is_some_and(|c| c.contour == local[0] && c.holes[..] == local[1..]);
    if same {
        return Ok(None);
    }
    let mut edited = object.clone();
    edited.pos = pos;
    edited.size = size;
    if let Some(curve) = edited.curve_mut() {
        let mut all = local.into_iter();
        curve.contour = all.next().unwrap_or_default();
        curve.holes = all.collect();
    }
    Ok(Some(edited))
}

pub(super) fn commit_contours_with_transform(
    session: &mut Session, id: ObjectId, contours: Vec<Contour>, name: &str, extra: Value, preserve_transform: bool,
) -> CommandResult {
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    let Some(edited) = edited_curve(object, &contours, preserve_transform)? else {
        return Ok(json!({ "id": id.0, "changed": false, "doc_version": session.generation }));
    };
    let mut reply = json!({ "id": id.0, "changed": true });
    if let (Some(target), Some(source)) = (reply.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    if super::symbols::object_link(&session.document, id).is_some() {
        let original = object.clone();
        if let Some(result) = super::symbols::curves::apply_edit(session, &original, &edited, name, reply.clone()) {
            return result;
        }
    }
    let before = snapshot(session);
    *session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)? = edited;
    commit_snapshot(session, name, before, reply)
}

fn parse_contour_value(value: &Value) -> Result<Contour, CommandError> {
    let closed = closed_flag(value.get("closed"))?;
    parse_contour(value.get("points").ok_or_else(CommandError::invalid_params)?, closed)
}

pub fn contour_index(params: &Value, key: &str, len: usize) -> Result<usize, CommandError> {
    let index = match params.get(key) {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().ok_or_else(CommandError::invalid_params)? as usize,
    };
    if index >= len {
        return Err(CommandError::invalid_params());
    }
    Ok(index)
}

fn plan_nodes(session: &Session, params: &Value) -> Result<(ObjectId, Vec<Contour>, bool), CommandError> {
    let id = ObjectId(params.get("id").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?);
    let preserve_transform = preserve_transform(params)?;
    let object = editable_curve_impl(session, id, preserve_transform)?;
    let mut contours = world_contours(object);
    match (params.get("contours"), params.get("contour")) {
        (Some(list), None) => {
            contours = parse_contours(list)?;
        }
        (None, Some(contour)) => {
            let index = contour_index(params, "index", contours.len())?;
            contours[index] = parse_contour_value(contour)?;
        }
        _ => return Err(CommandError::invalid_params()),
    }
    Ok((id, contours, preserve_transform))
}

fn nodes_set(session: &mut Session, params: &Value) -> CommandResult {
    let (id, contours, preserve_transform) = plan_nodes(session, params)?;
    commit_contours_with_transform(session, id, contours, CURVE_NODES_SET, json!({}), preserve_transform)
}

#[cfg(test)]
mod contracts {
    use super::*;
    use crate::document::{Document, Units};

    fn session() -> Session {
        let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
        session.has_document = true;
        session
    }

    fn state(session: &Session) -> (usize, Vec<ObjectId>, bool) {
        (session.document.objects.len(), session.selection.ids.clone(), session.undo.can_undo())
    }

    #[test]
    fn add_line_and_smooth_curve_with_undo() {
        let mut s = session();
        let reply = s.run(&Command::with_params(CURVE_ADD, json!({ "points": [[10.0, 20.0], [110.0, 20.0]] }))).unwrap();
        let id = ObjectId(reply["id"].as_u64().unwrap());
        let object = s.document.get_object(id).unwrap();
        assert_eq!((object.pos, object.size), ([10.0, 20.0], [100.0, MIN_CURVE_EXTENT]));
        assert_eq!(object.stroke.width, DEFAULT_STROKE_WIDTH);
        assert_eq!(object.fill, Paint::None);
        let curve = object.curve().unwrap();
        assert_eq!(curve.contour.points[1].pos, [100.0, 0.0]);
        assert!(!curve.contour.closed);
        assert_eq!(s.selection.ids, vec![id]);
        assert_eq!(world_contour(object).unwrap().points[0].pos, [10.0, 20.0]);
        let smooth = json!({ "points": [[0.0, 0.0], { "pos": [50.0, 50.0], "handle_in": [40.0, 50.0], "handle_out": [60.0, 50.0] }, [100.0, 0.0]], "closed": true });
        let reply = s.run(&Command::with_params(CURVE_ADD, smooth)).unwrap();
        let curve = s.document.get_object(ObjectId(reply["id"].as_u64().unwrap())).unwrap().curve().unwrap().clone();
        assert!(curve.contour.closed);
        assert_eq!(curve.contour.points[1].kind, NodeKind::Smooth);
        assert_eq!(curve.contour.points[1].id, PointId(2));
        s.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(s.document.objects.len(), 1);
        s.run(&Command::new("edit.undo")).unwrap();
        assert!(s.document.objects.is_empty());
    }

    #[test]
    fn add_rejects_invalid_without_side_effects() {
        let mut s = session();
        let before = state(&s);
        for params in [
            json!({}),
            json!({ "points": [[0.0, 0.0]] }),
            json!({ "points": [[0.0, 0.0], [1.0, 1.0]], "closed": true }),
            json!({ "points": [[0.0, 0.0], [1.0]] }),
            json!({ "points": [[0.0, 0.0], { "pos": [1.0, 1.0], "kind": "bogus" }] }),
            json!({ "points": [{ "id": 3, "pos": [0.0, 0.0] }, { "id": 3, "pos": [1.0, 1.0] }] }),
            json!({ "points": [[0.0, 0.0], [5.0, 5.0]], "closed": "yes" }),
        ] {
            let command = Command::with_params(CURVE_ADD, params);
            assert!(!s.can_execute(&command));
            assert!(s.run(&command).is_err());
            assert_eq!(state(&s), before);
        }
    }

    #[test]
    fn nodes_set_moves_nodes_with_one_undo() {
        let mut s = session();
        let reply = s.run(&Command::with_params(CURVE_ADD, json!({ "points": [[10.0, 10.0], [50.0, 10.0], [50.0, 60.0]], "closed": true }))).unwrap();
        let id = ObjectId(reply["id"].as_u64().unwrap());
        let mut contour = world_contour(s.document.get_object(id).unwrap()).unwrap();
        contour.points[0].pos = [0.0, 0.0];
        contour.points[1].handle_out = Some([60.0, 0.0]);
        contour.points[1].kind = NodeKind::Smooth;
        let undo_len = s.undo.applied_steps();
        let set = Command::with_params(CURVE_NODES_SET, json!({ "id": id.0, "contour": contour }));
        assert!(s.can_execute(&set));
        assert_eq!(s.run(&set).unwrap()["changed"], json!(true));
        assert_eq!(s.undo.applied_steps(), undo_len + 1);
        let object = s.document.get_object(id).unwrap();
        assert_eq!(object.pos, [0.0, 0.0]);
        assert_eq!(world_contour(object).unwrap(), contour);
        assert_eq!(s.run(&set).unwrap()["changed"], json!(false));
        assert_eq!(s.undo.applied_steps(), undo_len + 1);
        s.run(&Command::new("edit.undo")).unwrap();
        let object = s.document.get_object(id).unwrap();
        assert_eq!(object.pos, [10.0, 10.0]);
        assert_eq!(world_contour(object).unwrap().points[0].pos, [10.0, 10.0]);
    }

    #[test]
    fn nodes_set_index_and_all_contours() {
        let mut s = session();
        let reply = s.run(&Command::with_params(CURVE_ADD, json!({ "points": [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], "closed": true }))).unwrap();
        let id = ObjectId(reply["id"].as_u64().unwrap());
        let outer = world_contour(s.document.get_object(id).unwrap()).unwrap();
        let hole = json!({ "points": [[20.0, 20.0], [40.0, 20.0], [40.0, 40.0]], "closed": true });
        let steps = s.undo.applied_steps();
        s.run(&Command::with_params(CURVE_NODES_SET, json!({ "id": id.0, "contours": [outer, hole] }))).unwrap();
        assert_eq!(s.undo.applied_steps(), steps + 1);
        let contours = world_contours(s.document.get_object(id).unwrap());
        assert_eq!(contours.len(), 2);
        assert_eq!(contours[1].points[1].pos, [40.0, 20.0]);
        let moved = json!({ "points": [[20.0, 20.0], [50.0, 20.0], [40.0, 40.0]], "closed": true });
        s.run(&Command::with_params(CURVE_NODES_SET, json!({ "id": id.0, "contour": moved, "index": 1 }))).unwrap();
        assert_eq!(world_contours(s.document.get_object(id).unwrap())[1].points[1].pos, [50.0, 20.0]);
        assert!(s.run(&Command::with_params(CURVE_NODES_SET, json!({ "id": id.0, "contour": moved, "index": 5 }))).is_err());
        assert!(s.run(&Command::with_params(CURVE_NODES_SET, json!({ "id": id.0, "contour": moved, "contours": [moved] }))).is_err());
        s.run(&Command::new("edit.undo")).unwrap();
        s.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(world_contours(s.document.get_object(id).unwrap()).len(), 1);
    }

    #[test]
    fn nodes_set_rejects_bad_targets() {
        let mut s = session();
        let rect = s.document.create_object(Object { size: [10.0, 10.0], ..Object::default() });
        let reply = s.run(&Command::with_params(CURVE_ADD, json!({ "points": [[0.0, 0.0], [10.0, 0.0]] }))).unwrap();
        let id = reply["id"].as_u64().unwrap();
        let contour = json!({ "points": [[0.0, 0.0], [20.0, 0.0]], "closed": false });
        if let Some(object) = s.document.objects.iter_mut().find(|o| o.id.0 == id) {
            object.locked = true;
        }
        let before = (serde_json::to_value(&s.document.objects).unwrap(), s.undo.applied_steps());
        for params in [
            json!({ "id": rect.0, "contour": contour }),
            json!({ "id": 999, "contour": contour }),
            json!({ "id": id, "contour": contour }),
            json!({ "id": id }),
        ] {
            assert!(s.run(&Command::with_params(CURVE_NODES_SET, params)).is_err());
            assert_eq!((serde_json::to_value(&s.document.objects).unwrap(), s.undo.applied_steps()), before);
        }
    }
}
