use dtool_protocol::CommandError;
use kurbo::{BezPath, Line, ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point, Rect, Shape};

use super::request_too_large;

const PARAM_EPSILON: f64 = 1e-8;

pub(super) fn partition(path: &BezPath) -> (BezPath, Vec<BezPath>) {
    let mut closed = BezPath::new();
    let mut open = Vec::new();
    for contour in contours(path) {
        if matches!(contour.last(), Some(PathEl::ClosePath)) {
            closed.extend(contour.iter().copied());
        } else if contour.len() > 1 {
            open.push(BezPath::from_vec(contour.to_vec()));
        }
    }
    (closed, open)
}

fn contours(path: &BezPath) -> impl Iterator<Item = &[PathEl]> {
    let mut elements = path.elements();
    std::iter::from_fn(move || {
        if elements.is_empty() {
            return None;
        }
        let end = elements
            .iter()
            .skip(1)
            .position(|el| matches!(el, PathEl::MoveTo(_)))
            .map_or(elements.len(), |index| index + 1);
        let (contour, rest) = elements.split_at(end);
        elements = rest;
        Some(contour)
    })
}

pub(super) fn nearest_scissor(path: &BezPath, point: Point, radius: f64) -> Option<(f64, BezPath)> {
    let (_, open) = partition(path);
    let mut nearest = None;
    for (contour, path) in open.iter().enumerate() {
        let segments: Vec<_> = path.segments().collect();
        let end = segments.len() as f64;
        for (index, segment) in segments.iter().enumerate() {
            let hit = segment.nearest(point, 0.001);
            let node = hit.t.round();
            let at = if segment.eval(node).distance_squared(point) <= (radius * 0.5).powi(2) {
                index as f64 + node
            } else {
                index as f64 + hit.t
            };
            if hit.distance_sq <= radius * radius && at > PARAM_EPSILON && at < end - PARAM_EPSILON
                && nearest.as_ref().is_none_or(|(distance, _, _)| hit.distance_sq < *distance)
            {
                nearest = Some((hit.distance_sq, contour, at));
            }
        }
    }
    let (distance, target, at) = nearest?;
    let mut result = BezPath::new();
    let mut open_index = 0;
    for contour in contours(path) {
        if matches!(contour.last(), Some(PathEl::ClosePath)) {
            result.extend(contour.iter().copied());
        } else if contour.len() > 1 {
            if target == open_index {
                let segments: Vec<_> = open[target].segments().collect();
                for part in split_at(&segments, [at]) {
                    result.extend(part.elements().iter().copied());
                }
            } else {
                result.extend(contour.iter().copied());
            }
            open_index += 1;
        }
    }
    Some((distance, result))
}

pub(super) struct KnifeCut {
    edges: Vec<(Rect, Line)>,
    remaining_work: usize,
}

impl KnifeCut {
    pub(super) fn new(points: &[[f64; 2]]) -> Self {
        let edges = points
            .windows(2)
            .filter(|pair| pair[0] != pair[1])
            .map(|pair| {
                let start = Point::new(pair[0][0], pair[0][1]);
                let end = Point::new(pair[1][0], pair[1][1]);
                (Rect::from_points(start, end), Line::new(start, end))
            })
            .collect();
        Self {
            edges,
            remaining_work: 8_000_000,
        }
    }

    pub(super) fn split(
        &mut self,
        paths: Vec<BezPath>,
    ) -> Result<(bool, Vec<BezPath>), CommandError> {
        let mut changed = false;
        let mut pieces = Vec::new();
        for path in paths {
            let segments: Vec<_> = path.segments().collect();
            let cuts = self.intersections(&path, &segments, false)?;
            if cuts.is_empty() {
                pieces.push(path);
            } else {
                changed = true;
                pieces.extend(split_at(&segments, cuts));
            }
        }
        Ok((changed, pieces))
    }

    pub(super) fn close_straight_cut(
        &mut self,
        path: &BezPath,
    ) -> Result<Option<Vec<BezPath>>, CommandError> {
        let [(_, line)] = self.edges.as_slice() else { return Ok(None) };
        let line = *line;
        if contours(path).count() != 1 {
            return Ok(None);
        }
        let Some(mut pieces) = self.open_closed(path)? else { return Ok(None) };
        let [first, second] = pieces.as_mut_slice() else { return Ok(None) };
        let axis = line.p1 - line.p0;
        let side = |piece: &BezPath| {
            let tolerance = PARAM_EPSILON * axis.hypot();
            let mut positive = false;
            let mut negative = false;
            for segment in piece.segments() {
                for t in [0.25, 0.5, 0.75] {
                    let offset = axis.cross(segment.eval(t) - line.p0);
                    positive |= offset > tolerance;
                    negative |= offset < -tolerance;
                }
            }
            match (positive, negative) {
                (true, false) => 1,
                (false, true) => -1,
                _ => 0,
            }
        };
        let first_side = side(first);
        let second_side = side(second);
        if first_side == 0 || second_side == 0 || first_side == second_side {
            return Ok(None);
        }
        first.close_path();
        second.close_path();
        Ok(Some(pieces))
    }

    pub(super) fn open_closed(
        &mut self,
        path: &BezPath,
    ) -> Result<Option<Vec<BezPath>>, CommandError> {
        let mut pieces = Vec::new();
        let mut untouched = BezPath::new();
        for contour in contours(path) {
            let path = BezPath::from_vec(contour.to_vec());
            let segments: Vec<_> = path.segments().collect();
            let cuts = self.intersections(&path, &segments, true)?;
            if cuts.is_empty() {
                untouched.extend(contour.iter().copied());
                continue;
            }
            let seam_cut = cuts.first() == Some(&0.0);
            let mut open = split_at(&segments, cuts.into_iter().filter(|cut| *cut > 0.0));
            if !seam_cut && open.len() > 1 {
                let mut tail = open.pop().unwrap();
                tail.extend(open[0].elements().iter().skip(1).copied());
                open[0] = tail;
            }
            pieces.extend(open);
        }
        let Some(first) = pieces.first_mut() else {
            return Ok(None);
        };
        first.extend(untouched.elements().iter().copied());
        Ok(Some(pieces))
    }

    fn intersections(
        &mut self,
        path: &BezPath,
        segments: &[PathSeg],
        closed: bool,
    ) -> Result<Vec<f64>, CommandError> {
        let bounds = path.bounding_box();
        let edges: Vec<_> = self
            .edges
            .iter()
            .filter(|(edge, _)| touches(*edge, bounds))
            .collect();
        let path_end = segments.len() as f64;
        let count = path
            .elements()
            .len()
            .saturating_mul(edges.len())
            .saturating_add(self.edges.len());
        self.remaining_work = self
            .remaining_work
            .checked_sub(count)
            .ok_or_else(request_too_large)?;
        let mut cuts = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            let bounds = segment.bounding_box();
            for (_, line) in edges
                .iter()
                .copied()
                .filter(|(edge, _)| touches(*edge, bounds))
            {
                for hit in segment.intersect_line(*line) {
                    let nominal = index as f64 + hit.segment_t.clamp(0.0, 1.0);
                    let node = nominal.round();
                    let at = if (nominal - node).abs() < PARAM_EPSILON {
                        node
                    } else {
                        nominal
                    };
                    if closed && (at == 0.0 || at == path_end) {
                        cuts.push(0.0);
                    } else if at > 0.0 && at < path_end {
                        cuts.push(at);
                    }
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < PARAM_EPSILON);
        Ok(cuts)
    }
}

fn touches(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && a.x1 >= b.x0 && a.y0 <= b.y1 && a.y1 >= b.y0
}

fn split_at(segments: &[PathSeg], cuts: impl IntoIterator<Item = f64>) -> Vec<BezPath> {
    let mut cuts = cuts.into_iter().peekable();
    let mut pieces = Vec::new();
    let mut piece = BezPath::new();
    for (index, &segment) in segments.iter().enumerate() {
        let offset = index as f64;
        let mut from = 0.0;
        while let Some(cut) = cuts.next_if(|cut| *cut <= offset + 1.0) {
            let to = cut - offset;
            append(&mut piece, segment, from, to);
            if !piece.is_empty() {
                pieces.push(std::mem::take(&mut piece));
            }
            from = to;
        }
        append(&mut piece, segment, from, 1.0);
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}

fn append(path: &mut BezPath, segment: PathSeg, from: f64, to: f64) {
    if to > from {
        let part = segment.subsegment(from..to);
        if path.is_empty() {
            path.move_to(part.start());
        }
        path.push(part.as_path_el());
    }
}
