use kurbo::{BezPath, ParamCurve, ParamCurveExtrema, ParamCurveNearest, PathEl, PathSeg, Point, Rect};

use super::Ring;

#[derive(Clone, Copy)]
struct Span {
    segment: usize,
    from: f64,
    to: f64,
}

pub(super) struct Edges {
    segments: Vec<(PathSeg, Rect)>,
    has_curves: bool,
    tolerance: f64,
}

impl Edges {
    pub(super) fn new<'a>(paths: impl IntoIterator<Item = &'a BezPath>, tolerance: f64) -> Self {
        let segments: Vec<_> = paths.into_iter().flat_map(BezPath::segments)
            .map(|segment| (segment, segment.bounding_box()))
            .collect();
        let has_curves = segments.iter().any(|(segment, _)| !matches!(segment, PathSeg::Line(_)));
        Self { segments, has_curves, tolerance }
    }

    // The overlay determines connectivity. For an edge on a source Bézier,
    // recover its parameter interval and rejoin consecutive intervals.
    pub(super) fn restore(&self, shape: &[Ring], work: &mut usize) -> BezPath {
        let mut path = BezPath::new();
        for ring in shape {
            let points: Vec<_> = ring.iter().map(|p| Point::new(p[0], p[1])).collect();
            let Some(&first) = points.first() else {continue};
            let spans: Vec<_> = points.iter().zip(points.iter().cycle().skip(1))
                .map(|(&a, &b)| self.match_edge(a, b, work)).collect();
            path.move_to(first);
            let mut edge = 0;
            while edge < points.len() {
                let mut last = edge + 1;
                if let Some(mut run) = spans[edge] {
                    while let Some(Some(next)) = spans.get(last) {
                        if run.segment != next.segment || (run.to - next.from).abs() > 1e-7
                            || (run.to > run.from) != (next.to > next.from)
                        {
                            break;
                        }
                        run.to = next.to;
                        last += 1;
                    }
                    self.append(&mut path, run, points[edge], points[last % points.len()]);
                } else {
                    path.line_to(points[last % points.len()]);
                }
                edge = last;
            }
            path.close_path();
        }
        path
    }

    fn match_edge(&self, a: Point, b: Point, work: &mut usize) -> Option<Span> {
        if !self.has_curves {
            return None;
        }
        *work = work.checked_sub(self.segments.len())?;
        let limit = self.tolerance * 1.5;
        let error_sq = limit * limit;
        let accuracy = (self.tolerance * 0.001).clamp(1e-9, 1e-4);
        let inside = |p: Point, bounds: Rect| {
            (bounds.x0 - limit..=bounds.x1 + limit).contains(&p.x)
                && (bounds.y0 - limit..=bounds.y1 + limit).contains(&p.y)
        };
        let mut best = None;
        let mut best_error = f64::INFINITY;
        for (index, &(segment, bounds)) in self.segments.iter().enumerate() {
            if !inside(a, bounds) || !inside(b, bounds) {
                continue;
            }
            let start = segment.nearest(a, accuracy);
            let end = segment.nearest(b, accuracy);
            if start.distance_sq > error_sq || end.distance_sq > error_sq || (start.t - end.t).abs() < 1e-9 {
                continue;
            }
            let midpoint = segment.eval((start.t + end.t) * 0.5);
            let deviation = kurbo::Line::new(a, b).nearest(midpoint, accuracy).distance_sq;
            let error = start.distance_sq + end.distance_sq + deviation;
            if deviation <= error_sq && error < best_error {
                best = Some(Span { segment: index, from: start.t, to: end.t });
                best_error = error;
            }
        }
        best
    }

    fn append(&self, path: &mut BezPath, span: Span, a: Point, b: Point) {
        let source = self.segments[span.segment].0;
        let part = if span.from <= span.to {
            source.subsegment(span.from..span.to)
        } else {
            source.subsegment(span.to..span.from).reverse()
        };
        // Keep shared overlay vertices identical on both sides of a cut.
        let cubic = match part {
            PathSeg::Quad(quad) => quad.raise(),
            PathSeg::Cubic(cubic) => cubic,
            PathSeg::Line(_) => {
                path.line_to(b);
                return;
            }
        };
        path.push(PathEl::CurveTo(cubic.p1 + (a - cubic.p0), cubic.p2 + (b - cubic.p3), b));
    }
}
