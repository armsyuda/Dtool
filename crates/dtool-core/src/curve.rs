use kurbo::{BezPath, PathEl, Point};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PointId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    #[default]
    Sharp,
    Smooth,
    Smart,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathPoint {
    pub id: PointId,
    pub pos: [f64; 2],
    #[serde(default)]
    pub handle_in: Option<[f64; 2]>,
    #[serde(default)]
    pub handle_out: Option<[f64; 2]>,
    pub kind: NodeKind,
    #[serde(default)]
    pub corner: Option<CornerSpec>,
}

impl PathPoint {
    pub fn sharp(id: PointId, pos: [f64; 2]) -> Self {
        Self { id, pos, handle_in: None, handle_out: None, kind: NodeKind::Sharp, corner: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CornerKind {
    #[default]
    None,
    Rounded,
    Straight,
    Concave,
    Cutout,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CornerSpec {
    pub kind: CornerKind,
    pub radius: f64,
}

pub const CORNER_SEGMENT_EPSILON: f64 = 1e-4;

fn point_distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn stored_radius(point: &PathPoint) -> f64 {
    match point.corner {
        Some(spec) if spec.kind != CornerKind::None && spec.radius.is_finite() => spec.radius.max(0.0),
        _ => 0.0,
    }
}

pub fn corner_neighbors(contour: &Contour, index: usize) -> Option<(usize, usize)> {
    let len = contour.points.len();
    if index >= len || len < 3 {
        return None;
    }
    if contour.closed {
        Some(((index + len - 1) % len, (index + 1) % len))
    } else if index == 0 || index + 1 == len {
        None
    } else {
        Some((index - 1, index + 1))
    }
}

pub fn corner_eligible(contour: &Contour, index: usize) -> bool {
    let Some((prev, next)) = corner_neighbors(contour, index) else {
        return false;
    };
    let point = &contour.points[index];
    point.kind == NodeKind::Sharp
        && point_distance(contour.points[prev].pos, point.pos) > CORNER_SEGMENT_EPSILON
        && point_distance(point.pos, contour.points[next].pos) > CORNER_SEGMENT_EPSILON
}

pub fn max_corner_radius(contour: &Contour, index: usize, pending: &[PointId]) -> f64 {
    if !corner_eligible(contour, index) {
        return 0.0;
    }
    let (prev, next) = corner_neighbors(contour, index).expect("eligible");
    let point = &contour.points[index];
    let budget = |neighbour: usize, distance: f64| -> f64 {
        if pending.iter().any(|id| *id == contour.points[neighbour].id) {
            distance * 0.5
        } else {
            (distance - stored_radius(&contour.points[neighbour])).max(0.0)
        }
    };
    budget(prev, point_distance(contour.points[prev].pos, point.pos))
        .min(budget(next, point_distance(point.pos, contour.points[next].pos)))
}

pub fn effective_radius(contour: &Contour, index: usize) -> f64 {
    if !corner_eligible(contour, index) {
        return 0.0;
    }
    stored_radius(&contour.points[index]).min(max_corner_radius(contour, index, &[]))
}

pub fn corner_axis(contour: &Contour, index: usize) -> Option<[f64; 2]> {
    if !corner_eligible(contour, index) {
        return None;
    }
    let (prev, next) = corner_neighbors(contour, index)?;
    let pos = contour.points[index].pos;
    let mut dir = [
        (contour.points[prev].pos[0] - pos[0]) + (contour.points[next].pos[0] - pos[0]),
        (contour.points[prev].pos[1] - pos[1]) + (contour.points[next].pos[1] - pos[1]),
    ];
    let length = dir[0].hypot(dir[1]);
    if length <= CORNER_SEGMENT_EPSILON {
        return Some([0.0, 0.0]);
    }
    dir = [dir[0] / length, dir[1] / length];
    Some(dir)
}

pub fn corner_handle_position(contour: &Contour, index: usize) -> Option<[f64; 2]> {
    let dir = corner_axis(contour, index)?;
    let pos = contour.points[index].pos;
    let radius = effective_radius(contour, index);
    Some([pos[0] + dir[0] * radius, pos[1] + dir[1] * radius])
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contour {
    #[serde(default)]
    pub points: Vec<PathPoint>,
    #[serde(default)]
    pub closed: bool,
}

impl Contour {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn smart_handles(points: &[[f64; 2]]) -> Vec<PathPoint> {
        let mut out = Vec::with_capacity(points.len());
        for (i, pos) in points.iter().enumerate() {
            let id = PointId((i as u64) + 1);
            if points.len() < 2 {
                out.push(PathPoint::sharp(id, *pos));
                continue;
            }
            let prev = if i == 0 { points[points.len() - 1] } else { points[i - 1] };
            let next = if i + 1 == points.len() { points[0] } else { points[i + 1] };
            let hx = (next[0] - prev[0]) / 6.0;
            let hy = (next[1] - prev[1]) / 6.0;
            out.push(PathPoint {
                id,
                pos: *pos,
                handle_in: Some([pos[0] - hx, pos[1] - hy]),
                handle_out: Some([pos[0] + hx, pos[1] + hy]),
                kind: NodeKind::Smart,
                corner: None,
            });
        }
        out
    }

    pub fn to_kurbo(&self) -> BezPath {
        let mut path = BezPath::new();
        let Some(first) = self.points.first() else {
            return path;
        };
        path.move_to(to_point(first.pos));
        for window in self.points.windows(2) {
            append_segment(&mut path, &window[0], &window[1]);
        }
        if self.closed && self.points.len() > 1 {
            if let Some(last) = self.points.last() {
                append_segment(&mut path, last, first);
            }
            path.close_path();
        }
        path
    }

    pub fn from_kurbo(path: &BezPath) -> Self {
        contours_from_kurbo(path).into_iter().next().unwrap_or_default()
    }

    pub fn bounds(&self) -> [f32; 4] {
        crate::geometry::bez_path_bounds(&self.to_kurbo())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    #[serde(default)]
    pub contour: Contour,
    #[serde(default)]
    pub holes: Vec<Contour>,
    #[serde(default)]
    pub source_object: Option<u64>,
}

impl Curve {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_contour_and_holes(contour: Contour, holes: Vec<Contour>) -> Self {
        Self { contour, holes, source_object: None }
    }

    pub fn from_traced(contour: &BezPath, holes: &[BezPath]) -> Self {
        Self::from_contour_and_holes(
            Contour::from_kurbo(contour),
            holes.iter().map(Contour::from_kurbo).collect(),
        )
    }

    pub fn from_kurbo(path: &BezPath) -> Self {
        let mut contours = contours_from_kurbo(path);
        if contours.is_empty() {
            return Self::default();
        }
        let contour = contours.remove(0);
        Self { contour, holes: contours, source_object: None }
    }

    pub fn with_source(mut self, source_object: u64) -> Self {
        self.source_object = Some(source_object);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.contour.is_empty() && self.holes.iter().all(Contour::is_empty)
    }

    pub fn hole_count(&self) -> usize {
        self.holes.len()
    }

    pub fn to_kurbo(&self) -> BezPath {
        let mut path = self.contour.to_kurbo();
        for hole in &self.holes {
            for el in hole.to_kurbo().elements() {
                path.push(*el);
            }
        }
        path
    }

    pub fn to_bez_paths(&self) -> Vec<BezPath> {
        let mut paths = Vec::with_capacity(1 + self.holes.len());
        if !self.contour.is_empty() {
            paths.push(self.contour.to_kurbo());
        }
        for hole in &self.holes {
            if !hole.is_empty() {
                paths.push(hole.to_kurbo());
            }
        }
        paths
    }

    pub fn bounds(&self) -> [f32; 4] {
        crate::geometry::bez_path_bounds(&self.to_kurbo())
    }

    pub fn contours(&self) -> impl Iterator<Item = &Contour> {
        std::iter::once(&self.contour).chain(self.holes.iter())
    }
}

fn to_point(pos: [f64; 2]) -> Point {
    Point::new(pos[0], pos[1])
}

fn to_xy(point: Point) -> [f64; 2] {
    [point.x, point.y]
}

fn approx_eq(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() <= 1e-9 && (a[1] - b[1]).abs() <= 1e-9
}

fn append_segment(path: &mut BezPath, from: &PathPoint, to: &PathPoint) {
    match (from.handle_out, to.handle_in) {
        (None, None) => {
            path.line_to(to_point(to.pos));
        }
        (out, inn) => {
            let c1 = out.unwrap_or(from.pos);
            let c2 = inn.unwrap_or(to.pos);
            path.curve_to(to_point(c1), to_point(c2), to_point(to.pos));
        }
    }
}

fn finish_contour(out: &mut Vec<Contour>, current: &mut Contour) {
    if current.points.is_empty() {
        *current = Contour::default();
        return;
    }
    if current.closed && current.points.len() >= 2 {
        let last = current.points.last().cloned();
        let first_pos = current.points[0].pos;
        if let Some(last) = last {
            if approx_eq(last.pos, first_pos) {
                if let Some(first) = current.points.first_mut() {
                    if first.handle_in.is_none() {
                        first.handle_in = last.handle_in;
                    }
                    if first.kind == NodeKind::Sharp && last.handle_in.is_some() {
                        first.kind = last.kind;
                    }
                }
                current.points.pop();
            }
        }
    }
    out.push(std::mem::take(current));
}

fn push_point(current: &mut Contour, next_id: &mut u64, pos: [f64; 2], handle_in: Option<[f64; 2]>, kind: NodeKind) {
    current.points.push(PathPoint {
        id: PointId(*next_id),
        pos,
        handle_in,
        handle_out: None,
        kind,
        corner: None,
    });
    *next_id += 1;
}

fn contours_from_kurbo(path: &BezPath) -> Vec<Contour> {
    let mut out = Vec::new();
    let mut current = Contour::default();
    let mut next_id = 1u64;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                finish_contour(&mut out, &mut current);
                push_point(&mut current, &mut next_id, to_xy(*p), None, NodeKind::Sharp);
            }
            PathEl::LineTo(p) => {
                push_point(&mut current, &mut next_id, to_xy(*p), None, NodeKind::Sharp);
            }
            PathEl::QuadTo(c, p) => {
                let p0 = current.points.last().map(|pt| pt.pos).unwrap_or(to_xy(*c));
                let c1 = [
                    p0[0] + (c.x - p0[0]) * 2.0 / 3.0,
                    p0[1] + (c.y - p0[1]) * 2.0 / 3.0,
                ];
                let end = to_xy(*p);
                let c2 = [
                    end[0] + (c.x - end[0]) * 2.0 / 3.0,
                    end[1] + (c.y - end[1]) * 2.0 / 3.0,
                ];
                if let Some(last) = current.points.last_mut() {
                    last.handle_out = Some(c1);
                    if last.kind == NodeKind::Sharp {
                        last.kind = NodeKind::Smooth;
                    }
                } else {
                    push_point(&mut current, &mut next_id, p0, None, NodeKind::Smooth);
                    if let Some(last) = current.points.last_mut() {
                        last.handle_out = Some(c1);
                    }
                }
                push_point(&mut current, &mut next_id, end, Some(c2), NodeKind::Smooth);
            }
            PathEl::CurveTo(c1, c2, p) => {
                if let Some(last) = current.points.last_mut() {
                    last.handle_out = Some(to_xy(*c1));
                    if last.kind == NodeKind::Sharp {
                        last.kind = NodeKind::Smooth;
                    }
                } else {
                    push_point(&mut current, &mut next_id, to_xy(*c1), None, NodeKind::Smooth);
                    if let Some(last) = current.points.last_mut() {
                        last.handle_out = Some(to_xy(*c1));
                    }
                }
                push_point(&mut current, &mut next_id, to_xy(*p), Some(to_xy(*c2)), NodeKind::Smooth);
            }
            PathEl::ClosePath => {
                current.closed = true;
                finish_contour(&mut out, &mut current);
            }
        }
    }
    finish_contour(&mut out, &mut current);
    out
}
