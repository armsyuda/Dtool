use super::{
    combine, flood_object_shapes, normalize, remainder_mask, shape_area, shape_contains,
    shapes_to_curve, FloodReplacement, Object, ObjectId, Ring, Shape, VisibleFloodRegion, MIN_AREA,
};
use crate::clip_mask::ClipRegions;
use crate::commands::appearance::{Kind, PaintSlot};
use crate::paint::{Colour, Paint, Transparency};
use dtool_protocol::CommandError;
use i_overlay::core::fill_rule::FillRule as OverlayFillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::slice::FloatSlice;
use kurbo::{Point, Rect};
use std::collections::HashMap;

pub(super) struct Surface<'a> {
    object: &'a Object,
    bounds: Rect,
    solid: Option<Colour>,
}

impl<'a> Surface<'a> {
    pub(super) fn new(object: &'a Object, bounds: Rect) -> Self {
        let mut fills = std::iter::once(&object.fill).chain(&object.additional_fills).enumerate()
            .map(|(index, paint)| (PaintSlot { kind: Kind::Fill, index }, paint))
            .filter(|(slot, paint)| visibly_filled(paint) && object.hidden_appearance.is_visible(*slot));
        let solid = match (fills.next(), fills.next()) {
            (Some((slot, Paint::Solid { colour })), None) if object.appearance_blends.get(slot).is_normal() => Some(*colour),
            _ => None,
        };
        Self { object, bounds, solid }
    }
}

pub(super) fn visibly_filled(paint: &Paint) -> bool {
    match paint {
        Paint::None => false,
        Paint::Solid { colour } => colour.a != 0,
        _ => true,
    }
}

pub(super) fn ring_bounds(ring: &Ring) -> Option<Rect> {
    let [x, y] = *ring.first()?;
    Some(ring.iter().skip(1).fold(Rect::new(x, y, x, y), |bounds, &[x, y]| {
        Rect::new(bounds.x0.min(x), bounds.y0.min(y), bounds.x1.max(x), bounds.y1.max(y))
    }))
}

pub(super) fn connected_region(
    doc: &crate::Document,
    clips: &ClipRegions<'_>,
    surfaces: &mut [Surface<'_>],
    paths: &[Ring],
    barriers: &[usize],
    at: [f64; 2],
    replace: bool,
) -> Result<Option<VisibleFloodRegion>, CommandError> {
    let usable = |surface: &Surface<'_>| surface.solid.filter(|colour| replace || colour.a == 255);
    if surfaces.iter().filter(|surface| usable(surface).is_some()).take(2).count() < 2 {
        return Ok(None);
    }
    let order: HashMap<_, _> = doc.objects.iter().enumerate()
        .map(|(index, object)| (object.id, (index, object))).collect();
    surfaces.sort_by_cached_key(|surface| stack_path(&order, surface.object.id));
    let point = Point::new(at[0], at[1]);
    let Some(top) = surfaces.iter().rev().find(|surface| {
        surface.bounds.contains(point)
            && flood_object_shapes(doc, clips, surface.object).iter().any(|shape| shape_contains(shape, at))
    }) else { return Ok(None) };
    let Some(colour) = usable(top).filter(|_| plain_layer_stack(&order, top.object.id)) else { return Ok(None) };
    let matches = |surface: &Surface<'_>| {
        surface.solid == Some(colour) && plain_layer_stack(&order, surface.object.id)
    };
    if surfaces.iter().filter(|surface| matches(surface)).take(2).count() < 2 {
        return Ok(None);
    }

    // Limit compositing to the connected component of the matching fills, then
    // resolve foreground occlusion in drawing order within that component.
    let mut geometry = HashMap::new();
    let same_colour = surfaces.iter().filter(|surface| matches(surface))
        .flat_map(|surface| {
            let shapes = flood_object_shapes(doc, clips, surface.object);
            let rings = shapes.iter().flatten().cloned().collect::<Vec<_>>();
            geometry.insert(surface.object.id, shapes);
            rings
        }).collect::<Vec<_>>();
    let Some(potential) = normalize(&same_colour, super::FillRule::NonZero)
        .into_iter().find(|shape| shape_contains(shape, at))
    else { return Ok(None) };
    let Some(bounds) = potential.first().and_then(ring_bounds) else { return Ok(None) };
    let candidate = [potential];
    let depth = if colour.a == 255 {
        None
    } else {
        // Opacity changes where an additional coat overlaps. Count the matching
        // paint above the first different fill at the clicked point.
        Some(surfaces.iter().rev().filter(|surface| {
            surface.bounds.contains(point)
                && geometry.entry(surface.object.id)
                    .or_insert_with(|| flood_object_shapes(doc, clips, surface.object))
                    .iter().any(|shape| shape_contains(shape, at))
        }).take_while(|surface| matches(surface)).count())
    };
    let mut levels = vec![Vec::new()];
    for surface in surfaces.iter().filter(|surface| surface.bounds.intersect(bounds).area() > 0.0) {
        let shapes = geometry.entry(surface.object.id)
            .or_insert_with(|| flood_object_shapes(doc, clips, surface.object));
        let inside = combine(shapes, &candidate, OverlayRule::Intersect);
        if matches(surface) {
            add_coverage(&mut levels, &inside, depth.map_or(1, |count| count + 1));
        } else {
            for level in &mut levels {
                *level = combine(level, &inside, OverlayRule::Difference);
            }
            while levels.len() > 1 && levels.last().is_some_and(Vec::is_empty) {
                levels.pop();
            }
        }
    }
    let index = depth.unwrap_or(1).saturating_sub(1);
    let Some(coverage) = levels.get(index) else { return Ok(None) };
    let coverage = match levels.get(index + 1) {
        Some(denser) => combine(coverage, denser, OverlayRule::Difference),
        None => coverage.clone(),
    };
    let Some(connected) = coverage.into_iter().find(|shape| shape_contains(shape, at)) else { return Ok(None) };
    let dividing_paths = barriers.iter().map(|&index| &paths[index])
        .filter(|ring| ring_bounds(ring).is_some_and(|rect| {
            rect.x0 <= bounds.x1 && rect.x1 >= bounds.x0 && rect.y0 <= bounds.y1 && rect.y1 >= bounds.y0
        }))
        .cloned().collect::<Vec<_>>();
    let shape = if dividing_paths.is_empty() {
        connected
    } else {
        let Some(face) = connected.slice_by_as::<i64>(&dividing_paths, OverlayFillRule::EvenOdd)
            .into_iter().find(|shape| shape_contains(shape, at))
        else { return Ok(None) };
        face
    };
    let target = [shape];
    let Some(curve) = shapes_to_curve(&target) else { return Ok(None) };
    let replacements = if replace {
        plan_replacements(doc, surfaces, &geometry, &matches, &target)?
    } else {
        Vec::new()
    };
    Ok(Some(VisibleFloodRegion { curve, replacements }))
}

/// Each level contains the area covered by at least that many consecutive coats.
fn add_coverage(levels: &mut Vec<Vec<Shape>>, inside: &[Shape], limit: usize) {
    let previous_levels = levels.len();
    if previous_levels < limit && let Some(last) = levels.last() {
        let next = combine(last, inside, OverlayRule::Intersect);
        if !next.is_empty() {
            levels.push(next);
        }
    }
    for index in (1..previous_levels).rev() {
        let overlap = combine(&levels[index - 1], inside, OverlayRule::Intersect);
        levels[index] = combine(&levels[index], &overlap, OverlayRule::Union);
    }
    levels[0] = combine(&levels[0], inside, OverlayRule::Union);
}

fn plan_replacements(
    doc: &crate::Document,
    surfaces: &[Surface<'_>],
    geometry: &HashMap<ObjectId, Vec<Shape>>,
    matches: &impl Fn(&Surface<'_>) -> bool,
    target: &[Shape],
) -> Result<Vec<FloodReplacement>, CommandError> {
    let Some(bounds) = target.first().and_then(|shape| shape.first()).and_then(ring_bounds) else {
        return Ok(Vec::new());
    };
    let mut replaceable = target.to_vec();
    let mut uncovered = target.to_vec();
    let mut portions = Vec::new();
    for surface in surfaces.iter().rev().filter(|surface| surface.bounds.intersect(bounds).area() > 0.0) {
        let Some(outline) = geometry.get(&surface.object.id) else { continue };
        if !matches(surface) {
            replaceable = combine(&replaceable, outline, OverlayRule::Difference);
            continue;
        }
        let erase = combine(&replaceable, outline, OverlayRule::Intersect);
        if erase.iter().map(shape_area).sum::<f64>() <= MIN_AREA {
            continue;
        }
        let paint = combine(&uncovered, &erase, OverlayRule::Intersect);
        uncovered = combine(&uncovered, &paint, OverlayRule::Difference);
        portions.push((surface.object, outline, erase, paint));
    }
    if portions.len() < 2 {
        return Ok(Vec::new());
    }
    portions.into_iter().map(|(object, outline, erase, paint)| {
        let mask = if combine(outline, &erase, OverlayRule::Difference).iter().map(shape_area).sum::<f64>() > MIN_AREA {
            Some(remainder_mask(doc, object, &erase).ok_or_else(|| CommandError::new("CANNOT_EXECUTE"))?)
        } else {
            None
        };
        Ok(FloodReplacement {
            source: object.id,
            whole: mask.is_none() && combine(outline, &paint, OverlayRule::Difference).iter().map(shape_area).sum::<f64>() <= MIN_AREA,
            mask,
            removed_region: shapes_to_curve(&erase).ok_or_else(|| CommandError::new("CANNOT_EXECUTE"))?,
            paint_region: shapes_to_curve(&paint),
        })
    }).collect()
}

fn stack_path(order: &HashMap<ObjectId, (usize, &Object)>, id: ObjectId) -> Vec<usize> {
    let mut path = Vec::new();
    let mut cursor = Some(id);
    for _ in 0..64 {
        let Some((index, object)) = cursor.and_then(|id| order.get(&id)) else { break };
        path.push(*index);
        cursor = object.parent_id;
    }
    path.reverse();
    path
}

fn plain_layer_stack(order: &HashMap<ObjectId, (usize, &Object)>, id: ObjectId) -> bool {
    use crate::blend::BlendMode;
    let mut cursor = Some(id);
    for _ in 0..64 {
        let Some((_, object)) = cursor.and_then(|id| order.get(&id)) else { return true };
        if object.opacity != 1.0 || !matches!(object.blend_mode, BlendMode::Normal | BlendMode::PassThrough)
            || object.transparency != Transparency::None || object.blend_ranges.is_some() || !object.effects.is_empty()
        {
            return false;
        }
        cursor = object.parent_id;
    }
    false
}
