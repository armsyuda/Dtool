use crate::command::Session;
use crate::document::Document;
use crate::object::{Object, ObjectId, ObjectKind};
use dtool_protocol::{CommandError, CommandResult};
use kurbo::{Affine, BezPath, ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point, Shape, Vec2};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const QUERY: &str = "area.query";
const LENGTH_ACCURACY: f64 = 1e-4;

#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub area: f64,
    pub perimeter: f64,
    pub hole_perimeter: f64,
}

impl Metrics {
    pub fn add(&mut self, other: Self) {
        self.area += other.area;
        self.perimeter += other.perimeter;
        self.hole_perimeter += other.hole_perimeter;
    }
}

#[derive(Clone, Debug)]
pub struct Geometry {
    spans: Vec<BezPath>,
    pub metrics: Metrics,
    pub outlines: Vec<(Vec<[f32; 2]>, bool)>,
}

impl Geometry {
    pub fn for_object(object: &Object) -> Option<Self> {
        let local = crate::outline::object_outline(object)?;
        let matrix = crate::geometry::object_matrix(object).map(f64::from);
        if matrix.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let [x, y] = object.pos.map(f64::from);
        let path = Affine::new(matrix) * Affine::translate((x, y)) * local;
        if !path.is_finite() {
            return None;
        }
        let mut metrics = Metrics::default();
        let mut closed = Vec::new();
        let mut spans = Vec::new();
        for elements in path.subpaths() {
            let contour = BezPath::from_vec(elements.to_vec());
            let is_closed = elements.last() == Some(&PathEl::ClosePath);
            split_at_cusps(&contour, is_closed, &mut spans);
            if is_closed {
                closed.push(contour);
            } else {
                metrics.perimeter += contour.perimeter(LENGTH_ACCURACY);
            }
        }
        match closed.as_slice() {
            [] => {}
            [contour] if matches!(object.kind, ObjectKind::Shape) => {
                metrics.area = contour.area().abs();
                metrics.perimeter += contour.perimeter(LENGTH_ACCURACY);
            }
            contours => {
                let (area, perimeter, hole_perimeter) =
                    crate::boolean::closed_outline_metrics(contours, object.fill_rule, 0.01).ok()?;
                metrics.area = area;
                metrics.perimeter += perimeter;
                metrics.hole_perimeter = hole_perimeter;
            }
        }
        if metrics.perimeter <= 0.0 || !metrics.perimeter.is_finite() {
            return None;
        }
        let outlines = crate::outline::flatten_subpaths(&path, crate::outline::OUTLINE_TOLERANCE)
            .into_iter()
            .map(|(points, closed)| {
                (
                    points.into_iter().map(|p| p.map(|v| v as f32)).collect(),
                    closed,
                )
            })
            .collect();
        Some(Self {
            spans,
            metrics,
            outlines,
        })
    }

    pub fn segment(&self, point: [f32; 2], tolerance: f32) -> Option<(f64, Vec<[f32; 2]>)> {
        let [x, y] = point.map(f64::from);
        let point = Point::new(x, y);
        let limit = f64::from(tolerance).powi(2);
        let (_, path) = self.spans.iter()
            .filter_map(|path| {
                let distance = path.segments().map(|segment| segment.nearest(point, LENGTH_ACCURACY).distance_sq)
                    .min_by(f64::total_cmp)?;
                (distance <= limit).then_some((distance, path))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        let (points, _) = crate::outline::flatten_subpaths(path, crate::outline::OUTLINE_TOLERANCE).into_iter().next()?;
        Some((path.perimeter(LENGTH_ACCURACY), points.into_iter().map(|p| p.map(|v| v as f32)).collect()))
    }
}

fn endpoint_tangents(segment: PathSeg) -> Option<(Vec2, Vec2)> {
    let points = match segment {
        PathSeg::Line(line) => [line.p0, line.p0, line.p1, line.p1],
        PathSeg::Quad(curve) => [curve.p0, curve.p1, curve.p1, curve.p2],
        PathSeg::Cubic(curve) => [curve.p0, curve.p1, curve.p2, curve.p3],
    };
    let first = *points.first()?;
    let last = *points.last()?;
    let first_control = points.iter().find(|&&p| p != first)?;
    let last_control = points.iter().rev().find(|&&p| p != last)?;
    Some(((*first_control - first).normalize(), (last - *last_control).normalize()))
}

fn split_at_cusps(contour: &BezPath, closed: bool, spans: &mut Vec<BezPath>) {
    let mut segments: Vec<_> = contour.segments()
        .filter_map(|segment| Some((segment, endpoint_tangents(segment)?))).collect();
    if segments.is_empty() {
        return;
    }
    let cusp_before = |segments: &[(PathSeg, (Vec2, Vec2))], i: usize| {
        let before = (i + segments.len() - 1) % segments.len();
        segments[before].1.1.dot(segments[i].1.0) < 0.99999
    };
    if closed && let Some(first) = (0..segments.len()).find(|&i| cusp_before(&segments, i)) {
        segments.rotate_left(first);
    }
    let mut path = BezPath::new();
    for (i, (segment, _)) in segments.iter().enumerate() {
        if i > 0 && cusp_before(&segments, i) {
            spans.push(std::mem::take(&mut path));
        }
        if path.is_empty() {
            path.move_to(segment.start());
        }
        path.push(segment.as_path_el());
    }
    spans.push(path);
}

pub fn units(doc: &Document) -> (&'static str, f64) {
    units_for_objects(doc, &[])
}

pub fn units_for_objects(doc: &Document, ids: &[ObjectId]) -> (&'static str, f64) {
    let points = ids.iter().filter_map(|&id| doc.get_object(id)).map(|object| {
        let [x, y, w, h] = crate::geometry::object_world_bounds(object).map(f64::from);
        [x + w * 0.5, y + h * 0.5]
    });
    let (unit, scale) = super::measurement_basis(doc, points);
    let (units, factor) = unit.from_document(doc.units, doc.dpi);
    let [drawing, real] = scale.map(f64::from);
    (units, factor * real / drawing)
}

pub fn totals(doc: &Document, ids: &[ObjectId]) -> (usize, Metrics) {
    let mut count = 0;
    let mut metrics = Metrics::default();
    let mut seen = HashSet::new();
    for id in ids.iter().filter(|id| seen.insert(**id)) {
        if let Some(geometry) = doc.get_object(*id).and_then(Geometry::for_object) {
            count += 1;
            metrics.add(geometry.metrics);
        }
    }
    (count, metrics)
}

pub(super) fn query(session: &Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let ids = match params.get("ids") {
        None => session.selection.ids.clone(),
        Some(ids) => serde_json::from_value::<Vec<ObjectId>>(ids.clone())
            .map_err(|_| CommandError::invalid_params())?,
    };
    let (count, metrics) = totals(&session.document, &ids);
    let (units, factor) = units_for_objects(&session.document, &ids);
    Ok(json!({
        "count": count, "units": units, "area": metrics.area * factor.powi(2),
        "perimeter": metrics.perimeter * factor,
        "perimeter_including_holes": (metrics.perimeter + metrics.hole_perimeter) * factor,
    }))
}
