use crate::command::Session;
use crate::document::Document;
use crate::object::{Object, ObjectId, ObjectKind};
use dtool_protocol::{CommandError, CommandResult};
use kurbo::{Affine, BezPath, ParamCurveArclen, ParamCurveNearest, PathEl, PathSeg, Point, Shape};
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
    path: BezPath,
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
        let mut metrics = Metrics::default();
        let mut closed = Vec::new();
        for elements in path.subpaths() {
            let contour = BezPath::from_vec(elements.to_vec());
            if elements.last() == Some(&PathEl::ClosePath) {
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
                    crate::boolean::closed_outline_metrics(contours, 0.01).ok()?;
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
            path,
            metrics,
            outlines,
        })
    }

    pub fn segment(&self, point: [f32; 2], tolerance: f32) -> Option<(f64, [[f32; 2]; 2])> {
        let [x, y] = point.map(f64::from);
        let point = Point::new(x, y);
        let edge = self
            .path
            .segments()
            .filter_map(|segment| match segment {
                PathSeg::Line(line) if line.p0 != line.p1 => {
                    let distance = segment.nearest(point, LENGTH_ACCURACY).distance_sq;
                    (distance <= f64::from(tolerance).powi(2)).then_some((distance, line))
                }
                _ => None,
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))?
            .1;
        Some((
            edge.arclen(LENGTH_ACCURACY),
            [
                [edge.p0.x as f32, edge.p0.y as f32],
                [edge.p1.x as f32, edge.p1.y as f32],
            ],
        ))
    }
}

pub fn units(doc: &Document) -> (&'static str, f64) {
    let (units, factor) = doc.measurement_unit.from_document(doc.units, doc.dpi);
    let [drawing, real] = doc.drawing_scale.map(f64::from);
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
    let (units, factor) = units(&session.document);
    Ok(json!({
        "count": count, "units": units, "area": metrics.area * factor.powi(2),
        "perimeter": metrics.perimeter * factor,
        "perimeter_including_holes": (metrics.perimeter + metrics.hole_perimeter) * factor,
    }))
}
