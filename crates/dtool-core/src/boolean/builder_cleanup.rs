use dtool_protocol::CommandError;
use kurbo::{BezPath, Line, ParamCurve, Point, Rect, Shape as _};
use serde_json::Value;

use super::{request_too_large, shape_contains, Shape};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuilderCleanup {
    None,
    Internal,
    Connected,
    Unused,
}

impl BuilderCleanup {
    pub(super) fn parse(params: &Value) -> Result<Self, CommandError> {
        match params
            .get("cleanup")
            .and_then(Value::as_str)
            .unwrap_or("none")
        {
            "none" => Ok(Self::None),
            "internal" => Ok(Self::Internal),
            "connected" => Ok(Self::Connected),
            "unused" => Ok(Self::Unused),
            _ => Err(CommandError::invalid_params()),
        }
    }
}

pub(super) struct BuilderBoundary<'a> {
    shapes: &'a [Shape],
    edges: Vec<(Rect, Line)>,
    remaining_work: usize,
}

impl<'a> BuilderBoundary<'a> {
    pub(super) fn new(shapes: &'a [Shape], cleanup: BuilderCleanup) -> Self {
        let edges = shapes
            .iter()
            .filter(|_| {
                matches!(
                    cleanup,
                    BuilderCleanup::Internal | BuilderCleanup::Connected
                )
            })
            .flatten()
            .flat_map(|ring| {
                ring.iter()
                    .zip(ring.iter().cycle().skip(1))
                    .map(|(&a, &b)| {
                        let edge = Line::new(Point::new(a[0], a[1]), Point::new(b[0], b[1]));
                        (Rect::from_points(edge.p0, edge.p1), edge)
                    })
            })
            .collect();
        Self {
            shapes,
            edges,
            remaining_work: 8_000_000,
        }
    }

    pub(super) fn cleanup(
        &mut self,
        path: &BezPath,
        cleanup: BuilderCleanup,
    ) -> Result<Option<Vec<BezPath>>, CommandError> {
        match cleanup {
            BuilderCleanup::None => return Ok(None),
            BuilderCleanup::Unused => return Ok(Some(Vec::new())),
            _ => {}
        }
        let bounds = path.bounding_box();
        let edges: Vec<Line> = self
            .edges
            .iter()
            .filter(|(rect, _)| touches(*rect, bounds))
            .map(|(_, edge)| *edge)
            .collect();
        self.reserve_work(path.elements().len().saturating_mul(edges.len()))?;
        let mut pieces = Vec::new();
        let mut current = BezPath::new();
        let mut changed = false;
        for segment in path.segments() {
            let bounds = segment.bounding_box();
            let mut cuts = vec![0.0, 1.0];
            for edge in edges
                .iter()
                .filter(|edge| touches(Rect::from_points(edge.p0, edge.p1), bounds))
            {
                cuts.extend(
                    segment
                        .intersect_line(*edge)
                        .into_iter()
                        .map(|hit| hit.segment_t.clamp(0.0, 1.0)),
                );
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            self.reserve_work(cuts.len().saturating_mul(self.edges.len()))?;
            for span in cuts.windows(2) {
                let [from, to] = [span[0], span[1]];
                let mid = segment.eval((from + to) * 0.5);
                let inside = self
                    .shapes
                    .iter()
                    .any(|shape| shape_contains(shape, [mid.x, mid.y]))
                    && edges.iter().all(|edge| !point_on_edge(mid, *edge));
                if inside {
                    changed = true;
                    if cleanup == BuilderCleanup::Connected {
                        return Ok(Some(Vec::new()));
                    }
                    if !current.is_empty() {
                        pieces.push(std::mem::take(&mut current));
                    }
                } else {
                    let part = segment.subsegment(from..to);
                    if current.is_empty() {
                        current.move_to(part.start());
                    }
                    current.push(part.as_path_el());
                }
            }
        }
        if !current.is_empty() {
            pieces.push(current);
        }
        Ok(changed.then_some(pieces))
    }

    fn reserve_work(&mut self, count: usize) -> Result<(), CommandError> {
        self.remaining_work = self
            .remaining_work
            .checked_sub(count)
            .ok_or_else(request_too_large)?;
        Ok(())
    }
}

fn touches(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && a.x1 >= b.x0 && a.y0 <= b.y1 && a.y1 >= b.y0
}

fn point_on_edge(point: Point, line: Line) -> bool {
    let axis = line.p1 - line.p0;
    let delta = point - line.p0;
    let along = delta.dot(axis);
    let length_sq = axis.hypot2();
    length_sq > 0.0
        && along >= 0.0
        && along <= length_sq
        && delta.cross(axis).abs() <= 1e-8 * length_sq.sqrt()
}
