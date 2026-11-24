use dtool_core::curve::{NodeKind, PathPoint, PointId};

type Point = [f64; 2];

pub(super) fn interpolating(points: &[PathPoint], closed: bool) -> Vec<PathPoint> {
    let count = points.len();
    if count < 3 {
        return points.to_vec();
    }
    let interval = |a: Point, b: Point| (a[0] - b[0]).hypot(a[1] - b[1]).sqrt().max(1e-6);
    points.iter().enumerate().map(|(i, point)| {
        let prev = if i > 0 || closed { Some(points[(i + count - 1) % count].pos) } else { None };
        let next = if i + 1 < count || closed { Some(points[(i + 1) % count].pos) } else { None };
        let before = prev.map_or(0.0, |p| interval(p, point.pos));
        let after = next.map_or(0.0, |p| interval(point.pos, p));
        let tangent = std::array::from_fn(|axis| {
            let left = prev.map(|p| (point.pos[axis] - p[axis]) / before);
            let right = next.map(|p| (p[axis] - point.pos[axis]) / after);
            match (left, right) {
                (Some(a), Some(b)) => (after * a + before * b) / (before + after),
                (Some(a), None) | (None, Some(a)) => a,
                _ => 0.0,
            }
        });
        bezier_node(point.id, point.pos, tangent, prev.map(|_| before / 3.0), next.map(|_| after / 3.0))
    }).collect()
}

fn bezier_node(id: PointId, pos: Point, tangent: Point, before: Option<f64>, after: Option<f64>) -> PathPoint {
    let handle = |scale: f64| std::array::from_fn(|i| pos[i] + tangent[i] * scale);
    PathPoint {
        id, pos,
        handle_in: before.map(|step| handle(-step)),
        handle_out: after.map(handle),
        kind: NodeKind::Smooth,
        corner: None,
    }
}

pub(super) fn basis_spline(points: &[PathPoint], closed: bool) -> Vec<PathPoint> {
    let count = points.len();
    if count < 3 {
        return points.to_vec();
    }
    if closed {
        return points.iter().enumerate().map(|(i, point)| {
            let prev = points[(i + count - 1) % count].pos;
            let next = points[(i + 1) % count].pos;
            let pos = std::array::from_fn(|axis| (prev[axis] + 4.0 * point.pos[axis] + next[axis]) / 6.0);
            let tangent = std::array::from_fn(|axis| (next[axis] - prev[axis]) / 6.0);
            bezier_node(point.id, pos, tangent, Some(1.0), Some(1.0))
        }).collect();
    }

    let degree = (count - 1).min(3);
    let spans = count - degree;
    let control: Vec<_> = points.iter().map(|point| point.pos).collect();
    let knots: Vec<_> = (0..=count + degree).map(|i| i.saturating_sub(degree).min(spans) as f64).collect();
    let derivative: Vec<Point> = control.windows(2).enumerate().map(|(i, pair)| {
        let scale = degree as f64 / (knots[i + degree + 1] - knots[i + 1]);
        std::array::from_fn(|axis| (pair[1][axis] - pair[0][axis]) * scale)
    }).collect();
    (0..=spans).map(|i| {
        let u = i as f64;
        let pos = de_boor(&control, &knots, degree, u);
        let tangent = de_boor(&derivative, &knots[1..knots.len() - 1], degree - 1, u);
        bezier_node(points[i].id, pos, tangent, (i > 0).then_some(1.0 / 3.0), (i < spans).then_some(1.0 / 3.0))
    }).collect()
}

fn de_boor(points: &[Point], knots: &[f64], degree: usize, u: f64) -> Point {
    let span = (u.floor() as usize + degree).min(points.len() - 1);
    let first = span - degree;
    let mut work = [[0.0; 2]; 4];
    work[..=degree].copy_from_slice(&points[first..=span]);
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let i = first + j;
            let offset = (u - knots[i]) / (knots[i + degree + 1 - level] - knots[i]);
            work[j] = std::array::from_fn(|axis| work[j - 1][axis] + offset * (work[j][axis] - work[j - 1][axis]));
        }
    }
    work[degree]
}
