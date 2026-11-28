use crate::paint::{GradientGeom, GradientKind};
use kurbo::Vec2;

pub(super) fn fit_gradient(kind: &mut GradientKind, geom: &mut GradientGeom, size: [f64; 2], fit: &str) {
    if fit == "none" || !size.iter().all(|side| side.is_finite() && *side > 0.0) {
        return;
    }
    let axis = Vec2::new(geom.end[0] - geom.start[0], geom.end[1] - geom.start[1]);
    let radius = axis.hypot();
    if !radius.is_finite() || radius <= 1e-12 {
        return;
    }
    let direction = axis / radius;
    let normal = Vec2::new(-direction.y, direction.x);
    let ratio = if *kind == GradientKind::Elliptical { geom.ratio.abs() } else { 1.0 };
    if !ratio.is_finite() || ratio <= 1e-12 {
        return;
    }

    // A linear ramp has no transverse limit. Use one stop interval along both axes
    // as its reference square, and use the outer ellipse for radial fills.
    let extent = if *kind == GradientKind::Linear {
        let span = radius * (direction.x.abs() + direction.y.abs());
        Vec2::new(span, span)
    } else if *kind == GradientKind::Conical {
        let [dx, dy, sx, sy, ..] = geom.gradient_matrix(*kind);
        Vec2::new(2.0 * dx.hypot(sx), 2.0 * dy.hypot(sy))
    } else {
        Vec2::new(
            2.0 * radius * direction.x.hypot(normal.x * ratio),
            2.0 * radius * direction.y.hypot(normal.y * ratio),
        )
    };
    let fit_x = size[0] / extent.x;
    let fit_y = size[1] / extent.y;
    let [sx, sy] = match fit {
        "min" => [fit_x.min(fit_y); 2],
        "stretch" => [fit_x, fit_y],
        _ => [fit_x.max(fit_y); 2],
    };
    if ![sx, sy].iter().all(|scale| scale.is_finite() && *scale > 0.0) {
        return;
    }
    let center = Vec2::new(size[0] * 0.5, size[1] * 0.5);
    let scale = |point: Vec2| Vec2::new(point.x * sx, point.y * sy);
    let coordinates = |point: Vec2| [point.x, point.y];
    match kind {
        GradientKind::Linear => {
            // A linear ramp is defined by its scalar projection. Transform the
            // projection by the inverse transpose when stretching diagonally.
            let projection = Vec2::new(direction.x / sx, direction.y / sy);
            let half = projection * (0.5 * radius / projection.hypot2());
            geom.start = coordinates(center - half);
            geom.end = coordinates(center + half);
        }
        GradientKind::Conical => {
            *geom = geom.transform_for_kind(*kind, [sx, 0.0, 0.0, sy,
                center.x - sx * geom.start[0], center.y - sy * geom.start[1]]);
        }
        GradientKind::Radial | GradientKind::Elliptical => {
            geom.start = coordinates(center);
            let main = scale(axis);
            if sx == sy {
                geom.end = coordinates(center + main);
                return;
            }
            let cross = scale(normal * (radius * ratio));
            // Recover perpendicular ellipse axes after a nonuniform stretch.
            // The transformed axes may no longer be perpendicular.
            let xx = main.x * main.x + cross.x * cross.x;
            let yy = main.y * main.y + cross.y * cross.y;
            let xy = main.x * main.y + cross.x * cross.y;
            let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
            let (sin, cos) = angle.sin_cos();
            let mut fitted = Vec2::new(cos, sin);
            let mut perpendicular = Vec2::new(-sin, cos);
            if perpendicular.dot(main).abs() > fitted.dot(main).abs() {
                (fitted, perpendicular) = (perpendicular, -fitted);
            }
            if fitted.dot(main) < 0.0 {
                (fitted, perpendicular) = (-fitted, -perpendicular);
            }
            let r1 = fitted.dot(main).hypot(fitted.dot(cross));
            let r2 = perpendicular.dot(main).hypot(perpendicular.dot(cross));
            if let Some([fx, fy]) = geom.focus {
                let focus = main * fx + cross * fy;
                geom.focus = Some([fitted.dot(focus) / r1, perpendicular.dot(focus) / r2]);
            }
            geom.end = coordinates(center + fitted * r1);
            geom.ratio = r2 / r1;
            *kind = GradientKind::Elliptical;
        }
    }
}
