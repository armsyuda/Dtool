use kurbo::ParamCurve;

use super::*;

pub const CUT_WITH_KEY: &str = "path.curves.cut_with_key";

pub(super) fn operands<'a>(session: &'a Session, params: &Value) -> Result<(&'a Object, Vec<&'a Object>), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let key_id = match params.get("key_object_id") {
        Some(value) => value.as_u64().map(ObjectId),
        None => session.selection.explicit_key(),
    }.ok_or_else(CommandError::invalid_params)?;
    let ids = targets(session, params);
    if ids.len() < 2 || !ids.contains(&key_id) {
        return Err(CommandError::invalid_params());
    }
    let doc = &session.document;
    let key = doc.get_object(key_id).ok_or_else(CommandError::not_found)?;
    let mut sources = Vec::with_capacity(ids.len() - 1);
    for id in ids {
        let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
        if doc.is_object_locked(object)
            || !matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_))
            || object.clip_role != crate::object::ClipRole::None
            || object.page_id != key.page_id
            || crate::artboard::owner(doc, id).map(|board| board.id) != crate::artboard::owner(doc, key_id).map(|board| board.id)
            || doc.objects().iter().any(|dependent| dependent.parent_id == Some(id)
                || dependent.path_text_data().is_some_and(|text| text.path == id))
            || crate::raster_source::compositing_mask(doc, object).is_some()
            || crate::commands::symbols::object_link(doc, id).is_some()
        {
            return Err(CommandError::invalid_params());
        }
        if id != key_id {
            sources.push(object);
        }
    }
    Ok((key, sources))
}

fn paths_for_cut(outline: &BezPath, tolerance: f64) -> Result<Vec<Ring>, CommandError> {
    let mut paths = Vec::new();
    let mut count = 0;
    for (mut points, closed) in crate::outline::flatten_subpaths(outline, tolerance) {
        if closed && points.first() != points.last() {
            points.push(points[0]);
        }
        count += points.len();
        if count > MAX_BOOLEAN_POINTS {
            return Err(request_too_large());
        }
        paths.push(points);
    }
    Ok(paths)
}

/// Join the original Bézier spans for the common cut across a single contour.
/// The polygon result determines topology, while the editable seam follows the key.
fn curved_seam(
    target: &BezPath, key: &BezPath, boundary: &Ring, cutter: &mut knife_open::KnifeCut, tolerance: f64,
) -> Result<Option<Vec<BezPath>>, CommandError> {
    let Some(mut sides) = cutter.open_closed(target)? else {return Ok(None)};
    if sides.len() != 2 {
        return Ok(None);
    }
    let mut target_edges = knife_open::KnifeCut::new(boundary);
    let (_, spans) = target_edges.split(vec![key.clone()])?;
    let ends = |path: &BezPath| {
        let first = path.segments().next()?.start();
        let last = path.segments().last()?.end();
        Some((first, last))
    };
    let near = |a: kurbo::Point, b: kurbo::Point| a.distance_squared(b) <= (tolerance * 3.0).powi(2);
    for side in &mut sides {
        let (start, end) = ends(side).ok_or_else(CommandError::invalid_params)?;
        let mut seam = None;
        for span in &spans {
            let Some((from, to)) = ends(span) else {continue};
            if near(from, end) && near(to, start) {
                seam = Some(span.clone());
                break;
            }
            if near(from, start) && near(to, end) {
                seam = Some(span.reverse_subpaths());
                break;
            }
        }
        let Some(seam) = seam else {return Ok(None)};
        side.extend(seam.elements().iter().skip(1).copied());
        side.close_path();
    }
    Ok(Some(sides))
}

pub(super) fn run(session: &mut Session, params: &Value) -> CommandResult {
    let (key, sources) = operands(session, params)?;
    let tolerance = tolerance_of(params);
    check_tolerance(tolerance)?;
    let outline = |object: &Object| {
        let path = object_outline_world(object).filter(path_is_finite).ok_or_else(CommandError::invalid_params)?;
        if path.elements().len() > MAX_BOOLEAN_POINTS {return Err(request_too_large());}
        Ok(path)
    };
    let key_id = key.id;
    let key_outline = outline(key)?;
    let cuts = paths_for_cut(&key_outline, tolerance)?;
    if cuts.is_empty() {
        return Err(CommandError::new("NOTHING_TO_CUT"));
    }
    let (closed_key, open_key) = knife_open::partition(&key_outline);
    let mut cutter = knife_open::KnifeCut::from_paths(&cuts);
    let mut budget = 0;
    let mut curve_work = 1_000_000;
    let mut plan = Vec::new();
    for source in sources {
        let (closed, open) = knife_open::partition(&outline(source)?);
        let rings = if closed.is_empty() {
            Vec::new()
        } else {
            flatten_closed(std::slice::from_ref(&closed), tolerance, &mut budget)?
        };
        let regions = normalize(&rings, source.fill_rule);
        let sliced = if regions.is_empty() {Vec::new()} else {
            regions.slice_by_as::<i64>(&cuts, OverlayFillRule::EvenOdd)
        };
        let changed_closed = sliced.len() > regions.len();
        let (changed_open, open_pieces) = cutter.split(open)?;
        if !changed_closed && !changed_open {
            continue;
        }
        let mut pieces = Vec::new();
        if changed_closed {
            let exact = match (open_key.as_slice(), rings.as_slice()) {
                ([key_path], [ring]) if closed_key.is_empty() && regions.len() == 1 && regions[0].len() == 1 && sliced.len() == 2 => {
                    let mut boundary = ring.clone();
                    boundary.push(ring[0]);
                    curved_seam(&closed, key_path, &boundary, &mut cutter, tolerance)?
                }
                _ => None,
            };
            if let Some(sides) = exact {
                for side in sides {
                    pieces.push(cut_piece(source, &side, FillRule::EvenOdd)?);
                }
            } else {
                let edges = bezier_edges::Edges::new([&closed, &key_outline], tolerance);
                for piece in sliced.iter().filter(|piece| shape_area(piece) > MIN_AREA) {
                    let path = edges.restore(piece, &mut curve_work);
                    pieces.push(cut_piece(source, &path, FillRule::EvenOdd)?);
                }
            }
        } else if !closed.is_empty() {
            pieces.push(cut_piece(source, &closed, source.fill_rule)?);
        }
        for path in open_pieces {
            pieces.push(cut_piece(source, &path, source.fill_rule)?);
        }
        plan.push((source.id, pieces));
    }
    if plan.is_empty() {
        return Err(CommandError::new("NOTHING_TO_CUT"));
    }
    let before = snapshot(session);
    let mut created = Vec::new();
    let mut deleted = vec![key_id];
    session.document.objects.retain(|object| object.id != key_id);
    for (id, pieces) in plan {
        let index = session.document.objects.iter().position(|object| object.id == id)
            .expect("cut plan only contains distinct objects in the document");
        session.document.objects.remove(index);
        deleted.push(id);
        for (offset, piece) in pieces.into_iter().enumerate() {
            created.push(place(session, index + offset, piece));
        }
    }
    session.selection.set(created.clone(), SelectOp::Replace);
    commit_snapshot(session, CUT_WITH_KEY, before, json!({"created":created,"deleted":deleted,"key_object_id":key_id}))
}
