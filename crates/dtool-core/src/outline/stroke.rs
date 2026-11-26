use crate::boolean::{DEFAULT_TOLERANCE, MAX_BOOLEAN_POINTS};
use crate::paint::{Cap, Join, StrokeStyle};
use kurbo::{BezPath, Point, Vec2};
use std::ops::Range;

#[derive(Clone, Copy)]
struct Sample {
    point: Point,
    radius: f64,
}

/// Expand pressure along the original centreline before output transforms are applied.
pub fn pressure_stroke_path(path: &BezPath, style: &StrokeStyle, width: f64) -> Option<BezPath> {
    pressure_stroke_path_with_tolerance(path, style, width, DEFAULT_TOLERANCE)
}

fn pressure_stroke_path_with_tolerance(path: &BezPath, style: &StrokeStyle, width: f64, tolerance: f64) -> Option<BezPath> {
    if !(width.is_finite() && width > 0.0 && path.is_finite() && style.dash_offset.is_finite())
        || style
            .pressure
            .iter()
            .any(|[x, y]| !(0.0..=1.0).contains(x) || !y.is_finite() || *y < 0.0)
        || style
            .pressure
            .windows(2)
            .any(|pair| pair[0][0] > pair[1][0])
        || style
            .dash
            .iter()
            .any(|dash| !dash.is_finite() || *dash < 0.0)
    {
        return None;
    }
    let mut outline = BezPath::new();
    let mut budget = MAX_BOOLEAN_POINTS;
    for (points, closed) in super::flatten_subpaths(path, tolerance) {
        let mut points: Vec<_> = points.into_iter().map(|[x, y]| Point::new(x, y)).collect();
        points.dedup();
        if points.len() < 2 {
            continue;
        }
        if closed && points.first() != points.last() {
            points.push(points[0]);
        }
        budget = budget.checked_sub(points.len())?;
        let distance = |pair: &[Point]| (pair[1] - pair[0]).hypot();
        let distances: Vec<_> = std::iter::once(0.0)
            .chain(points.windows(2).scan(0.0, |length, pair| {
                *length += distance(pair);
                Some(*length)
            }))
            .collect();
        let length = *distances.last()?;
        if !(length.is_finite() && length > 0.0) {
            continue;
        }
        let mut stops = distances.clone();
        stops.extend(style.pressure.iter().map(|[x, _]| f64::from(*x) * length));
        stops.sort_unstable_by(f64::total_cmp);
        stops.dedup();
        let runs = dash_runs(length, &style.dash, f64::from(style.dash_offset), budget)?;
        let closed_run = closed && style.dash.iter().all(|length| *length == 0.0);
        for mut range in runs {
            let mut caps = None;
            // Move the round cap centres inwards, keeping the visible tips on the path endpoints.
            if style.cap == Cap::RoundInset && !closed_run {
                let half = (range.end - range.start) * 0.5;
                let start = (width * 0.5 * pressure(&style.pressure, range.start / length)).min(half);
                let end = (width * 0.5 * pressure(&style.pressure, range.end / length)).min(half);
                let trim = |radius: f64| radius.min(half * (1.0 - 1e-7));
                range.start += trim(start);
                range.end -= trim(end);
                caps = Some([trim(start), trim(end)]);
            }
            let first = stops.partition_point(|stop| *stop <= range.start);
            let last = stops.partition_point(|stop| *stop < range.end);
            let sample = |at: f64| {
                let segment = distances
                    .partition_point(|distance| *distance <= at)
                    .clamp(1, points.len() - 1);
                let t =
                    (at - distances[segment - 1]) / (distances[segment] - distances[segment - 1]);
                let point = points[segment - 1].lerp(points[segment], t);
                Sample {
                    point,
                    radius: width * 0.5 * pressure(&style.pressure, at / length),
                }
            };
            if range.is_empty() {
                let dot = sample(range.start);
                let before = distances.partition_point(|stop| *stop < range.start).clamp(1, points.len() - 1);
                let after = distances.partition_point(|stop| *stop <= range.start).clamp(1, points.len() - 1);
                let incoming = (points[before] - points[before - 1]).normalize();
                let outgoing = (points[after] - points[after - 1]).normalize();
                if style.cap == Cap::Round && before == after && dot.radius > 0.0 {
                    let Point { x, y } = dot.point;
                    outline.extend(crate::geometry::ellipse_to_bez_path(x, y, dot.radius, dot.radius));
                } else if style.cap != Cap::RoundInset {
                    cap(&mut outline, dot, -incoming, style.cap);
                    cap(&mut outline, dot, outgoing, style.cap);
                }
                budget = budget.checked_sub(1)?;
                if outline.elements().len() > MAX_BOOLEAN_POINTS { return None; }
                continue;
            }
            let mut samples: Vec<_> = std::iter::once(range.start)
                .chain(stops[first..last].iter().copied())
                .chain(std::iter::once(range.end))
                .map(sample)
                .collect();
            if let Some([start, end]) = caps {
                samples.first_mut()?.radius = start;
                samples.last_mut()?.radius = end;
            }
            budget = budget.checked_sub(samples.len())?;
            append_run(
                &mut outline,
                &samples,
                closed_run,
                style,
            );
            if outline.elements().len() > MAX_BOOLEAN_POINTS {
                return None;
            }
        }
    }
    Some(outline)
}

fn pressure(profile: &[[f32; 2]], at: f64) -> f64 {
    if profile.is_empty() { return 1.0; }
    let upper = profile.partition_point(|point| f64::from(point[0]) <= at);
    if upper == 0 {
        return f64::from(profile[0][1]);
    }
    let [from, weight] = profile[upper - 1].map(f64::from);
    let Some([to, next]) = profile.get(upper).map(|point| point.map(f64::from)) else {
        return weight;
    };
    let offset = (at - from) / (to - from);
    weight + (next - weight) * offset
}

fn dash_runs(length: f64, dash: &[f32], offset: f64, budget: usize) -> Option<Vec<Range<f64>>> {
    let sum = dash.iter().copied().map(f64::from).sum::<f64>();
    if sum == 0.0 {
        return Some(std::iter::once(0.0..length).collect());
    }
    let mut runs = Vec::new();
    let period = if dash.len() % 2 == 0 { sum } else { sum * 2.0 };
    let mut phase = offset.rem_euclid(period);
    let mut pattern = dash.iter().copied().map(f64::from).cycle().enumerate().peekable();
    while let Some(&(_, step)) = pattern.peek() {
        if phase <= step {
            break;
        }
        phase -= step;
        pattern.next();
    }
    let mut start = 0.0;
    for (index, step) in pattern.take(budget) {
        let end = (start + (step - phase)).min(length);
        phase = 0.0;
        if index % 2 == 0 {
            runs.push(start..end);
        }
        if end >= length {
            return Some(runs);
        }
        start = end;
    }
    None
}

fn normal(tangent: Vec2) -> Vec2 {
    Vec2::new(-tangent.y, tangent.x)
}

fn append_run(path: &mut BezPath, samples: &[Sample], closed: bool, style: &StrokeStyle) {
    let edge = |pair: &[Sample]| (pair[1].point - pair[0].point).normalize();
    let directions: Vec<_> = samples.windows(2).map(edge).collect();
    for (pair, &direction) in samples.windows(2).zip(&directions) {
        let (a, b) = (pair[0], pair[1]);
        let left = normal(direction);
        polygon(
            path,
            &[
                a.point + left * a.radius,
                a.point - left * a.radius,
                b.point - left * b.radius,
                b.point + left * b.radius,
            ],
        );
    }
    for (index, &sample) in samples.iter().enumerate().take(samples.len() - 1).skip(1) {
        join(
            path,
            sample,
            directions[index - 1],
            directions[index],
            &style.join,
        );
    }
    let first = samples[0];
    let last = samples[samples.len() - 1];
    let start = directions[0];
    let end = directions[directions.len() - 1];
    if closed {
        join(
            path,
            Sample {
                radius: first.radius.min(last.radius),
                ..first
            },
            end,
            start,
            &style.join,
        );
    } else {
        cap(path, first, -start, style.cap);
        cap(path, last, end, style.cap);
    }
}

fn join(path: &mut BezPath, sample: Sample, incoming: Vec2, outgoing: Vec2, style: &Join) {
    let turn = incoming.cross(outgoing);
    if sample.radius <= 0.0 || turn.abs() < 1e-9 {
        return;
    }
    let outside = -turn.signum() * sample.radius;
    let a = normal(incoming) * outside;
    let b = normal(outgoing) * outside;
    if matches!(style, Join::Round) {
        sector(path, sample.point, a, turn.atan2(incoming.dot(outgoing)));
        return;
    }
    if let Join::Miter { limit } = style {
        let at = a + incoming * ((b - a).cross(outgoing) / turn);
        if at.hypot() <= sample.radius * f64::from(*limit) {
            polygon(
                path,
                &[
                    sample.point,
                    sample.point + a,
                    sample.point + at,
                    sample.point + b,
                ],
            );
            return;
        }
    }
    polygon(path, &[sample.point, sample.point + a, sample.point + b]);
}

fn cap(path: &mut BezPath, sample: Sample, outward: Vec2, style: Cap) {
    if sample.radius <= 0.0 {
        return;
    }
    let centre = sample.point;
    let across = normal(outward) * sample.radius;
    match style {
        Cap::Butt => {}
        Cap::Square => {
            let extent = outward * sample.radius;
            polygon(
                path,
                &[
                    centre + across,
                    centre - across,
                    centre - across + extent,
                    centre + across + extent,
                ],
            );
        }
        Cap::Round | Cap::RoundInset => sector(path, centre, -across, std::f64::consts::PI),
    }
}

fn sector(path: &mut BezPath, centre: Point, from: Vec2, sweep: f64) {
    let steps = (sweep.abs() / 0.1).ceil() as usize;
    let mut points = Vec::with_capacity(steps + 2);
    points.push(centre);
    for step in 0..=steps {
        let (sin, cos) = (sweep * step as f64 / steps as f64).sin_cos();
        points.push(centre + Vec2::new(from.x * cos - from.y * sin, from.x * sin + from.y * cos));
    }
    polygon(path, &points);
}

fn polygon(path: &mut BezPath, points: &[Point]) {
    let origin = points[0];
    let area: f64 = points[1..]
        .windows(2)
        .map(|pair| (pair[0] - origin).cross(pair[1] - origin))
        .sum();
    if !area.is_finite() || area.abs() <= 1e-12 {
        return;
    }
    path.move_to(origin);
    if area > 0.0 {
        for &point in &points[1..] {
            path.line_to(point);
        }
    } else {
        for &point in points[1..].iter().rev() {
            path.line_to(point);
        }
    }
    path.close_path();
}

/// Expand a styled world-space centerline. The flag selects even-odd filling.
pub fn styled_stroke_path(path: &kurbo::BezPath, style: &crate::paint::StrokeStyle) -> Option<(kurbo::BezPath, bool)> {
    styled_stroke_path_with_tolerance(path, style, DEFAULT_TOLERANCE)
}

pub fn styled_stroke_path_with_tolerance(path: &BezPath, style: &StrokeStyle, tolerance: f64) -> Option<(BezPath, bool)> {
    use crate::paint::{Cap, Join, StrokeAlign};
    use kurbo::ParamCurveArclen;
    if !(style.width.is_finite() && style.width > 0.0 && style.dash_offset.is_finite())
        || style.dash.iter().any(|dash| !dash.is_finite() || *dash < 0.0)
    {
        return None;
    }
    let cap = match style.cap { Cap::Butt => kurbo::Cap::Butt, Cap::Round | Cap::RoundInset => kurbo::Cap::Round, Cap::Square => kurbo::Cap::Square };
    let (join, limit) = match style.join {
        Join::Miter { limit } => (kurbo::Join::Miter, f64::from(limit)),
        Join::Round => (kurbo::Join::Round, 4.0),
        Join::Bevel => (kurbo::Join::Bevel, 4.0),
    };
    if !limit.is_finite() || limit < 1.0 || !path.is_finite() { return None; }
    let aligned = style.align != StrokeAlign::Centre
        && path.elements().iter().any(|element| matches!(element, kurbo::PathEl::ClosePath));
    let width = f64::from(style.width) * if aligned { 2.0 } else { 1.0 };
    let pattern = style.dash.iter().copied().map(f64::from);
    let shortest = pattern.clone().filter(|step| *step > 0.0).fold(f64::INFINITY, f64::min);
    if path.segments().map(|segment| segment.arclen(tolerance)).sum::<f64>() / shortest
        > crate::boolean::MAX_BOOLEAN_POINTS as f64 / 2.0
    {
        return None;
    }
    let outline = if style.pressure.is_empty() && style.cap != Cap::RoundInset && !style.dash.contains(&0.0) {
        let stroke = kurbo::Stroke::new(width).with_caps(cap).with_join(join).with_miter_limit(limit).with_dashes(f64::from(style.dash_offset), pattern);
        let outline = kurbo::stroke(path.elements().iter().copied(), &stroke, &Default::default(), tolerance);
        if cap == kurbo::Cap::Round {
            close_stroke_subpaths(&outline)
        } else {
            outline
        }
    } else {
        pressure_stroke_path_with_tolerance(path, style, width, tolerance)?
    };
    if outline.elements().is_empty() { return Some((outline, false)); }
    let outline = if aligned {
        let op = match style.align {
            StrokeAlign::Inside => crate::boolean::BoolOp::Intersect,
            StrokeAlign::Outside => crate::boolean::BoolOp::Subtract,
            StrokeAlign::Centre => unreachable!(),
        };
        let Ok(curves) = crate::boolean::boolean(
            &[outline], std::slice::from_ref(path), op, crate::boolean::FillRule::NonZero, tolerance,
        ) else { return None; };
        curves.into_iter().flat_map(|curve| curve.to_kurbo()).collect()
    } else {
        outline
    };
    Some((outline, aligned))
}

fn close_stroke_subpaths(path: &kurbo::BezPath) -> kurbo::BezPath {
    use kurbo::PathEl;
    // Round caps return to the start of the outline without an explicit ClosePath.
    let mut outline = kurbo::BezPath::new();
    let mut open = false;
    for &element in path.elements() {
        match element {
            PathEl::MoveTo(_) => {
                if open {
                    outline.close_path();
                }
                open = true;
            }
            PathEl::ClosePath => open = false,
            _ => {}
        }
        outline.push(element);
    }
    if open {
        outline.close_path();
    }
    outline
}
