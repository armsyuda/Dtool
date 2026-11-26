use crate::commands::curve_edit::{offset_contour, preserve_transform, MIN_CURVE_EXTENT};
use crate::boolean::FillRule;
use crate::curve::{Contour, Curve, PathPoint, PointId};
use crate::document::Document;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::path_ops::{apply_plan, plan_join, plan_split};
use crate::{Command, CommandError, CommandResult, Session};
use kurbo::{Affine, Point};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const PATH_BREAK: &str = "path.break";
pub const PATH_JOIN: &str = "path.join";
pub const PATH_MERGE: &str = "path.curves.merge";
pub const PATH_SEPARATE: &str = "path.curves.separate";
pub const PATH_FILL_HOLES: &str = "path.curves.fill_holes";
pub const PATH_REMOVE_INNER: &str = "path.curves.remove_inner";
pub const PATH_FILL_RULE: &str = "path.fill_rule.set";
pub const PATH_POINT_ADD: &str = "path.point.add";
pub const PATH_CLOSE: &str = "path.close";
pub const CLOSE_WELD_EPSILON: f64 = 1e-9;
pub const DEFAULT_JOIN_TOLERANCE: f64 = 0.5;

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        PATH_BREAK => Some(session.has_document && target_ids(session, &command.params).first().is_some_and(|id| is_curve(&session.document, *id))),
        PATH_JOIN => Some(session.has_document && can_join(session, &command.params)),
        PATH_MERGE | PATH_SEPARATE | PATH_FILL_HOLES | PATH_REMOVE_INNER => {
            Some(compound_targets(session, command).is_ok())
        }
        PATH_FILL_RULE => Some(fill_rule_objects(session, &command.params).is_some()),
        PATH_POINT_ADD => Some(session.has_document && target_ids(session, &command.params).first().is_some_and(|id| is_curve(&session.document, *id))),
        PATH_CLOSE => Some(session.has_document && target_ids(session, &command.params).first().is_some_and(|id| is_curve(&session.document, *id))),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        PATH_BREAK => Some(path_break(session, command)),
        PATH_JOIN => Some(path_join(session, command)),
        PATH_MERGE => Some(merge_curves(session, command)),
        PATH_SEPARATE => Some(separate_curves(session, command)),
        PATH_FILL_HOLES | PATH_REMOVE_INNER => Some(clean_curve_boundaries(session, command)),
        PATH_FILL_RULE => Some(set_fill_rule(session, command)),
        PATH_POINT_ADD => Some(path_point_add(session, command)),
        PATH_CLOSE => Some(path_close(session, command)),
        _ => None,
    }
}

fn fill_rule_objects<'a>(session: &'a Session, params: &Value) -> Option<Vec<&'a Object>> {
    if !session.has_document { return None; }
    let ids = super::arrange::target_ids(session, &Command::with_params(PATH_FILL_RULE, params.clone())).ok()?;
    ids.into_iter().map(|id| {
        let object = session.document.get_object(id)?;
        (matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_)) && !object.locked
            && super::symbols::object_link(&session.document, id).is_none()).then_some(object)
    }).collect()
}

pub fn fill_rule_checked(session: &Session, rule: FillRule) -> bool {
    fill_rule_objects(session, &Value::Null)
        .is_some_and(|objects| objects.iter().all(|object| object.fill_rule == rule))
}

fn set_fill_rule(session: &mut Session, command: &Command) -> CommandResult {
    let rule = command.params.get("rule").and_then(|value| serde_json::from_value::<FillRule>(value.clone()).ok())
        .ok_or_else(CommandError::invalid_params)?;
    let ids: Vec<_> = fill_rule_objects(session, &command.params).ok_or_else(CommandError::invalid_params)?
        .into_iter().filter(|object| object.fill_rule != rule).map(|object| object.id).collect();
    let reply = json!({"ids": ids, "rule": rule});
    if ids.is_empty() { return Ok(reply); }
    let mut document = session.document.clone();
    for id in ids {
        document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?.fill_rule = rule;
    }
    let selection = session.selection.ids.clone();
    super::document_change::commit(session, PATH_FILL_RULE, document, selection, reply)
}

fn is_curve(doc: &Document, id: ObjectId) -> bool {
    doc.get_object(id).is_some_and(|object| matches!(object.kind, ObjectKind::Curve(_)))
}

fn parse_id(value: &Value) -> Option<u64> {
    if let Some(id) = value.as_u64() {
        return Some(id);
    }
    let text = value.as_str()?;
    let text = text.strip_prefix("node:").unwrap_or(text);
    text.trim().parse::<u64>().ok()
}

fn target_ids(session: &Session, params: &Value) -> Vec<ObjectId> {
    if let Some(list) = params.get("ids").and_then(Value::as_array) {
        return list.iter().filter_map(parse_id).map(ObjectId).collect();
    }
    if let Some(id) = params.get("id").and_then(parse_id) {
        return vec![ObjectId(id)];
    }
    match params.get("target") {
        Some(Value::Array(list)) => list.iter().filter_map(parse_id).map(ObjectId).collect(),
        Some(value) => parse_id(value).map(ObjectId).into_iter().collect(),
        None => session.selection.ids.clone(),
    }
}

fn can_join(session: &Session, params: &Value) -> bool {
    let ids = target_ids(session, params);
    let document = &session.document;
    let Some(first) = ids.first().and_then(|id| document.get_object(*id)) else { return false };
    ids.len() >= 2 && ids.iter().collect::<HashSet<_>>().len() == ids.len()
        && ids.iter().all(|id| document.get_object(*id).is_some_and(|object| {
            join_context_matches(document, object, first)
                && !object.locked
                && object.curve().is_some_and(|curve| !curve.contour.closed && !curve.contour.is_empty())
        }))
        && parse_tolerance(params).is_ok()
}

fn join_context_matches(doc: &Document, object: &Object, first: &Object) -> bool {
    object.parent_id == first.parent_id && object.page_id == first.page_id
        && object.baked_warps == first.baked_warps
        && crate::geometry::invert_matrix(&crate::geometry::object_matrix(object)).is_some()
        && object.clip_role.is_none() && object.vector_crop.is_none()
        && !doc.objects.iter().any(|child| child.parent_id == Some(object.id))
        && !doc.layer_masks.iter().any(|mask| mask.object == object.id)
}

fn compound_targets<'a>(session: &'a Session, command: &Command) -> Result<Vec<&'a Object>, CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let ids = target_ids(session, &command.params);
    if ids.is_empty() { return Err(CommandError::new("NOTHING_SELECTED")); }
    let set: HashSet<_> = ids.iter().copied().collect();
    if set.len() != ids.len() { return Err(CommandError::invalid_params()); }
    let doc = &session.document;
    let objects: Vec<_> = doc.objects.iter().filter(|object| set.contains(&object.id)).collect();
    if objects.len() != ids.len() { return Err(CommandError::not_found()); }
    let merge = command.name == PATH_MERGE;
    let cleanup = matches!(command.name.as_str(), PATH_FILL_HOLES | PATH_REMOVE_INNER);
    if merge && objects.len() < 2 { return Err(CommandError::new("NEED_TWO_CURVES")); }
    let first = objects[0];
    let mut has_multiple_paths = false;
    let mut has_closed = false;
    for object in &objects {
        let curve = object.curve().filter(|curve| !curve.is_empty())
            .ok_or_else(|| CommandError::new("NOT_A_CURVE"))?;
        has_multiple_paths |= curve.contours().filter(|path| !path.is_empty()).take(2).count() > 1;
        has_closed |= curve.contours().any(|path| path.closed && path.points.len() >= 2);
        if object.locked { return Err(CommandError::new("LOCKED")); }
        if !object.clip_role.is_none() || carries_path_text(doc, object.id)
            || doc.objects.iter().any(|child| child.parent_id == Some(object.id))
            || doc.layer_masks.iter().any(|mask| mask.object == object.id)
            || crate::geometry::invert_matrix(&crate::geometry::object_matrix(object)).is_none()
            || (cleanup && crate::commands::symbols::object_link(doc, object.id).is_some())
            || (merge && (object.parent_id != first.parent_id || object.page_id != first.page_id
                || object.baked_warps != first.baked_warps || object.vector_crop.is_some()))
        {
            return Err(CommandError::invalid_params());
        }
    }
    if (command.name == PATH_SEPARATE && !has_multiple_paths)
        || (cleanup && !has_closed)
    {
        return Err(CommandError::invalid_params());
    }
    Ok(objects)
}

fn curve_frame(object: &Object) -> Affine {
    let [x, y] = object.pos.map(f64::from);
    Affine::new(crate::geometry::object_matrix(object).map(f64::from)) * Affine::translate((x, y))
}

fn contour_in_frame(contour: &Contour, matrix: Affine) -> Contour {
    let [a, b, c, d, _, _] = matrix.as_coeffs();
    let preserves_radius = ((a * a + b * b) - 1.0).abs() < 1e-6
        && ((c * c + d * d) - 1.0).abs() < 1e-6 && (a * c + b * d).abs() < 1e-6;
    let mut projected = if preserves_radius { contour.clone() } else { crate::outline::cornered_contour(contour) };
    let transform = |[x, y]: [f64; 2]| {
        let p = matrix * Point::new(x, y);
        [p.x, p.y]
    };
    for point in &mut projected.points {
        point.pos = transform(point.pos);
        point.handle_in = point.handle_in.map(transform);
        point.handle_out = point.handle_out.map(transform);
    }
    projected
}

pub(crate) fn reframe(object: &mut Object, mut curve: Curve) -> Result<(), CommandError> {
    let [x0, y0, x1, y1] = curve.bounds();
    let extent = [(x1 - x0).max(MIN_CURVE_EXTENT), (y1 - y0).max(MIN_CURVE_EXTENT)];
    let new_origin = curve_frame(object) * Point::new(f64::from(x0 + extent[0] / 2.0), f64::from(y0 + extent[1] / 2.0));
    if !new_origin.is_finite() || extent.iter().any(|side| !side.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    let offset = [-f64::from(x0), -f64::from(y0)];
    curve.contour = offset_contour(&curve.contour, offset);
    for contour in &mut curve.holes { *contour = offset_contour(contour, offset); }
    let matrix = Affine::translate((offset[0], offset[1])).as_coeffs();
    for paint in std::iter::once(&mut object.fill).chain(&mut object.additional_fills) {
        crate::paint::transform_paint_matrix(paint, matrix);
    }
    for stroke in std::iter::once(&mut object.stroke).chain(&mut object.additional_strokes) {
        crate::paint::transform_stroke_style_matrix(stroke, matrix);
    }
    crate::paint::transform_transparency_matrix(&mut object.transparency, matrix);
    if let Some(crop) = &mut object.vector_crop {
        crop.rect[0] -= x0;
        crop.rect[1] -= y0;
    }
    object.pos = [new_origin.x as f32 - extent[0] / 2.0, new_origin.y as f32 - extent[1] / 2.0];
    object.size = [extent[0].copysign(object.size[0]), extent[1].copysign(object.size[1])];
    object.kind = ObjectKind::Curve(curve);
    Ok(())
}

fn merge_curves(session: &mut Session, command: &Command) -> CommandResult {
    let objects = compound_targets(session, command)?;
    let first = objects[0];
    let mut result = first.clone();
    let mut curve = first.curve().unwrap().clone();
    let mut next_point = max_point_id(&curve);
    let inverse = curve_frame(first).inverse();
    for object in objects.iter().skip(1) {
        let matrix = inverse * curve_frame(object);
        for contour in object.curve().unwrap().contours().filter(|path| !path.is_empty()) {
            let mut contour = contour_in_frame(contour, matrix);
            for point in &mut contour.points {
                next_point = next_point.checked_add(1).ok_or_else(CommandError::invalid_params)?;
                point.id = PointId(next_point);
            }
            curve.holes.push(contour);
        }
    }
    reframe(&mut result, curve)?;
    let id = result.id;
    let removed: HashSet<_> = objects.iter().skip(1).map(|object| object.id).collect();
    let removed_ids: Vec<_> = objects.iter().skip(1).map(|object| object.id).collect();
    let mut document = session.document.clone();
    *document.find_object_mut(id.0).ok_or_else(CommandError::not_found)? = result;
    document.detach_objects(&removed);
    document.layer_meta.retain(|meta| !removed.contains(&meta.object));
    super::document_change::commit(session, PATH_MERGE, document, vec![id], json!({"id": id, "removed": removed_ids}))
}

fn separate_curves(session: &mut Session, command: &Command) -> CommandResult {
    let objects = compound_targets(session, command)?;
    let mut document = session.document.clone();
    let mut selection = Vec::new();
    for object in objects {
        let curve = object.curve().unwrap();
        let mut contours = curve.contours().filter(|path| !path.is_empty()).peekable();
        let Some(first) = contours.next() else { continue };
        selection.push(object.id);
        if contours.peek().is_none() { continue; }
        let index = document.objects.iter().position(|item| item.id == object.id).ok_or_else(CommandError::not_found)?;
        let mut pieces = Vec::new();
        for (piece_index, contour) in std::iter::once(first).chain(contours).enumerate() {
            let mut piece = object.clone();
            let single = Curve { contour: contour.clone(), holes: Vec::new(), source_object: curve.source_object };
            reframe(&mut piece, single)?;
            if piece_index != 0 {
                piece.id = ObjectId(document.next_object_id);
                document.next_object_id = document.next_object_id.checked_add(1).ok_or_else(CommandError::invalid_params)?;
                if let Some(meta) = super::layer_meta::meta(&session.document, object.id) {
                    document.layer_meta.push(super::layer_meta::LayerMeta { object: piece.id, ..meta.clone() });
                }
                selection.push(piece.id);
            }
            pieces.push(piece);
        }
        document.objects.splice(index..=index, pieces);
    }
    let reply = json!({"ids": selection});
    super::document_change::commit(session, PATH_SEPARATE, document, selection, reply)
}

fn clean_curve_boundaries(session: &mut Session, command: &Command) -> CommandResult {
    let targets = compound_targets(session, command)?;
    let mut edits = Vec::new();
    for object in targets {
        let original = object.curve().unwrap();
        let Some(curve) = crate::boolean::clean_curve_boundaries(original, object.fill_rule, command.name == PATH_FILL_HOLES)? else {
            continue;
        };
        if &curve != original {
            edits.push((object.id, curve));
        }
    }
    let ids: Vec<_> = edits.iter().map(|(id, _)| *id).collect();
    let reply = json!({"ids": ids});
    if edits.is_empty() {
        return Ok(reply);
    }
    let mut document = session.document.clone();
    for (id, curve) in edits {
        let object = document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        if object.curve().unwrap().bounds() == curve.bounds() {
            object.kind = ObjectKind::Curve(curve);
        } else {
            reframe(object, curve)?;
        }
        object.fill_rule = FillRule::EvenOdd;
    }
    let selection = session.selection.ids.clone();
    super::document_change::commit(session, &command.name, document, selection, reply)
}

fn parse_tolerance(params: &Value) -> Result<f64, CommandError> {
    let tolerance = match params.get("tolerance") {
        None | Some(Value::Null) => DEFAULT_JOIN_TOLERANCE,
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            let number = trimmed.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            number.trim().parse::<f64>().map_err(|_| CommandError::invalid_params())?
        }
        Some(value) => value.as_f64().ok_or_else(CommandError::invalid_params)?,
    };
    if tolerance.is_finite() && tolerance >= 0.0 { Ok(tolerance) } else { Err(CommandError::invalid_params()) }
}

fn curve_source(doc: &Document, id: ObjectId) -> Result<(&Object, &Curve), CommandError> {
    let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    if object.locked {
        return Err(CommandError::new("LOCKED"));
    }
    let curve = object.curve().ok_or_else(|| CommandError::new("NOT_A_CURVE"))?;
    Ok((object, curve))
}

fn curve_of(doc: &Document, id: ObjectId) -> Result<(Object, Curve), CommandError> {
    let (object, curve) = curve_source(doc, id)?;
    if object.rotation_deg != 0.0 || object.shear_deg != 0.0 {
        return Err(CommandError::invalid_params());
    }
    Ok((object.clone(), world_curve(object, curve)))
}

fn topology_curve(doc: &Document, id: ObjectId, preserve: bool) -> Result<(Object, Curve), CommandError> {
    if !preserve {
        return curve_of(doc, id);
    }
    let (object, curve) = curve_source(doc, id)?;
    crate::geometry::invert_matrix(&crate::geometry::object_matrix(object)).ok_or_else(CommandError::invalid_params)?;
    Ok((object.clone(), curve.clone()))
}

fn place_topology(mut object: Object, curve: Curve, preserve: bool) -> Result<Object, CommandError> {
    if preserve {
        reframe(&mut object, curve)?;
    } else {
        object.kind = ObjectKind::Curve(curve);
        fit_bounds(&mut object);
    }
    Ok(object)
}

pub fn world_curve(object: &Object, curve: &Curve) -> Curve {
    let delta = [f64::from(object.pos[0]), f64::from(object.pos[1])];
    Curve {
        contour: offset_contour(&curve.contour, delta),
        holes: curve.holes.iter().map(|hole| offset_contour(hole, delta)).collect(),
        source_object: curve.source_object,
    }
}

fn max_point_id(curve: &Curve) -> u64 {
    curve.contours().flat_map(|contour| contour.points.iter()).map(|point| point.id.0).max().unwrap_or(0)
}

fn contour_at(curve: &Curve, subpath: usize) -> Option<&Contour> {
    if subpath == 0 { Some(&curve.contour) } else { curve.holes.get(subpath - 1) }
}

fn set_contour(curve: &mut Curve, subpath: usize, contour: Contour) {
    if subpath == 0 {
        curve.contour = contour;
    } else if let Some(slot) = curve.holes.get_mut(subpath - 1) {
        *slot = contour;
    }
}

fn locate(curve: &Curve, point: PointId) -> Option<(usize, usize)> {
    for (subpath, contour) in curve.contours().enumerate() {
        if let Some(index) = contour.points.iter().position(|p| p.id == point) {
            return Some((subpath, index));
        }
    }
    None
}

fn fit_bounds(object: &mut Object) {
    let ObjectKind::Curve(world) = &object.kind else {
        return;
    };
    if world.is_empty() {
        return;
    }
    let b = crate::geometry::bez_path_bounds(&world.to_kurbo());
    if b.iter().any(|v| !v.is_finite()) {
        return;
    }
    let pos = [b[0], b[1]];
    let size = [(b[2] - b[0]).max(MIN_CURVE_EXTENT), (b[3] - b[1]).max(MIN_CURVE_EXTENT)];
    let delta = [-f64::from(pos[0]), -f64::from(pos[1])];
    let local = Curve {
        contour: offset_contour(&world.contour, delta),
        holes: world.holes.iter().map(|hole| offset_contour(hole, delta)).collect(),
        source_object: world.source_object,
    };
    object.pos = pos;
    object.size = size;
    object.kind = ObjectKind::Curve(local);
}

fn carries_path_text(doc: &Document, id: ObjectId) -> bool {
    doc.objects.iter().any(|object| matches!(&object.kind, ObjectKind::PathText(data) if object.id == id || data.path == id))
}

fn commit(session: &mut Session, name: &str, before: Document, reply: Value) -> CommandResult {
    let after = session.document.clone();
    session.undo.push(crate::undo::Change {
        name: name.to_string(),
        undo: Command::with_params("core.feature.restore", json!({ "document": before })),
        redo: Command::with_params("core.feature.restore", json!({ "document": after })),
    });
    session.generation += 1;
    session.document.version += 1;
    Ok(reply)
}

fn path_break(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let params = &command.params;
    let id = *target_ids(session, params).first().ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let point = params.get("point").and_then(|p| p.get("id").or(Some(p))).and_then(parse_id).ok_or_else(CommandError::invalid_params)?;
    let split_objects = params.get("split_objects").and_then(Value::as_bool).unwrap_or(true);
    let preserve = preserve_transform(params)?;
    let (object, mut curve) = topology_curve(&session.document, id, preserve)?;
    let (subpath, index) = locate(&curve, PointId(point)).ok_or_else(CommandError::not_found)?;
    let contour = contour_at(&curve, subpath).cloned().ok_or_else(CommandError::not_found)?;
    let count = contour.points.len();
    let duplicate_id = PointId(max_point_id(&curve) + 1);
    let mut duplicate = contour.points[index].clone();
    duplicate.id = duplicate_id;

    let split_at = if count > 1 { index as f32 / (count - 1) as f32 } else { 0.0 };
    let plan = if carries_path_text(&session.document, id) { Some(plan_split(&session.document, id, split_at)?) } else { None };
    let mut new_piece = None;
    let first_side: Vec<u64>;
    let mut second_side: Vec<u64> = Vec::new();
    if contour.closed {
        let mut points: Vec<PathPoint> = contour.points[index..].to_vec();
        points.extend_from_slice(&contour.points[..index]);
        let mut first = points[0].clone();
        first.handle_in = None;
        points[0] = first;
        duplicate.handle_out = None;
        points.push(duplicate);
        first_side = points.iter().map(|p| p.id.0).collect();
        set_contour(&mut curve, subpath, Contour { points, closed: false });
    } else {
        if index == 0 || index + 1 >= count {
            return Err(CommandError::new("ENDPOINT"));
        }
        let mut head: Vec<PathPoint> = contour.points[..=index].to_vec();
        if let Some(last) = head.last_mut() {
            last.handle_out = None;
        }
        duplicate.handle_in = None;
        let mut tail: Vec<PathPoint> = vec![duplicate];
        tail.extend_from_slice(&contour.points[index + 1..]);
        first_side = head.iter().map(|p| p.id.0).collect();
        second_side = tail.iter().map(|p| p.id.0).collect();
        let tail_contour = Contour { points: tail, closed: false };
        set_contour(&mut curve, subpath, Contour { points: head, closed: false });
        if split_objects {
            let mut piece = object.clone();
            piece.id = ObjectId(0);
            piece.name = format!("{} 2", object.name);
            new_piece = Some(place_topology(piece, Curve::from_contour_and_holes(tail_contour, Vec::new()), preserve)?);
        } else {
            curve.holes.push(tail_contour);
        }
    }
    let result = place_topology(object, curve, preserve)?;
    let before = session.document.clone();
    *session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)? = result;
    let new_object = new_piece.map(|piece| session.document.create_object(piece).0);
    if let Some(plan) = &plan {
        apply_plan(&mut session.document, plan)?;
    }
    let reply = json!({
        "id": id.0,
        "point": point,
        "duplicate": duplicate_id.0,
        "new_object": new_object,
        "sides": [first_side, second_side],
        "closed_before": contour.closed,
        "path_text_kept": plan.is_some(),
    });
    commit(session, &command.name, before, reply)
}

fn reversed(contour: &Contour) -> Contour {
    let points = contour
        .points
        .iter()
        .rev()
        .map(|p| PathPoint { id: p.id, pos: p.pos, handle_in: p.handle_out, handle_out: p.handle_in, kind: p.kind, corner: p.corner })
        .collect();
    Contour { points, closed: contour.closed }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn oriented_pair(a: &Contour, b: &Contour, frame: Affine) -> (Contour, Contour, f64) {
    let world = |point: &PathPoint| frame * Point::new(point.pos[0], point.pos[1]);
    let a_first = world(&a.points[0]);
    let a_last = world(&a.points[a.points.len() - 1]);
    let b_first = world(&b.points[0]);
    let b_last = world(&b.points[b.points.len() - 1]);
    let options = [
        (a_last.distance(b_first), false, false),
        (a_last.distance(b_last), false, true),
        (a_first.distance(b_first), true, false),
        (a_first.distance(b_last), true, true),
    ];
    let mut best = options[0];
    for option in options.iter().skip(1) {
        if option.0 < best.0 {
            best = *option;
        }
    }
    let left = if best.1 { reversed(a) } else { a.clone() };
    let right = if best.2 { reversed(b) } else { b.clone() };
    (left, right, best.0)
}

fn path_join(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let params = &command.params;
    let ids = target_ids(session, params);
    if ids.len() < 2 {
        return Err(CommandError::new("NEED_TWO_CURVES"));
    }
    let tolerance = parse_tolerance(params)?;
    let survivor = ids[0];
    let (first, first_curve) = curve_source(&session.document, survivor)?;
    if !join_context_matches(&session.document, first, first) { return Err(CommandError::invalid_params()); }
    let frame = curve_frame(first);
    let inverse = frame.inverse();
    let mut joined = first_curve.clone();
    if joined.contour.closed || joined.contour.points.is_empty() {
        return Err(CommandError::new("NOT_OPEN"));
    }
    let mut others: Vec<(ObjectId, Curve)> = Vec::new();
    for id in ids.iter().skip(1) {
        if *id == survivor || others.iter().any(|(seen, _)| seen == id) {
            return Err(CommandError::invalid_params());
        }
        let (object, curve) = curve_source(&session.document, *id)?;
        if curve.contour.closed || curve.contour.points.is_empty() {
            return Err(CommandError::new("NOT_OPEN"));
        }
        if !join_context_matches(&session.document, object, first) {
            return Err(CommandError::invalid_params());
        }
        let matrix = inverse * curve_frame(object);
        let projected = Curve {
            contour: contour_in_frame(&curve.contour, matrix),
            holes: curve.holes.iter().map(|contour| contour_in_frame(contour, matrix)).collect(),
            source_object: curve.source_object,
        };
        others.push((*id, projected));
    }
    let plan = if ids.iter().any(|id| carries_path_text(&session.document, *id)) { Some(plan_join(&session.document, &ids)?) } else { None };

    let mut next_point = max_point_id(&joined);
    let mut point_ids: HashSet<_> = joined.contours().flat_map(|contour| contour.points.iter().map(|point| point.id)).collect();
    let mut remapped: Vec<Value> = Vec::new();
    let mut welded: Vec<Value> = Vec::new();
    for (other_id, other) in &others {
        let (left, mut right, gap) = oriented_pair(&joined.contour, &other.contour, frame);
        let mut points = left.points;
        if gap <= tolerance {
            let dropped = right.points.remove(0);
            if let Some(end) = points.last_mut() {
                end.handle_out = dropped.handle_out;
                welded.push(json!({ "survivor": end.id.0, "removed": dropped.id.0, "object": other_id.0 }));
            }
        }
        for point in right.points.iter_mut() {
            if point_ids.contains(&point.id) {
                next_point = next_point.checked_add(1).ok_or_else(CommandError::invalid_params)?;
                remapped.push(json!({ "object": other_id.0, "from": point.id.0, "to": next_point }));
                point.id = PointId(next_point);
            } else if point.id.0 > next_point {
                next_point = point.id.0;
            }
            point_ids.insert(point.id);
        }
        points.extend(right.points);

        joined.contour = Contour { points, closed: false };
        for mut hole in other.holes.clone() {
            for point in &mut hole.points {
                next_point = next_point.checked_add(1).ok_or_else(CommandError::invalid_params)?;
                remapped.push(json!({ "object": other_id.0, "from": point.id.0, "to": next_point }));
                point.id = PointId(next_point);
                point_ids.insert(point.id);
            }
            joined.holes.push(hole);
        }
    }
    let removed: Vec<u64> = others.iter().map(|(id, _)| id.0).collect();
    let point_ids: Vec<_> = joined.contour.points.iter().map(|p| p.id.0).collect();
    let mut result = first.clone();
    reframe(&mut result, joined)?;
    let mut document = session.document.clone();
    *document.find_object_mut(survivor.0).ok_or_else(CommandError::not_found)? = result;
    let removed_set = others.iter().map(|(id, _)| *id).collect();
    document.detach_objects(&removed_set);
    document.layer_meta.retain(|meta| !removed_set.contains(&meta.object));
    if let Some(plan) = &plan {
        apply_plan(&mut document, plan)?;
    }
    let reply = json!({
        "id": survivor.0,
        "removed": removed,
        "welded": welded,
        "remapped": remapped,
        "points": point_ids,
        "tolerance": tolerance,
    });
    super::document_change::commit(session, &command.name, document, vec![survivor], reply)
}
fn document_max_point_id(doc: &Document) -> u64 {
    doc.objects.iter().filter_map(|object| object.curve()).map(max_point_id).max().unwrap_or(0)
}

fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn path_point_add(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let params = &command.params;
    let id = *target_ids(session, params).first().ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let segment = params.get("segment").ok_or_else(CommandError::invalid_params)?;
    let from = segment.get("from_id").and_then(parse_id).ok_or_else(CommandError::invalid_params)?;
    let to = segment.get("to_id").and_then(parse_id).ok_or_else(CommandError::invalid_params)?;
    let t = match params.get("t") {
        None | Some(Value::Null) => 0.5,
        Some(value) => value.as_f64().ok_or_else(CommandError::invalid_params)?,
    };
    if !t.is_finite() || t <= 0.0 || t >= 1.0 {
        return Err(CommandError::invalid_params());
    }
    let preserve = preserve_transform(params)?;
    let (object, mut curve) = topology_curve(&session.document, id, preserve)?;
    let scope = match params.get("subpath") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().and_then(|index| usize::try_from(index).ok()).ok_or_else(CommandError::invalid_params)?),
    };
    let locate_point = |id| match scope {
        Some(index) => contour_at(&curve, index)?.points.iter().position(|point| point.id == PointId(id)).map(|point| (index, point)),
        None => locate(&curve, PointId(id)),
    };
    let (subpath, from_index) = locate_point(from).ok_or_else(CommandError::not_found)?;
    let (to_subpath, to_index) = locate_point(to).ok_or_else(CommandError::not_found)?;
    if subpath != to_subpath {
        return Err(CommandError::new("NOT_A_SEGMENT"));
    }
    let mut contour = contour_at(&curve, subpath).cloned().ok_or_else(CommandError::not_found)?;
    let count = contour.points.len();
    let next = |i: usize| if i + 1 < count { Some(i + 1) } else if contour.closed && count > 1 { Some(0) } else { None };
    let (start, end, local_t, reversed) = if next(from_index) == Some(to_index) {
        (from_index, to_index, t, false)
    } else if next(to_index) == Some(from_index) {
        (to_index, from_index, 1.0 - t, true)
    } else {
        return Err(CommandError::new("NOT_A_SEGMENT"));
    };
    let new_id = PointId(document_max_point_id(&session.document).max(max_point_id(&curve)) + 1);
    let p0 = contour.points[start].pos;
    let p3 = contour.points[end].pos;
    let inserted = match (contour.points[start].handle_out, contour.points[end].handle_in) {
        (None, None) => PathPoint::sharp(new_id, lerp(p0, p3, local_t)),
        (out, inn) => {
            let c1 = out.unwrap_or(p0);
            let c2 = inn.unwrap_or(p3);
            let q0 = lerp(p0, c1, local_t);
            let q1 = lerp(c1, c2, local_t);
            let q2 = lerp(c2, p3, local_t);
            let r0 = lerp(q0, q1, local_t);
            let r1 = lerp(q1, q2, local_t);
            contour.points[start].handle_out = Some(q0);
            contour.points[end].handle_in = Some(q2);
            PathPoint { id: new_id, pos: lerp(r0, r1, local_t), handle_in: Some(r0), handle_out: Some(r1), kind: crate::curve::NodeKind::Smooth, corner: None }
        }
    };
    let pos = if preserve {
        let [x, y] = inserted.pos;
        let world = curve_frame(&object) * Point::new(x, y);
        [world.x, world.y]
    } else {
        inserted.pos
    };
    let before_id = contour.points[start].id.0;
    let after_id = contour.points[end].id.0;
    contour.points.insert(start + 1, inserted);
    let order: Vec<u64> = contour.points.iter().map(|p| p.id.0).collect();
    set_contour(&mut curve, subpath, contour);
    let before = session.document.clone();
    if let Some(target) = session.document.find_object_mut(id.0) {
        target.kind = ObjectKind::Curve(curve);
        // Subdividing a segment leaves the rendered outline and object frame unchanged.
        if !preserve {
            fit_bounds(target);
        }
    }
    let reply = json!({
        "id": id.0,
        "point": new_id.0,
        "pos": pos,
        "subpath": subpath,
        "between": [before_id, after_id],
        "reversed": reversed,
        "points": order,
    });
    commit(session, &command.name, before, reply)
}
fn path_close(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let params = &command.params;
    let id = *target_ids(session, params).first().ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let subpath = match params.get("subpath") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().ok_or_else(CommandError::invalid_params)? as usize,
    };
    let tolerance = if params.get("tolerance").is_some_and(|v| !v.is_null()) { parse_tolerance(params)? } else { CLOSE_WELD_EPSILON };
    let preserve = preserve_transform(params)?;
    let (object, mut curve) = topology_curve(&session.document, id, preserve)?;
    let mut contour = contour_at(&curve, subpath).cloned().ok_or_else(CommandError::not_found)?;
    if contour.closed {
        return Err(CommandError::new("ALREADY_CLOSED"));
    }
    if contour.points.len() < 2 {
        return Err(CommandError::new("NOT_ENOUGH_POINTS"));
    }
    let first_id = contour.points[0].id.0;
    let last = contour.points[contour.points.len() - 1].clone();
    let mut weld = Value::Null;
    let first = contour.points[0].pos;
    let gap = if preserve {
        let matrix = curve_frame(&object);
        (matrix * Point::new(first[0], first[1])).distance(matrix * Point::new(last.pos[0], last.pos[1]))
    } else {
        distance(first, last.pos)
    };
    if contour.points.len() >= 3 && gap <= tolerance {
        contour.points.pop();
        if contour.points[0].handle_in.is_none() {
            contour.points[0].handle_in = last.handle_in;
        }
        weld = json!({ "survivor": first_id, "removed": last.id.0 });
    }
    let closing_from = contour.points[contour.points.len() - 1].id.0;
    contour.closed = true;
    let order: Vec<u64> = contour.points.iter().map(|p| p.id.0).collect();
    set_contour(&mut curve, subpath, contour);
    let result = place_topology(object, curve, preserve)?;
    let before = session.document.clone();
    *session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)? = result;
    let reply = json!({
        "id": id.0,
        "subpath": subpath,
        "closed": true,
        "closing_segment": { "from": closing_from, "to": first_id },
        "welded": weld,
        "points": order,
    });
    commit(session, &command.name, before, reply)
}
