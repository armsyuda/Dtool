use super::BLENDS_KEY;
use crate::commands::appearance::{AppearanceSlots, Kind, PaintSlot, RowBlend};
use crate::curve::{Contour, Curve, PathPoint, PointId};
use crate::document::Document;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::paint::{Colour, GradientGeom, GradientStop, Paint, StrokeStyle, interpolate_stops, lerp_colour};
use kurbo::{Affine, BezPath, ParamCurve, ParamCurveArclen, PathEl, Point, QuadBez, Rect, Shape};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_BLEND_STEPS: u32 = 200;
const SAMPLES: usize = 64;
pub const BLEND_SPACING: std::ops::RangeInclusive<u32> = 5..=95;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlendLayout {
    pub spacing: u32,
    pub spine: [f32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_id: Option<ObjectId>,
}

impl Default for BlendLayout {
    fn default() -> Self {
        Self { spacing: 50, spine: [0.0; 2], path_id: None }
    }
}

/// Keep the source objects anchored and connect them to the guide's own endpoints.
pub fn linked_blend_path(doc: &Document, from: ObjectId, to: ObjectId, path_id: ObjectId) -> Option<BezPath> {
    let a = doc.get_object(from)?;
    let b = doc.get_object(to)?;
    let guide = doc.get_object(path_id)?;
    let ObjectKind::Curve(curve) = &guide.kind else { return None };
    if path_id == from || path_id == to || guide.page_id != a.page_id
        || curve.contour.closed || !curve.holes.is_empty() || is_generated(doc, path_id)
    {
        return None;
    }
    let [x, y] = guide.pos.map(f64::from);
    let matrix = Affine::new(crate::geometry::object_matrix(guide).map(f64::from));
    let outline = matrix * Affine::translate((x, y)) * crate::outline::object_outline(guide)?;
    let outline = crate::commands::warp::live_warp_outline(doc, path_id, &outline).unwrap_or(outline);
    let (PathEl::MoveTo(start), rest) = outline.elements().split_first()? else { return None };
    let end = outline.segments().last()?.end();
    let [ax, ay] = crate::geometry::box_centre(a).map(f64::from);
    let [bx, by] = crate::geometry::box_centre(b).map(f64::from);
    let mut path = BezPath::new();
    path.move_to((ax, ay));
    if *start != Point::new(ax, ay) {
        path.line_to(*start);
    }
    path.extend(rest.iter().copied());
    if end != Point::new(bx, by) {
        path.line_to((bx, by));
    }
    Some(path)
}

impl BlendLayout {
    pub fn is_valid(self) -> bool {
        BLEND_SPACING.contains(&self.spacing) && self.spine.into_iter().all(f32::is_finite)
    }

    pub fn path(self, from: [f32; 2], to: [f32; 2]) -> QuadBez {
        let from = Point::new(f64::from(from[0]), f64::from(from[1]));
        let to = Point::new(f64::from(to[0]), f64::from(to[1]));
        let midpoint = from.midpoint(to);
        let control = midpoint + kurbo::Vec2::new(2.0 * f64::from(self.spine[0]), 2.0 * f64::from(self.spine[1]));
        QuadBez::new(from, control, to)
    }

    pub(super) fn from_entry(entry: &Value) -> Self {
        serde_json::from_value::<Self>(entry["layout"].clone())
            .ok()
            .filter(|layout| layout.is_valid())
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlendInfo {
    pub id: u64,
    pub steps: u32,
    pub layout: Option<BlendLayout>,
    pub from: ObjectId,
    pub to: ObjectId,
}

pub fn entries(doc: &Document) -> &[Value] {
    doc.export_defaults
        .get(BLENDS_KEY)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

pub fn is_generated(doc: &Document, object: ObjectId) -> bool {
    entries(doc)
        .iter()
        .any(|entry| generated(entry).any(|id| id == object))
}

fn generated(entry: &Value) -> impl Iterator<Item = ObjectId> + '_ {
    entry["generated"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(ObjectId)
}

pub fn blend_for_object(doc: &Document, object: ObjectId) -> Option<BlendInfo> {
    entries(doc).iter().rev().find_map(|entry| {
        if entry["from"].as_u64() != Some(object.0)
            && entry["to"].as_u64() != Some(object.0)
            && !generated(entry).any(|id| id == object)
        {
            return None;
        }
        let steps = u32::try_from(entry["steps"].as_u64()?).ok()?;
        Some(BlendInfo {
            id: entry["id"].as_u64()?,
            steps,
            layout: (entry["live"].as_bool() == Some(true)).then(|| BlendLayout::from_entry(entry)),
            from: ObjectId(entry["from"].as_u64()?),
            to: ObjectId(entry["to"].as_u64()?),
        })
    })
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn tint(a: &Paint, b: &Paint, t: f32) -> Paint {
    match (a, b) {
        (Paint::None, paint) => fade_paint(paint, t),
        (paint, Paint::None) => fade_paint(paint, 1.0 - t),
        (Paint::Solid { colour: a }, Paint::Solid { colour: b }) => Paint::Solid {
            colour: lerp_colour(*a, *b, t),
        },
        (Paint::Gradient { kind, stops: a, geom: ga }, Paint::Gradient { kind: next, stops: b, geom: gb })
            if kind == next => Paint::Gradient {
                kind: *kind,
                geom: blend_gradient_geometry(*ga, *gb, f64::from(t)),
                stops: blend_gradient_stops(a, b, t),
            },
        (Paint::Gradient { kind, stops, geom }, Paint::Solid { colour }) => Paint::Gradient {
            kind: *kind,
            geom: *geom,
            stops: stops.iter().map(|stop| GradientStop {
                colour: lerp_colour(stop.colour, *colour, t), ..*stop
            }).collect(),
        },
        (Paint::Solid { colour }, Paint::Gradient { kind, stops, geom }) => Paint::Gradient {
            kind: *kind,
            geom: *geom,
            stops: stops.iter().map(|stop| GradientStop {
                colour: lerp_colour(*colour, stop.colour, t), ..*stop
            }).collect(),
        },
        _ => if t < 0.5 { a.clone() } else { b.clone() },
    }
}

fn fade_paint(paint: &Paint, opacity: f32) -> Paint {
    let fade = |mut colour: Colour| {
        colour.a = (f32::from(colour.a) * opacity).round() as u8;
        colour
    };
    match paint {
        Paint::Solid { colour } => Paint::Solid { colour: fade(*colour) },
        Paint::Gradient { kind, stops, geom } => Paint::Gradient {
            kind: *kind,
            geom: *geom,
            stops: stops.iter().map(|stop| GradientStop { colour: fade(stop.colour), ..*stop }).collect(),
        },
        paint if opacity >= 0.5 => paint.clone(),
        _ => Paint::None,
    }
}

fn blend_gradient_geometry(a: GradientGeom, b: GradientGeom, t: f64) -> GradientGeom {
    let axis = |geom: GradientGeom| {
        let dx = geom.end[0] - geom.start[0];
        let dy = geom.end[1] - geom.start[1];
        (dy.atan2(dx), dx.hypot(dy))
    };
    let (mut angle_a, length_a) = axis(a);
    let (mut angle_b, length_b) = axis(b);
    if length_a == 0.0 { angle_a = angle_b; }
    if length_b == 0.0 { angle_b = angle_a; }
    let turn = (angle_b - angle_a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    let (sin, cos) = (angle_a + t * turn).sin_cos();
    let length = length_a + t * (length_b - length_a);
    let start = std::array::from_fn(|i| a.start[i] + t * (b.start[i] - a.start[i]));
    GradientGeom {
        start,
        end: [start[0] + length * cos, start[1] + length * sin],
        ratio: a.ratio + t * (b.ratio - a.ratio),
    }
}

fn blend_gradient_stops(a: &[GradientStop], b: &[GradientStop], t: f32) -> Vec<GradientStop> {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    b.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    let mut offsets: Vec<_> = a.iter().chain(&b).map(|stop| stop.offset).collect();
    offsets.sort_by(f32::total_cmp);
    offsets.dedup();
    let mut result = Vec::with_capacity(offsets.len());
    for offset in offsets {
        let [a0, a1] = stop_sides(&a, offset);
        let [b0, b1] = stop_sides(&b, offset);
        let left = lerp_colour(a0, b0, t);
        let right = lerp_colour(a1, b1, t);
        result.push(GradientStop { offset, colour: left });
        if left != right {
            result.push(GradientStop { offset, colour: right });
        }
    }
    result
}

fn stop_sides(stops: &[GradientStop], offset: f32) -> [Colour; 2] {
    let first = stops.partition_point(|stop| stop.offset < offset);
    let end = stops.partition_point(|stop| stop.offset <= offset);
    if first < end {
        [stops[first].colour, stops[end - 1].colour]
    } else {
        [interpolate_stops(stops, offset); 2]
    }
}

fn blend_stroke(target: &mut StrokeStyle, a: &StrokeStyle, b: &StrokeStyle, t: f32) {
    target.paint = tint(&a.paint, &b.paint, t);
    target.width = lerp(a.width, b.width, t);
}

fn blend_row_mode(slot: PaintSlot, from: (&Object, bool), to: (&Object, bool), t: f32) -> RowBlend {
    let available = |(object, present): (&Object, bool)| present.then(|| object.appearance_blends.get(slot));
    let (from, to) = (available(from), available(to));
    if t < 0.5 { from.or(to) } else { to.or(from) }.unwrap_or_default()
}

fn blend_extra_stroke(a: Option<&StrokeStyle>, b: Option<&StrokeStyle>, t: f32) -> Option<StrokeStyle> {
    let absent = StrokeStyle { paint: Paint::None, width: 0.0, ..StrokeStyle::default() };
    let mut stroke = if t < 0.5 { a.or(b) } else { b.or(a) }?.clone();
    blend_stroke(&mut stroke, a.unwrap_or(&absent), b.unwrap_or(&absent), t);
    (stroke.paint != Paint::None).then_some(stroke)
}

fn preferred_stack(object: &Object) -> Option<Vec<PaintSlot>> {
    (!object.appearance_stack.is_empty()).then(|| {
        object.appearance().render_order(object.paint_order).into_iter()
            .map(|(kind, index)| PaintSlot { kind, index }).collect()
    })
}

fn stack_for_rows(preferred: &[PaintSlot], generated: &[PaintSlot], paint_order: crate::paint::PaintOrder) -> Vec<Kind> {
    let mut order: Vec<_> = preferred.iter().copied().filter(|slot| generated.contains(slot)).collect();
    let fill_first = paint_order == crate::paint::PaintOrder::StrokeOver;
    for &slot in generated {
        if order.contains(&slot) {
            continue;
        }
        let position = order.iter().position(|row| row.kind == slot.kind && row.index > slot.index)
            .or_else(|| order.iter().rposition(|row| row.kind == slot.kind).map(|index| index + 1))
            .unwrap_or_else(|| if (slot.kind == Kind::Fill) == fill_first { 0 } else { order.len() });
        order.insert(position, slot);
    }
    order.into_iter().map(|slot| slot.kind).collect()
}

fn sample(points: &[[f64; 2]], closed: bool) -> Vec<[f64; 2]> {
    let mut points = points.to_vec();
    if closed {
        points.push(points[0]);
    }
    let mut lengths = Vec::with_capacity(points.len());
    lengths.push(0.0);
    for pair in points.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        lengths.push(lengths.last().copied().unwrap_or(0.0) + (a[0] - b[0]).hypot(a[1] - b[1]));
    }
    let length = lengths.last().copied().unwrap_or(0.0);
    let mut edge = 1;
    (0..SAMPLES)
        .map(|index| {
            let distance =
                length * index as f64 / (if closed { SAMPLES } else { SAMPLES - 1 }) as f64;
            while edge + 1 < lengths.len() && lengths[edge] < distance {
                edge += 1;
            }
            let a = points[edge - 1];
            let b = points[edge];
            let span = lengths[edge] - lengths[edge - 1];
            let t = if span > 0.0 {
                (distance - lengths[edge - 1]) / span
            } else {
                0.0
            };
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        })
        .collect()
}

type Pair = (Vec<[f64; 2]>, Vec<[f64; 2]>, bool);

struct ContourPairs {
    paths: Vec<Pair>,
    from_closed: bool,
    to_closed: bool,
}

struct Outline {
    path: BezPath,
    samples: Vec<[f64; 2]>,
    closed: bool,
    bounds: Rect,
    area: f64,
    anchor: Point,
}

fn outlines(object: &Object) -> Option<Vec<Outline>> {
    let outline = crate::outline::object_outline(object)?;
    let contours = crate::outline::flatten_subpaths(&outline, 0.2);
    Some(contours.into_iter().filter_map(|(points, closed)| {
        let (first, rest) = points.split_first()?;
        if points.len() < if closed { 3 } else { 2 } {
            return None;
        }
        let mut path = BezPath::new();
        path.move_to((first[0], first[1]));
        for &[x, y] in rest {
            path.line_to((x, y));
        }
        if closed {
            path.close_path();
        }
        let bounds = path.bounding_box();
        let centre = bounds.center();
        let anchor = if !closed || path.contains(centre) { centre } else { Point::new(first[0], first[1]) };
        let area = if closed { path.area().abs() } else { 0.0 };
        Some(Outline { path, samples: sample(&points, closed), closed, bounds, area, anchor })
    }).collect())
}

fn outline_children(outlines: &[Outline]) -> Vec<Vec<usize>> {
    let mut tree = vec![Vec::new(); outlines.len() + 1];
    for (index, outline) in outlines.iter().enumerate() {
        let [x, y] = outline.samples[0];
        let parent = outline.closed.then(|| outlines.iter().enumerate()
            .filter(|(_, outer)| outer.closed && outer.area > outline.area
                && outer.bounds.union(outline.bounds) == outer.bounds
                && outer.path.contains(Point::new(x, y)))
            .min_by(|(_, a), (_, b)| a.area.total_cmp(&b.area)).map(|(index, _)| index)).flatten();
        tree[parent.unwrap_or(outlines.len())].push(index);
    }
    for siblings in &mut tree {
        siblings.sort_by(|&a, &b| outlines[b].area.total_cmp(&outlines[a].area));
    }
    tree
}

fn unit_point(point: Point, bounds: Rect) -> Point {
    Point::new((point.x - bounds.x0) / bounds.width().max(f64::EPSILON),
        (point.y - bounds.y0) / bounds.height().max(f64::EPSILON))
}

fn collapse_point(point: Point, source: Rect, target: Rect, container: Option<&Outline>) -> Point {
    let relative = unit_point(point, source);
    let mapped = Point::new(target.x0 + target.width() * relative.x, target.y0 + target.height() * relative.y);
    container.filter(|outline| !outline.path.contains(mapped)).map_or(mapped, |outline| outline.anchor)
}

fn correspond(a: &Object, b: &Object) -> Option<ContourPairs> {
    let from = outlines(a)?;
    let to = outlines(b)?;
    if from.is_empty() || to.is_empty() {
        return None;
    }
    let source_tree = outline_children(&from);
    let target_tree = outline_children(&to);
    let object_bounds = |object: &Object| Rect::new(0.0, 0.0, f64::from(object.size[0].abs()), f64::from(object.size[1].abs()));
    let mut branches = vec![(Some(from.len()), Some(to.len()), None, None)];
    let mut pairs = Vec::new();
    while let Some((source_parent, target_parent, source_collapse, target_collapse)) = branches.pop() {
        let source = source_parent.map_or(&[][..], |i| &source_tree[i]);
        let target = target_parent.map_or(&[][..], |i| &target_tree[i]);
        let source_frame = source_parent.and_then(|i| from.get(i));
        let target_frame = target_parent.and_then(|i| to.get(i));
        let a_bounds = source_frame.map_or_else(|| object_bounds(a), |outline| outline.bounds);
        let b_bounds = target_frame.map_or_else(|| object_bounds(b), |outline| outline.bounds);
        let mut available = target.to_vec();
        for &i in source {
            let ring = &from[i];
            let relative = unit_point(ring.anchor, a_bounds);
            let best = available.iter().enumerate().filter(|&(_, &j)| to[j].closed == ring.closed)
                .min_by(|&(_, &j), &(_, &k)| {
                    let distance = |j: usize| unit_point(to[j].anchor, b_bounds).distance_squared(relative);
                    distance(j).total_cmp(&distance(k))
                }).map(|(index, _)| index);
            if let Some(index) = best {
                let j = available.remove(index);
                pairs.push(align_samples(ring, &to[j]));
                branches.push((Some(i), Some(j), None, None));
            } else {
                let anchor = target_collapse.unwrap_or_else(|| collapse_point(ring.anchor, a_bounds, b_bounds, target_frame));
                pairs.push((ring.samples.clone(), vec![[anchor.x, anchor.y]; SAMPLES], ring.closed));
                branches.push((Some(i), None, None, Some(anchor)));
            }
        }
        for j in available {
            let ring = &to[j];
            let anchor = source_collapse.unwrap_or_else(|| collapse_point(ring.anchor, b_bounds, a_bounds, source_frame));
            pairs.push((vec![[anchor.x, anchor.y]; SAMPLES], ring.samples.clone(), ring.closed));
            branches.push((None, Some(j), Some(anchor), None));
        }
    }
    Some(ContourPairs {
        paths: pairs,
        from_closed: from.iter().any(|outline| outline.closed),
        to_closed: to.iter().any(|outline| outline.closed),
    })
}

fn align_samples(from: &Outline, to: &Outline) -> Pair {
    let closed = from.closed;
    let a = from.samples.clone();
    let mut b = to.samples.clone();
    let mean = |points: &[[f64; 2]]| {
        let sum = points
            .iter()
            .fold([0.0; 2], |[x, y], [px, py]| [x + px, y + py]);
        sum.map(|value| value / SAMPLES as f64)
    };
    let [ax, ay] = mean(&a);
    let [bx, by] = mean(&b);
    let mut best = (f64::INFINITY, false, 0);
    for reverse in [false, true] {
        for shift in 0..if closed { SAMPLES } else { 1 } {
            let error = a
                .iter()
                .enumerate()
                .map(|(i, [x, y])| {
                    let j = if reverse { SAMPLES - 1 - i } else { i };
                    let [tx, ty] = b[(j + shift) % SAMPLES];
                    (x - ax - tx + bx).powi(2) + (y - ay - ty + by).powi(2)
                })
                .sum::<f64>();
            if error < best.0 {
                best = (error, reverse, shift);
            }
        }
    }
    if best.1 {
        b.reverse();
    }
    if best.1 {
        b.rotate_right(best.2);
    } else {
        b.rotate_left(best.2);
    }
    (a, b, closed)
}

pub fn objects(doc: &Document, from: ObjectId, to: ObjectId, steps: u32, layout: BlendLayout) -> Option<Vec<Object>> {
    if !(1..=MAX_BLEND_STEPS).contains(&steps) {
        return None;
    }
    let a = doc.get_object(from)?;
    let b = doc.get_object(to)?;
    let supported = |object: &Object| {
        matches!(
            object.kind,
            ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Compound(_)
        )
    };
    if !supported(a) || !supported(b) || a.parent_id != b.parent_id || a.page_id != b.page_id {
        return None;
    }
    let ContourPairs { paths, from_closed, to_closed } = correspond(a, b)?;
    let visible = |object: &Object, kind, index| {
        object.hidden_appearance.is_visible(PaintSlot { kind, index })
    };
    let fill_a = if from_closed && visible(a, Kind::Fill, 0) { &a.fill } else { &Paint::None };
    let fill_b = if to_closed && visible(b, Kind::Fill, 0) { &b.fill } else { &Paint::None };
    let mut stroke_a = a.stroke.clone();
    let mut stroke_b = b.stroke.clone();
    for (source, style) in [(a, &mut stroke_a), (b, &mut stroke_b)] {
        if !visible(source, Kind::Stroke, 0) {
            style.paint = Paint::None;
            style.width = 0.0;
        }
    }
    let extra_strokes_a: Vec<_> = a.additional_strokes.iter().enumerate()
        .map(|(i, stroke)| visible(a, Kind::Stroke, i + 1).then_some(stroke)).collect();
    let extra_strokes_b: Vec<_> = b.additional_strokes.iter().enumerate()
        .map(|(i, stroke)| visible(b, Kind::Stroke, i + 1).then_some(stroke)).collect();
    let extra_a = if from_closed { a.additional_fills.as_slice() } else { &[] };
    let extra_b = if to_closed { b.additional_fills.as_slice() } else { &[] };
    let (stack_a, stack_b) = (preferred_stack(a), preferred_stack(b));
    let from_centre = crate::geometry::box_centre(a);
    let to_centre = crate::geometry::box_centre(b);
    let spine = layout.path(from_centre, to_centre);
    let curved = layout.spine != [0.0; 2];
    let length = if curved { spine.arclen(0.01) } else { 0.0 };
    let guide = layout.path_id.and_then(|id| linked_blend_path(doc, from, to, id));
    let segments: Vec<_> = guide.iter().flat_map(|path| path.segments())
        .map(|segment| (segment, segment.arclen(0.01)))
        .filter(|(_, length)| length.is_finite() && *length > 0.0)
        .collect();
    let guide_length: f64 = segments.iter().map(|(_, length)| length).sum();
    let spacing = (f64::from(layout.spacing) * 0.01).ln() / 0.5_f64.ln();
    Some(
        (1..=steps)
            .map(|index| {
                let fraction = (f64::from(index) / f64::from(steps + 1)).powf(spacing);
                let t = fraction as f32;
                let mut next_id = 0;
                let mut contours = paths.iter().map(|(a, b, closed)| Contour {
                    closed: *closed,
                    points: a
                        .iter()
                        .zip(b)
                        .map(|(p, q)| {
                            next_id += 1;
                            PathPoint::sharp(
                                PointId(next_id),
                                std::array::from_fn(|i| p[i] + (q[i] - p[i]) * f64::from(t)),
                            )
                        })
                        .collect(),
                });
                let mut result = if t < 0.5 { a.clone() } else { b.clone() };
                result.id = ObjectId(0);
                result.name = format!("{} {index}", dtool_strings::text("tool.blend"));
                result.size = std::array::from_fn(|i| lerp(a.size[i], b.size[i], t));
                let mut remaining = guide_length * fraction;
                let guide_position = segments.iter().find_map(|(segment, length)| {
                    if remaining <= *length {
                        Some(segment.eval(segment.inv_arclen(remaining, 0.01)))
                    } else {
                        remaining -= length;
                        None
                    }
                });
                let centre = guide_position.or_else(|| curved.then(|| spine.eval(spine.inv_arclen(length * fraction, 0.01))));
                result.pos = if let Some(point) = centre {
                    let centre = [point.x as f32, point.y as f32];
                    std::array::from_fn(|i| centre[i] - result.size[i].abs() * 0.5)
                } else {
                    std::array::from_fn(|i| lerp(a.pos[i], b.pos[i], t))
                };
                result.rotation_deg = a.rotation_deg
                    + crate::geometry::wrap_degrees(b.rotation_deg - a.rotation_deg) * t;
                result.shear_deg = lerp(a.shear_deg, b.shear_deg, t);
                result.opacity = lerp(a.opacity, b.opacity, t);
                result.fill = tint(fill_a, fill_b, t);
                blend_stroke(&mut result.stroke, &stroke_a, &stroke_b, t);
                result.additional_fills.clear();
                result.additional_strokes.clear();
                result.hidden_appearance = Default::default();
                result.appearance_blends = Default::default();
                let mut generated = Vec::new();
                let fill_ink = |paint: &Paint| *paint != Paint::None;
                let stroke_ink = |style: &StrokeStyle| style.width > 0.0 && fill_ink(&style.paint);
                let primary_fill = PaintSlot { kind: Kind::Fill, index: 0 };
                let primary_stroke = PaintSlot { kind: Kind::Stroke, index: 0 };
                if fill_ink(&result.fill) {
                    result.appearance_blends.set(primary_fill,
                        blend_row_mode(primary_fill, (a, fill_ink(fill_a)), (b, fill_ink(fill_b)), t));
                }
                if stroke_ink(&result.stroke) {
                    result.appearance_blends.set(primary_stroke,
                        blend_row_mode(primary_stroke, (a, stroke_ink(&stroke_a)), (b, stroke_ink(&stroke_b)), t));
                }
                for index in 0..extra_a.len().max(extra_b.len()) {
                    let original = PaintSlot { kind: Kind::Fill, index: index + 1 };
                    let from = extra_a.get(index).filter(|paint| visible(a, Kind::Fill, original.index) && fill_ink(paint));
                    let to = extra_b.get(index).filter(|paint| visible(b, Kind::Fill, original.index) && fill_ink(paint));
                    let paint = tint(from.unwrap_or(&Paint::None), to.unwrap_or(&Paint::None), t);
                    if fill_ink(&paint) {
                        result.additional_fills.push(paint);
                        generated.push(original);
                        let slot = PaintSlot { kind: Kind::Fill, index: result.additional_fills.len() };
                        result.appearance_blends.set(slot, blend_row_mode(original, (a, from.is_some()), (b, to.is_some()), t));
                    }
                }
                for index in 0..extra_strokes_a.len().max(extra_strokes_b.len()) {
                    let from = extra_strokes_a.get(index).copied().flatten();
                    let to = extra_strokes_b.get(index).copied().flatten();
                    if let Some(stroke) = blend_extra_stroke(from, to, t) {
                        result.additional_strokes.push(stroke);
                        let original = PaintSlot { kind: Kind::Stroke, index: index + 1 };
                        generated.push(original);
                        let slot = PaintSlot { kind: Kind::Stroke, index: result.additional_strokes.len() };
                        result.appearance_blends.set(slot,
                            blend_row_mode(original, (a, from.is_some_and(stroke_ink)), (b, to.is_some_and(stroke_ink)), t));
                    }
                }
                if fill_ink(&result.fill) || !result.additional_fills.is_empty() {
                    generated.push(primary_fill);
                }
                if result.stroke != StrokeStyle::default() || !result.additional_strokes.is_empty() {
                    generated.push(primary_stroke);
                }
                let preferred = if t < 0.5 { stack_a.as_deref() } else { stack_b.as_deref() };
                result.appearance_stack = preferred.map_or_else(Vec::new, |order| {
                    stack_for_rows(order, &generated, result.paint_order)
                });
                result.shape = None;
                result.vector_crop = None;
                result.kind = ObjectKind::Curve(Curve::from_contour_and_holes(
                    contours.next().unwrap_or_default(),
                    contours.collect(),
                ));
                result.locked = true;
                result
            })
            .collect(),
    )
}

pub fn refresh_blends(doc: &mut Document) {
    refresh_blends_where(doc, |_, _, _| true);
}

pub(crate) fn refresh_saved_blend_appearance(doc: &mut Document) {
    refresh_blends_where(doc, |doc, from, to| {
        [from, to].into_iter().filter_map(|id| doc.get_object(id)).any(|object| {
            !object.appearance_blends.is_empty() || !object.appearance_stack.is_empty()
        })
    });
}

fn refresh_blends_where(doc: &mut Document, include: impl Fn(&Document, ObjectId, ObjectId) -> bool) {
    let records: Vec<_> = entries(doc).iter().filter(|entry| entry["live"].as_bool() == Some(true)).cloned().collect();
    for entry in records {
        let ids: Vec<_> = generated(&entry).collect();
        let data = entry["from"]
            .as_u64()
            .zip(entry["to"].as_u64())
            .zip(entry["steps"].as_u64());
        let steps = data.and_then(|((from, to), steps)| {
            let (from, to) = (ObjectId(from), ObjectId(to));
            if !include(doc, from, to) {
                return None;
            }
            objects(
                doc,
                from,
                to,
                u32::try_from(steps).ok()?,
                BlendLayout::from_entry(&entry),
            )
        });
        // Keep the last valid paths so an invalidated blend can still be expanded.
        if let Some(objects) = steps.filter(|objects| objects.len() == ids.len()) {
            for (id, mut replacement) in ids.into_iter().zip(objects) {
                if let Some(slot) = doc.find_object_mut(id.0) {
                    replacement.id = id;
                    *slot = replacement;
                }
            }
        }
    }
}
