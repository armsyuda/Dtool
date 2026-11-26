use dtool_core::blend::BlendMode;
use dtool_core::paint::{GradientGeom, GradientKind, GradientStop};
use kurbo::{Affine, Ellipse, Point, Shape, Vec2};

use super::{ClipRole, Colour, Dictionary, GState, LossKind, Object, ObjectKind, Paint, PathPoint, PdfDocument, PdfMapper, PdfObject, Space, SubPath, deref, dict_of, num};

mod calculator;
mod sampled;

fn numbers(pdf: &PdfDocument, object: &PdfObject) -> Option<Vec<f64>> {
    deref(pdf, object).as_array().ok()?.iter().map(|item| num(deref(pdf, item))).collect()
}

fn interval(pdf: &PdfDocument, dict: &Dictionary, key: &[u8]) -> Option<[f64; 2]> {
    numbers(pdf, dict.get(key).ok()?)?.try_into().ok()
}

fn function_type(pdf: &PdfDocument, function: &Dictionary) -> Option<i64> {
    deref(pdf, function.get(b"FunctionType").ok()?).as_i64().ok()
}

type Components = [f64; 4];
type Limits = [[f64; 2]; 4];

#[derive(Clone, Copy)]
struct SampleSpan {
    offsets: [f64; 2],
    input: [f64; 2],
}

impl SampleSpan {
    fn split_at(self, boundary: f64, cuts: &mut Vec<f64>) {
        let fraction = (boundary - self.input[0]) / (self.input[1] - self.input[0]);
        if fraction > 0.0 && fraction < 1.0 { cuts.push(fraction); }
    }

    fn segments(self, mut cuts: Vec<f64>) -> impl Iterator<Item = Self> {
        cuts.extend([0.0, 1.0]);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        cuts.into_iter().scan(0.0, move |previous, next| {
            let fractions = [*previous, next];
            *previous = next;
            Some(Self {
                offsets: fractions.map(|t| interpolate(self.offsets, t)),
                input: fractions.map(|t| interpolate(self.input, t)),
            })
        }).skip(1)
    }
}

fn interpolate([start, end]: [f64; 2], t: f64) -> f64 {
    start * (1.0 - t) + end * t
}

struct FunctionSamples {
    components: usize,
    stops: Vec<(f32, Components)>,
    remaining_functions: usize,
    remaining_sample_values: usize,
    remaining_calculator_steps: usize,
    limited: bool,
}

impl FunctionSamples {
    fn new(components: usize) -> Self {
        Self {
            components, stops: Vec::new(), remaining_functions: 2048, remaining_sample_values: 262_144,
            remaining_calculator_steps: 2_000_000, limited: false,
        }
    }

    fn push(&mut self, offset: f64, colour: Components) -> Option<()> {
        let stop = (offset as f32, colour);
        if self.stops.last() != Some(&stop) {
            if self.stops.len() >= 1024 { return None; }
            self.stops.push(stop);
        }
        Some(())
    }

    fn subdivide(
        &mut self, span: SampleSpan, [start, end]: [Components; 2],
        evaluate: &impl Fn(f64) -> Components, depth: usize,
    ) -> Option<()> {
        let offset = span.offsets[0].midpoint(span.offsets[1]);
        let input = span.input[0].midpoint(span.input[1]);
        let middle = evaluate(input);
        // Each channel is convex, concave or constant between the clipping breakpoints.
        // Its maximum error from the chord is at most twice the midpoint error.
        let needs_split = (0..self.components)
            .any(|i| (middle[i] - start[i].midpoint(end[i])).abs() > 0.25 / 255.0);
        let distinct_offset = (offset as f32) > span.offsets[0] as f32 && (offset as f32) < span.offsets[1] as f32;
        if needs_split && depth < 48 && distinct_offset {
            let left = SampleSpan { offsets: [span.offsets[0], offset], input: [span.input[0], input] };
            let right = SampleSpan { offsets: [offset, span.offsets[1]], input: [input, span.input[1]] };
            self.subdivide(left, [start, middle], evaluate, depth + 1)?;
            self.subdivide(right, [middle, end], evaluate, depth + 1)
        } else {
            self.limited |= needs_split;
            self.push(span.offsets[1], end)
        }
    }
}

fn exponential_samples(
    pdf: &PdfDocument, function: &Dictionary, domain: [f64; 2], span: SampleSpan,
    range: Limits, output: &mut FunctionSamples,
) -> Option<()> {
    let exponent = num(deref(pdf, function.get(b"N").ok()?))?;
    let integer = exponent.fract() == 0.0;
    let odd = integer && exponent.rem_euclid(2.0) == 1.0;
    if (!integer && domain[0] < 0.0) || (exponent < 0.0 && domain[0] <= 0.0 && domain[1] >= 0.0) {
        return None;
    }
    let endpoint = |key: &[u8], default: f64| -> Option<Components> {
        let mut result = [0.0; 4];
        if let Ok(object) = function.get(key) {
            let values = numbers(pdf, object)?;
            if values.len() != output.components { return None; }
            result[..values.len()].copy_from_slice(&values);
        } else if output.components == 1 {
            result[0] = default;
        } else {
            return None;
        }
        Some(result)
    };
    let first = endpoint(b"C0", 0.0)?;
    let last = endpoint(b"C1", 1.0)?;
    let delta: Components = std::array::from_fn(|i| last[i] - first[i]);
    let evaluate = |x: f64| {
        let magnitude = x.abs().powf(exponent);
        let power = if x < 0.0 && odd { -magnitude } else { magnitude };
        std::array::from_fn(|i| {
            let value = if delta[i] == 0.0 { first[i] } else { first[i] + delta[i] * power };
            value.clamp(range[i][0], range[i][1])
        })
    };
    let mut cuts = Vec::new();
    if exponent != 0.0 && exponent != 1.0 { span.split_at(0.0, &mut cuts); }
    if exponent != 0.0 {
        for i in 0..output.components {
            if delta[i] == 0.0 { continue; }
            for limit in range[i] {
                let power = (limit - first[i]) / delta[i];
                if power > 0.0 {
                    let root = power.powf(1.0 / exponent);
                    span.split_at(root, &mut cuts);
                    if integer && !odd { span.split_at(-root, &mut cuts); }
                } else if power < 0.0 && odd {
                    span.split_at(-(-power).powf(1.0 / exponent), &mut cuts);
                }
            }
        }
    }
    for segment in span.segments(cuts) {
        let endpoints = segment.input.map(evaluate);
        output.push(segment.offsets[0], endpoints[0])?;
        output.subdivide(segment, endpoints, &evaluate, 0)?;
    }
    Some(())
}

fn sample_function(
    pdf: &PdfDocument, object: &PdfObject, span: SampleSpan, mut range: Limits,
    output: &mut FunctionSamples, depth: usize,
) -> Option<()> {
    if depth >= 16 { return None; }
    output.remaining_functions = output.remaining_functions.checked_sub(1)?;
    let function = dict_of(pdf, object)?;
    let [low, high] = interval(pdf, function, b"Domain")?;
    if low >= high { return None; }
    if let Ok(limits) = function.get(b"Range") {
        let limits = numbers(pdf, limits)?;
        if limits.len() != output.components * 2 { return None; }
        for (limit, pair) in range.iter_mut().zip(limits.chunks_exact(2)) {
            if pair[0] > pair[1] { return None; }
            *limit = [pair[0].clamp(limit[0], limit[1]), pair[1].clamp(limit[0], limit[1])];
        }
    }
    let mut cuts = Vec::new();
    span.split_at(low, &mut cuts);
    span.split_at(high, &mut cuts);
    let clamp = |x: f64| x.clamp(low, high);
    match function_type(pdf, function)? {
        0 => {
            let stream = deref(pdf, object).as_stream().ok()?;
            sampled::append_samples(pdf, stream, [low, high], span, range, output)?;
        }
        2 => {
            for segment in span.segments(cuts) {
                let segment = SampleSpan { input: segment.input.map(clamp), ..segment };
                exponential_samples(pdf, function, [low, high], segment, range, output)?;
            }
        }
        3 => {
            let sections = deref(pdf, function.get(b"Functions").ok()?).as_array().ok()?;
            if sections.is_empty() || sections.len() > 256 { return None; }
            let mut bounds = vec![low];
            bounds.extend(numbers(pdf, function.get(b"Bounds").ok()?)?);
            bounds.push(high);
            let encode = numbers(pdf, function.get(b"Encode").ok()?)?;
            if bounds.len() != sections.len() + 1 || encode.len() != sections.len() * 2
                || bounds.windows(2).any(|pair| pair[0] >= pair[1]) { return None; }
            for &boundary in &bounds[1..bounds.len() - 1] { span.split_at(boundary, &mut cuts); }
            for segment in span.segments(cuts) {
                let inputs = segment.input.map(clamp);
                let mid = inputs[0].midpoint(inputs[1]);
                let index = bounds[1..bounds.len() - 1].partition_point(|&boundary| boundary <= mid);
                let [from, to] = [bounds[index], bounds[index + 1]];
                let ends = [encode[index * 2], encode[index * 2 + 1]];
                let input = inputs.map(|x| interpolate(ends, ((x - from) / (to - from)).clamp(0.0, 1.0)));
                sample_function(pdf, &sections[index], SampleSpan { input, ..segment }, range, output, depth + 1)?;
            }
        }
        4 => {
            let stream = deref(pdf, object).as_stream().ok()?;
            calculator::append_samples(stream, [low, high], span, range, output)?;
        }
        _ => return None,
    }
    Some(())
}

fn shading_samples(pdf: &PdfDocument, function: &PdfObject, span: SampleSpan, components: usize) -> Option<FunctionSamples> {
    let mut result = FunctionSamples::new(components);
    let PdfObject::Array(functions) = deref(pdf, function) else {
        sample_function(pdf, function, span, [[0.0, 1.0]; 4], &mut result, 0)?;
        return Some(result);
    };
    if functions.len() != components { return None; }
    let mut channels = Vec::new();
    for function in functions {
        let mut channel = FunctionSamples::new(1);
        sample_function(pdf, function, span, [[0.0, 1.0]; 4], &mut channel, 0)?;
        result.limited |= channel.limited;
        channels.push(channel.stops);
    }
    let mut offsets: Vec<_> = channels.iter().flatten().map(|&(offset, _)| offset).collect();
    offsets.sort_by(f32::total_cmp);
    offsets.dedup();
    for offset in offsets {
        let mut left = [0.0; 4];
        let mut right = [0.0; 4];
        for (i, stops) in channels.iter().enumerate() {
            let start = stops.partition_point(|s| s.0 < offset);
            let end = stops.partition_point(|s| s.0 <= offset);
            if start < end {
                left[i] = stops[start].1[0];
                right[i] = stops[end - 1].1[0];
            } else {
                let (lo, a) = stops[start.checked_sub(1)?];
                let &(hi, b) = stops.get(end)?;
                let fraction = f64::from(offset - lo) / f64::from(hi - lo);
                left[i] = interpolate([a[0], b[0]], fraction);
                right[i] = left[i];
            }
        }
        result.push(f64::from(offset), left)?;
        result.push(f64::from(offset), right)?;
    }
    Some(result)
}

fn clip_half_plane(points: &[Point], signed_distance: impl Fn(Point) -> f64) -> Vec<Point> {
    let Some(&last) = points.last() else { return Vec::new(); };
    let mut output = Vec::new();
    let mut previous = last;
    let mut distance = signed_distance(last);
    for &point in points {
        let next = signed_distance(point);
        if (distance >= 0.0) != (next >= 0.0) {
            output.push(previous.lerp(point, distance / (distance - next)));
        }
        if next >= 0.0 { output.push(point); }
        previous = point;
        distance = next;
    }
    output
}

fn extend_flags(pdf: &PdfDocument, dict: &Dictionary) -> Option<[bool; 2]> {
    let Ok(values) = dict.get(b"Extend") else { return Some([false, false]); };
    let flags = deref(pdf, values).as_array().ok()?;
    let [start, end] = flags.as_slice() else { return None; };
    Some([deref(pdf, start).as_bool().ok()?, deref(pdf, end).as_bool().ok()?])
}

impl PdfMapper<'_> {
    pub(super) fn shade(&mut self, name: &[u8], resources: &[Dictionary], gs: &GState) {
        let dict = match self.lookup(resources, b"Shading", name) {
            Some(PdfObject::Dictionary(dict)) => dict,
            Some(PdfObject::Stream(stream)) => stream.dict,
            _ => {
                self.loss(LossKind::Gradient, "shading resource missing");
                return;
            }
        };
        let Ok(space) = dict.get(b"ColorSpace") else { return; };
        let space = self.space_of(space, resources);
        let [x0, y0, x1, y1] = gs.clip.unwrap_or_else(|| {
            let [x, y] = self.origin.map(f64::from);
            let [w, h] = self.geometry.size().map(f64::from);
            [x, y, x + w, y + h]
        });
        if x1 <= x0 || y1 <= y0 { return; }
        let polygon = vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
        let shading_type = dict.get(b"ShadingType").ok().and_then(|value| deref(self.pdf, value).as_i64().ok());
        if shading_type == Some(3) && self.radial_shading(&dict, gs, space, &polygon).is_some() {
            return;
        }
        let (fill, polygon, rect) = self.axial_shading(&dict, gs, space, &polygon).unwrap_or_else(|| {
            self.loss(LossKind::Gradient, "shading approximated as a solid fill");
            let values = dict.get(b"Function").ok().and_then(|function| dict_of(self.pdf, function))
                .and_then(|function| function.get(b"C0").ok()).and_then(|c0| numbers(self.pdf, c0))
                .filter(|values| !values.is_empty()).unwrap_or_else(|| vec![0.5; 4]);
            (self.colour_from(space, &values), polygon, true)
        });
        if polygon.len() < 3 { return; }
        let mut shade = gs.clone();
        shade.fill = fill;
        let path = self.shade_polygon(&polygon, rect);
        self.emit_path(&[path], &shade, true, false, false, None);
    }

    fn shade_polygon(&mut self, polygon: &[Point], rect: bool) -> SubPath {
        let points = polygon.iter().map(|p| PathPoint::sharp(self.next_point(), [p.x, p.y])).collect();
        SubPath { points, closed: true, rect }
    }

    fn shading_stops(&mut self, dict: &Dictionary, space: Space) -> Option<Vec<GradientStop>> {
        let pdf = self.pdf;
        let domain = if dict.has(b"Domain") { interval(pdf, dict, b"Domain")? } else { [0.0, 1.0] };
        if domain[1] <= domain[0] { return None; }
        let function = dict.get(b"Function").ok()?;
        let components = space.channels()?;
        let span = SampleSpan { offsets: [0.0, 1.0], input: domain };
        let samples = shading_samples(pdf, function, span, components)?;
        if samples.limited {
            self.report.add_loss(LossKind::Gradient, 1, false, "steep shading limited by gradient stop precision");
        }
        if let Some(conversion) = self.profiles.conversion(space) {
            let (stops, limited) = super::icc::gradient_stops(conversion, &samples.stops)?;
            if limited {
                self.report.add_loss(LossKind::Gradient, 1, false, "ICC gradient limited by gradient stop count or precision");
            }
            return Some(stops);
        }
        samples.stops.into_iter()
            .map(|(offset, values)| match self.colour_from(space, &values[..components]) {
                Paint::Solid { colour } => Some(GradientStop { offset, colour }),
                _ => None,
            }).collect()
    }

    fn clip_shading_bbox(&self, dict: &Dictionary, gs: &GState, mut polygon: Vec<Point>) -> Option<Vec<Point>> {
        if let Ok(bbox) = dict.get(b"BBox") {
            let [left, bottom, right, top] = super::rect_of(self.pdf, bbox)?;
            if right <= left || top <= bottom { return Some(Vec::new()); }
            let corners = [[left, bottom], [right, bottom], [right, top], [left, top]]
                .map(|xy| {
                    let [x, y] = self.user_to_doc(gs, xy);
                    Point::new(x, y)
                });
            let orientation = (corners[1] - corners[0]).cross(corners[3] - corners[0]).signum();
            for (&a, &b) in corners.iter().zip(corners.iter().cycle().skip(1)) {
                polygon = clip_half_plane(&polygon, |p| (b - a).cross(p - a) * orientation);
            }
        }
        Some(polygon)
    }

    fn axial_shading(&mut self, dict: &Dictionary, gs: &GState, space: Space, polygon: &[Point]) -> Option<(Paint, Vec<Point>, bool)> {
        let pdf = self.pdf;
        if deref(pdf, dict.get(b"ShadingType").ok()?).as_i64().ok()? != 2 { return None; }
        let [x0, y0, x1, y1]: [f64; 4] = numbers(pdf, dict.get(b"Coords").ok()?)?.try_into().ok()?;
        let [extend_start, extend_end] = extend_flags(pdf, dict)?;
        let stops = self.shading_stops(dict, space)?;
        let to_doc = |xy| {
            let [x, y] = self.user_to_doc(gs, xy);
            Point::new(x, y)
        };
        let first = to_doc([x0, y0]);
        let along = to_doc([x1, y1]) - first;
        let across = to_doc([x0 - (y1 - y0), y0 + (x1 - x0)]) - first;
        // The gradient measures distance perpendicular to the transformed colour bands.
        let normal = Vec2::new(-across.y, across.x);
        let axis = normal * (along.dot(normal) / normal.hypot2());
        let length_squared = axis.hypot2();
        if !length_squared.is_finite() || length_squared <= 1e-16 { return None; }
        let position = |point: Point| (point - first).dot(axis) / length_squared;
        let mut polygon = self.clip_shading_bbox(dict, gs, polygon.to_vec())?;
        if !extend_start { polygon = clip_half_plane(&polygon, position); }
        if !extend_end { polygon = clip_half_plane(&polygon, |p| 1.0 - position(p)); }
        if polygon.len() < 3 { return Some((Paint::None, polygon, false)); }
        let origin = Point::new(polygon.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            polygon.iter().map(|p| p.y).fold(f64::INFINITY, f64::min));
        let start = first - origin;
        let end = start + axis;
        let geom = GradientGeom { start: [start.x, start.y], end: [end.x, end.y], ..Default::default() };
        Some((Paint::Gradient { kind: GradientKind::Linear, stops, geom }, polygon, !dict.has(b"BBox") && extend_start && extend_end))
    }

    fn radial_shading(&mut self, dict: &Dictionary, gs: &GState, space: Space, polygon: &[Point]) -> Option<()> {
        let [x0, y0, r0, x1, y1, r1]: [f64; 6] =
            numbers(self.pdf, dict.get(b"Coords").ok()?)?.try_into().ok()?;
        let [e0, e1] = extend_flags(self.pdf, dict)?;
        let mut stops = self.shading_stops(dict, space)?;
        let mut first = (Point::new(x0, y0), r0, e0);
        let mut last = (Point::new(x1, y1), r1, e1);
        if r0 > r1 {
            std::mem::swap(&mut first, &mut last);
            stops.reverse();
            for stop in &mut stops { stop.offset = 1.0 - stop.offset; }
        }
        let (focus, small_radius, extend_inner) = first;
        let (center, large_radius, extend_outer) = last;
        // One editable radial fill represents a strictly nested family of circles.
        if small_radius < 0.0 || (focus - center).hypot() >= large_radius - small_radius {
            return None;
        }
        let mut polygon = self.clip_shading_bbox(dict, gs, polygon.to_vec())?;

        let to_doc = |p: Point| {
            let [x, y] = self.user_to_doc(gs, [p.x, p.y]);
            Point::new(x, y)
        };
        let outer = to_doc(center);
        let inner = to_doc(focus);
        let u = to_doc(center + Vec2::new(large_radius, 0.0)) - outer;
        let v = to_doc(center + Vec2::new(0.0, large_radius)) - outer;
        let ellipse = Ellipse::from_affine(Affine::new([u.x, u.y, v.x, v.y, outer.x, outer.y]));
        let (radii, angle) = ellipse.radii_and_rotation();
        if !ellipse.is_finite() || !radii.is_finite() || radii.y <= 1e-8 { return None; }
        let bounds = ellipse.bounding_box();
        if bounds.x0.abs().max(bounds.x1.abs()).max(bounds.y0.abs()).max(bounds.y1.abs()) > f32::MAX as f64 {
            return None;
        }
        if !extend_outer {
            polygon = clip_half_plane(&polygon, |p| p.x - bounds.x0);
            polygon = clip_half_plane(&polygon, |p| bounds.x1 - p.x);
            polygon = clip_half_plane(&polygon, |p| p.y - bounds.y0);
            polygon = clip_half_plane(&polygon, |p| bounds.y1 - p.y);
        }
        if polygon.len() < 3 { return Some(()); }
        let ratio = (radii.y / radii.x).min(1.0);
        let kind = if (ratio - 1.0).abs() < 1e-6 { GradientKind::Radial } else { GradientKind::Elliptical };
        let path = self.shade_polygon(&polygon, !dict.has(b"BBox"));
        let [left, top, right, bottom] = Self::path_bounds(std::slice::from_ref(&path));
        if right <= left || bottom <= top { return Some(()); }
        let start = outer - Point::new(left, top);
        let axis = Vec2::from_angle(angle);
        let end = start + axis * radii.x;
        let delta = inner - outer;
        let normalized_focus = [delta.dot(axis) / radii.x, axis.cross(delta) / radii.y];
        let focus_radius = small_radius / large_radius;
        if normalized_focus[0].hypot(normalized_focus[1]) + focus_radius >= 1.0 { return None; }
        let geom = GradientGeom {
            start: [start.x, start.y], end: [end.x, end.y], ratio,
            focus: (inner != outer).then_some(normalized_focus), focus_radius,
            ..Default::default()
        };
        let mut shade = gs.clone();
        if !extend_outer || (!extend_inner && small_radius > 0.0) {
            let mut mask = if extend_outer {
                // Enclose the entire inner contour so even-odd fill only removes coverage.
                let rect = [
                    Point::new(left.min(bounds.x0), top.min(bounds.y0)),
                    Point::new(right.max(bounds.x1), top.min(bounds.y0)),
                    Point::new(right.max(bounds.x1), bottom.max(bounds.y1)),
                    Point::new(left.min(bounds.x0), bottom.max(bounds.y1)),
                ];
                vec![self.shade_polygon(&rect, true)]
            } else {
                vec![self.shade_ellipse(outer, u, v)]
            };
            if !extend_inner && small_radius > 0.0 {
                mask.push(self.shade_ellipse(inner, u * focus_radius, v * focus_radius));
            }
            let [x0, y0, x1, y1] = Self::path_bounds(&mask);
            let parent = self.doc.create_object(Object {
                name: "radial shading".into(), kind: ObjectKind::Group,
                pos: [left as f32, top as f32],
                size: [(right - left) as f32, (bottom - top) as f32],
                parent_id: gs.parent,
                blend_mode: if gs.blend_mode.is_normal() { BlendMode::Normal } else { BlendMode::PassThrough },
                ..Object::default()
            });
            self.doc.create_object(Object {
                name: "shading limits".into(),
                kind: ObjectKind::Curve(Self::curve_from_path(&mask, [x0, y0], true)),
                pos: [x0 as f32, y0 as f32], size: [(x1 - x0) as f32, (y1 - y0) as f32],
                parent_id: Some(parent), clip_role: ClipRole::Mask,
                fill: Paint::Solid { colour: Colour::new(255, 255, 255, 255) },
                fill_rule: dtool_core::boolean::FillRule::EvenOdd,
                ..Object::default()
            });
            shade.parent = Some(parent);
        }
        shade.fill = Paint::Gradient { kind, stops, geom };
        self.emit_path(&[path], &shade, true, false, false, None);
        Some(())
    }

    fn shade_ellipse(&mut self, center: Point, u: Vec2, v: Vec2) -> SubPath {
        const HANDLE: f64 = 0.552_284_749_830_793_6;
        let points = [(u, v), (v, -u), (-u, -v), (-v, u)].map(|(axis, tangent)| {
            let p = center + axis;
            let incoming = p - tangent * HANDLE;
            let outgoing = p + tangent * HANDLE;
            PathPoint {
                handle_in: Some([incoming.x, incoming.y]),
                handle_out: Some([outgoing.x, outgoing.y]),
                kind: dtool_core::curve::NodeKind::Smooth,
                ..PathPoint::sharp(self.next_point(), [p.x, p.y])
            }
        }).to_vec();
        SubPath { points, closed: true, rect: false }
    }
}
