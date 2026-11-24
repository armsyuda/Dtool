use crate::curve::{Contour, CornerKind, Curve, PathPoint, PointId};
use crate::object::{Dimension, Object, ObjectKind};
use crate::shape::{CornerRecipe, ShapeKind};
use kurbo::{Affine, Arc, BezPath, Ellipse, PathEl, Point, Rect, RoundedRect, RoundedRectRadii, Shape, Vec2};
use std::f64::consts::{FRAC_1_SQRT_2, PI};

mod stroke;
mod spiral;
pub use stroke::{pressure_stroke_path, styled_stroke_path};

pub const OUTLINE_TOLERANCE: f64 = 0.1;
pub const DIMENSION_TICK: f64 = 6.0;
pub const DIMENSION_LABEL_GAP: f64 = 4.0;
pub const DIMENSION_LABEL_SIZE: f64 = 10.0;

pub fn object_outline(object: &Object) -> Option<BezPath> {
    match &object.kind {
        ObjectKind::Curve(curve) => Some(cornered(curve).to_kurbo()),
        ObjectKind::Shape => match crate::commands::qr::payload(object) {
            Some(payload) => Some(crate::commands::qr::outline(payload, object.size)),
            None => shape_outline(object),
        },
        ObjectKind::Compound(spec) => crate::boolean::compound_outline(spec),
        _ => None,
    }
}

pub fn object_outline_world(object: &Object) -> Option<BezPath> {
    let matrix = crate::geometry::object_matrix(object);
    if matrix.iter().chain(&object.pos).any(|value| !value.is_finite()) {
        return None;
    }
    let path = object_outline(object)?;
    let [x, y] = object.pos.map(f64::from);
    let affine = Affine::new(matrix.map(f64::from)) * Affine::translate((x, y));
    Some(affine * path)
}

fn path_text_scale(object: &Object) -> [f64; 2] {
    object.size.map(|side| if side == 0.0 { 1.0 } else { f64::from(side.abs()) })
}

pub fn text_path_curve(object: &Object) -> Option<Curve> {
    let outline = object_outline(object)?;
    let [w, h] = path_text_scale(object);
    Some(Curve::from_kurbo(&(Affine::scale_non_uniform(1.0 / w, 1.0 / h) * outline)))
}

fn text_path_outline(object: &Object) -> Option<BezPath> {
    match &object.kind {
        ObjectKind::PathText(data) if data.path == object.id => match &object.path_text_curve {
            Some(curve) => {
                let [w, h] = path_text_scale(object);
                Some(Affine::scale_non_uniform(w, h) * curve.to_kurbo())
            }
            None => shape_outline(object),
        },
        _ => object_outline(object),
    }
}

pub fn text_path_contours(object: &Object) -> Vec<(Vec<[f32; 2]>, bool)> {
    map_text_path_contours(object, &crate::geometry::IDENTITY)
}

/// Read a guide in the metric used for shaping its path text.
pub fn path_text_layout_contours(text: &Object, guide: &Object) -> Vec<(Vec<[f32; 2]>, bool)> {
    let inverse = crate::geometry::invert_matrix(&crate::geometry::text_matrix(text))
        .unwrap_or(crate::geometry::IDENTITY);
    map_text_path_contours(guide, &inverse)
}

fn map_text_path_contours(object: &Object, document_to_layout: &[f32; 6]) -> Vec<(Vec<[f32; 2]>, bool)> {
    let Some(path) = text_path_outline(object) else { return Vec::new(); };
    let matrix = crate::geometry::compose(document_to_layout, &crate::geometry::object_matrix(object));
    let [x, y] = object.pos.map(f64::from);
    let path = Affine::new(matrix.map(f64::from)) * Affine::translate((x, y)) * path;
    flatten_subpaths(&path, 0.05).into_iter()
        .map(|(points, closed)| (points.into_iter().map(|point| point.map(|v| v as f32)).collect(), closed))
        .collect()
}

pub fn flatten_subpaths(path: &BezPath, tolerance: f64) -> Vec<(Vec<[f64; 2]>, bool)> {
    let mut out: Vec<(Vec<[f64; 2]>, bool)> = Vec::new();
    kurbo::flatten(path.elements().iter().copied(), tolerance, |element| match element {
        PathEl::MoveTo(p) => out.push((vec![[p.x, p.y]], false)),
        PathEl::LineTo(p) => {
            if let Some((points, _)) = out.last_mut() {
                points.push([p.x, p.y]);
            }
        }
        PathEl::ClosePath => {
            if let Some((points, closed)) = out.last_mut() {
                *closed = true;
                if points.len() > 1 && points.first() == points.last() {
                    points.pop();
                }
            }
        }
        _ => {}
    });
    out.retain(|(points, _)| points.len() > 1);
    out
}

pub fn object_contours(object: &Object, tolerance: f64) -> Vec<(Vec<[f32; 2]>, bool)> {
    if matches!(object.kind, ObjectKind::Group | ObjectKind::Table(_) | ObjectKind::PathText(_) | ObjectKind::Dimension(_)) {
        return Vec::new();
    }
    let path = object_outline(object).unwrap_or_else(|| {
        let w = f64::from(object.size[0].abs()).max(0.0);
        let h = f64::from(object.size[1].abs()).max(0.0);
        Rect::new(0.0, 0.0, w, h).to_path(tolerance)
    });
    flatten_subpaths(&path, tolerance)
        .into_iter()
        .map(|(points, closed)| (points.into_iter().map(|p| [p[0] as f32, p[1] as f32]).collect(), closed))
        .collect()
}

fn shape_outline(object: &Object) -> Option<BezPath> {
    let w = f64::from(object.size[0].abs());
    let h = f64::from(object.size[1].abs());
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let bounds = Rect::new(0.0, 0.0, w, h);
    let Some(recipe) = &object.shape else {
        return Some(bounds.to_path(OUTLINE_TOLERANCE));
    };
    match recipe.kind {
        ShapeKind::Rectangle => Some(bounds.to_path(OUTLINE_TOLERANCE)),
        ShapeKind::Ellipse => Some(crate::geometry::ellipse_to_bez_path(w * 0.5, h * 0.5, w * 0.5, h * 0.5)),
        ShapeKind::RoundedRectangle => {
            let r = corner_radii(recipe.corner.as_ref()?, w, h);
            Some(RoundedRect::from_rect(bounds, RoundedRectRadii::new(r[0], r[1], r[2], r[3])).to_path(OUTLINE_TOLERANCE))
        }
        ShapeKind::Polygon => {
            let polygon = recipe.polygon.as_ref()?;
            if polygon.number_of_sides < 3 {
                return None;
            }
            let count = polygon.number_of_sides as usize;
            if polygon.curve != 0.0 || polygon.smooth_points {
                let curve = signed_pct(polygon.curve);
                Some(curved_radial(w, h, count.min(1024), |_| 1.0, [curve; 2], polygon.smooth_points))
            } else {
                Some(radial(w, h, count, |_| 1.0))
            }
        }
        ShapeKind::Star => {
            let star = recipe.star.as_ref()?;
            if star.number_of_points < 3 {
                return None;
            }
            let outer = f64::from(star.outer_radius) / 100.0;
            let inner = f64::from(star.inner_radius) / 100.0;
            if !outer.is_finite() || !inner.is_finite() || outer <= 0.0 || inner < 0.0 {
                return None;
            }
            let radius = |i| if i % 2 == 0 { outer } else { inner };
            if star.curve_left != 0.0 || star.curve_right != 0.0 {
                let curvature = [signed_pct(star.curve_right), signed_pct(star.curve_left)];
                Some(curved_radial(w, h, star.number_of_points.min(1024) as usize * 2, radius, curvature, false))
            } else {
                Some(radial(w, h, star.number_of_points as usize * 2, radius))
            }
        }
        ShapeKind::Triangle => {
            let apex = f64::from(recipe.triangle.as_ref()?.points[0]).clamp(0.0, 1.0) * w;
            Some(polyline(&[(apex, 0.0), (w, h), (0.0, h)]))
        }
        ShapeKind::Diamond => {
            let y = h * pct(recipe.diamond.as_ref()?.indent, 100.0);
            Some(polyline(&[(w * 0.5, 0.0), (w, y), (w * 0.5, h), (0.0, y)]))
        }
        ShapeKind::Heart => {
            let heart = recipe.heart.as_ref()?;
            let indentation = h * (0.08 + 0.36 * pct(heart.indent, 100.0));
            let tip_control = w * (0.24 - 0.16 * pct(heart.pointiness, 100.0));
            let mut path = BezPath::new();
            path.move_to((w * 0.5, indentation));
            path.curve_to((w * 0.42, 0.0), (w * 0.34, 0.0), (w * 0.25, 0.0));
            path.curve_to((w * 0.10, 0.0), (0.0, h * 0.16), (0.0, h * 0.36));
            path.curve_to((0.0, h * 0.62), (w * 0.5 - tip_control, h * 0.88), (w * 0.5, h));
            path.curve_to((w * 0.5 + tip_control, h * 0.88), (w, h * 0.62), (w, h * 0.36));
            path.curve_to((w, h * 0.16), (w * 0.90, 0.0), (w * 0.75, 0.0));
            path.curve_to((w * 0.66, 0.0), (w * 0.58, 0.0), (w * 0.5, indentation));
            path.close_path();
            Some(path)
        }
        ShapeKind::Crescent => {
            let crescent = recipe.crescent.as_ref()?;
            let centre = w * 0.5;
            let middle = h * 0.5;
            let inner_radius = centre * pct(crescent.indent, 100.0);
            let inner_left = centre - inner_radius;
            let kappa = 0.552_284_749_8;
            let dy = middle * kappa;
            let mut path = BezPath::new();
            path.move_to((centre, 0.0));
            path.curve_to((centre * (1.0 - kappa), 0.0), (0.0, middle - dy), (0.0, middle));
            path.curve_to((0.0, middle + dy), (centre * (1.0 - kappa), h), (centre, h));
            path.curve_to((centre - inner_radius * kappa, h), (inner_left, middle + dy), (inner_left, middle));
            path.curve_to((inner_left, middle - dy), (centre - inner_radius * kappa, 0.0), (centre, 0.0));
            path.close_path();
            Some(path)
        }
        ShapeKind::Trapezoid => {
            let inset = w * pct(recipe.trapezoid.as_ref()?.indent, 50.0);
            Some(polyline(&[(inset, 0.0), (w - inset, 0.0), (w, h), (0.0, h)]))
        }
        ShapeKind::Arrow => {
            let arrow = recipe.arrow.as_ref()?;
            let cy = h * 0.5;
            let head = cy * pct(arrow.head_width, 50.0) * 2.0;
            let shaft = (cy * pct(arrow.shaft_width, 50.0) * 2.0).min(head);
            let tail = cy * pct(arrow.tail_width, 50.0) * 2.0;
            let base = w - w * pct(arrow.head_length, 100.0);
            Some(polyline(&[(0.0, cy - tail), (base, cy - shaft), (base, cy - head), (w, cy), (base, cy + head), (base, cy + shaft), (0.0, cy + tail)]))
        }
        ShapeKind::Donut => {
            let donut = recipe.donut.as_ref()?;
            let outer = pct(donut.outer_radius, 100.0);
            let inner = pct(donut.inner_radius, 100.0).min(outer);
            if outer <= 0.0 {
                return None;
            }
            let mut path = ellipse(w, h, outer);
            if inner > 0.0 {
                path.extend(ellipse(w, h, inner).reverse_subpaths().elements().iter().copied());
            }
            Some(path)
        }
        ShapeKind::Pie => {
            let pie = recipe.pie.as_ref()?;
            let outer = pct(pie.outer_radius, 100.0);
            let inner = pct(pie.inner_radius, 100.0).min(outer);
            if outer <= 0.0 {
                return None;
            }
            let start = f64::from(pie.start_angle).to_radians();
            let mut sweep = f64::from(pie.end_angle - pie.start_angle).rem_euclid(360.0).to_radians();
            if sweep == 0.0 {
                sweep = 2.0 * PI;
            }
            let centre = Point::new(w * 0.5, h * 0.5);
            let (rx, ry) = (w * 0.5 * outer, h * 0.5 * outer);
            let mut path = BezPath::new();
            path.move_to(arc_point(centre, rx, ry, start));
            path.extend(Arc::new(centre, Vec2::new(rx, ry), start, sweep, 0.0).append_iter(OUTLINE_TOLERANCE));
            if inner > 0.0 {
                let (ix, iy) = (w * 0.5 * inner, h * 0.5 * inner);
                path.line_to(arc_point(centre, ix, iy, start + sweep));
                path.extend(Arc::new(centre, Vec2::new(ix, iy), start + sweep, -sweep, 0.0).append_iter(OUTLINE_TOLERANCE));
            } else {
                path.line_to(centre);
            }
            path.close_path();
            Some(path)
        }
        ShapeKind::Cog => {
            let cog = recipe.cog.as_ref()?;
            let outer = pct(cog.outer_radius, 50.0) * 2.0;
            let root = (pct(cog.inner_radius, 50.0) * 2.0).min(outer);
            let teeth = cog.number_of_teeth as usize;
            if teeth < 3 || outer <= 0.0 {
                return None;
            }
            let pitch = 2.0 * PI / teeth as f64;
            let half = pitch * pct(cog.tooth_width, 100.0) * 0.5;
            let mut points = Vec::with_capacity(teeth * 4);
            for i in 0..teeth {
                let mid = -PI * 0.5 + pitch * i as f64;
                for (angle, r) in [(mid - half, root), (mid - half, outer), (mid + half, outer), (mid + half, root)] {
                    points.push((w * 0.5 + w * 0.5 * r * angle.cos(), h * 0.5 + h * 0.5 * r * angle.sin()));
                }
            }
            Some(polyline(&points))
        }
        ShapeKind::SquareStar => {
            let star = recipe.square_star.as_ref()?;
            if star.number_of_sides < 3 {
                return None;
            }
            let inner = 1.0 - pct(star.cutout, 100.0);
            Some(radial(w, h, star.number_of_sides as usize * 2, |i| if i % 2 == 0 { 1.0 } else { inner }))
        }
        ShapeKind::DoubleStar => {
            let star = recipe.double_star.as_ref()?;
            if star.number_of_points < 3 {
                return None;
            }
            let inner = pct(star.inner_radius, 100.0);
            let tip = inner + (1.0 - inner) * 0.65 * pct(star.point_radius, 100.0);
            let points = star.number_of_points.min(1024) as usize;
            Some(radial(w, h, points * 4, |i| match i % 4 {
                0 => 1.0,
                2 => tip,
                _ => inner,
            }))
        }
        ShapeKind::Tear => {
            let tear = recipe.tear.as_ref()?;
            let widest = h * (0.28 + 0.44 * pct(tear.indent, 100.0));
            let shoulder = w * (0.08 + 0.32 * pct(tear.pointiness, 100.0));
            let lower_control = widest + (h - widest) * 0.552_284_749_8;
            let mut path = BezPath::new();
            path.move_to((w * 0.5, 0.0));
            path.curve_to((w * 0.5 + shoulder, widest * 0.38), (w, widest * 0.7), (w, widest));
            path.curve_to((w, lower_control), (w * 0.776, h), (w * 0.5, h));
            path.curve_to((w * 0.224, h), (0.0, lower_control), (0.0, widest));
            path.curve_to((0.0, widest * 0.7), (w * 0.5 - shoulder, widest * 0.38), (w * 0.5, 0.0));
            path.close_path();
            Some(path)
        }
        ShapeKind::Cloud => {
            let cloud = recipe.cloud.as_ref()?;
            let valley = 1.0 - 0.32 * pct(cloud.indent, 100.0);
            let tangent = 0.30 - 0.19 * pct(cloud.pointiness, 100.0);
            Some(cloud_outline(w, h, valley, tangent))
        }
        ShapeKind::Segment => {
            let segment = recipe.segment.as_ref()?;
            let centre_y = h * (0.40 + 0.20 * pct(segment.pointiness, 100.0));
            let centre = Point::new(w * 0.5, centre_y);
            let start = (0.94 * pct(segment.indent, 100.0)).asin();
            let radii = Vec2::new(w * 0.5, centre_y);
            let mut path = BezPath::new();
            path.move_to(arc_point(centre, radii.x, radii.y, start));
            path.extend(Arc::new(centre, radii, start, -PI - 2.0 * start, 0.0).append_iter(OUTLINE_TOLERANCE));
            path.close_path();
            Some(path)
        }
        ShapeKind::Cat => {
            let cat = recipe.cat.as_ref()?;
            let ears = 0.08 + 0.14 * pct(cat.pointiness, 100.0);
            let brow = 0.13 + 0.17 * pct(cat.indent, 100.0);
            Some(Affine::scale_non_uniform(w, h) * cat_outline(ears, brow))
        }
        ShapeKind::CalloutEllipse => {
            let callout = recipe.callout_ellipse.as_ref()?;
            let rx = w * pct(callout.radius_x, 50.0);
            let ry = h * pct(callout.radius_y, 50.0);
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            let centre = Point::new(w * 0.5, h * 0.5);
            let length = pct(callout.pointer_length, 100.0) * w.min(h);
            let gap = pct(callout.pointer_width, 100.0) * PI;
            if gap == 0.0 || length == 0.0 {
                return Some(crate::geometry::ellipse_to_bez_path(centre.x, centre.y, rx, ry));
            }
            let angle = PI * 0.5 - f64::from(callout.pointer_angle).to_radians();
            let start = angle + gap * 0.5;
            let mut path = BezPath::new();
            path.move_to(arc_point(centre, rx, ry, start));
            path.extend(Arc::new(centre, Vec2::new(rx, ry), start, 2.0 * PI - gap, 0.0).append_iter(OUTLINE_TOLERANCE));
            path.line_to(arc_point(centre, rx + length, ry + length, angle));
            path.close_path();
            Some(path)
        }
        ShapeKind::CalloutRectangle => {
            let callout = recipe.callout_rectangle.as_ref()?;
            let half_base = w * pct(callout.pointer_width, 100.0) * 0.5;
            let length = h * pct(callout.pointer_length, 100.0);
            let angle = f64::from(callout.pointer_angle).to_radians();
            if !angle.is_finite() {
                return None;
            }
            let tip = (w * 0.5 + length * angle.sin(), h + length * angle.cos());
            Some(polyline(&[(0.0, 0.0), (w, 0.0), (w, h), (w * 0.5 + half_base, h), tip, (w * 0.5 - half_base, h), (0.0, h)]))
        }
        ShapeKind::Spiral => spiral::outline(recipe.spiral.as_ref()?, w, h),
    }
}

fn cloud_outline(w: f64, h: f64, valley: f64, tangent: f64) -> BezPath {
    let mut path = BezPath::new();
    let radii = Vec2::new(w * 0.5, h * 0.5);
    let centre = Point::new(radii.x, radii.y);
    let step = PI / 6.0;
    let point = |angle: f64, radius| arc_point(centre, radii.x * radius, radii.y * radius, angle);
    let start = -PI * 0.5 - step * 0.5;
    path.move_to(point(start, valley));
    for index in 0..12 {
        let angle = start + step * (f64::from(index) + 0.5);
        let before = point(angle - step * 0.5, valley);
        let peak = point(angle, 1.0);
        let after = point(angle + step * 0.5, valley);
        let tangent = Vec2::new(-radii.x * angle.sin(), radii.y * angle.cos()) * (step * tangent);
        let rise = peak - point(angle, valley);
        path.curve_to(before + rise * 0.6, peak - tangent, peak);
        path.curve_to(peak + tangent, after + rise * 0.6, after);
    }
    path.close_path();
    path
}

fn cat_outline(ears: f64, brow: f64) -> BezPath {
    let mut path = BezPath::new();
    let ear_tip = (brow - ears).max(0.0);
    path.move_to((0.30, brow));
    path.line_to((0.13, ear_tip));
    path.curve_to((0.10, 0.21), (0.04, 0.36), (0.10, 0.44));
    path.curve_to((0.12, 0.48), (0.16, 0.50), (0.19, 0.51));
    path.curve_to((0.13, 0.59), (0.09, 0.72), (0.14, 0.83));
    path.curve_to((0.16, 0.91), (0.24, 0.95), (0.32, 0.94));
    path.curve_to((0.30, 1.02), (0.49, 1.02), (0.46, 0.94));
    path.line_to((0.51, 0.94));
    path.curve_to((0.49, 1.02), (0.66, 1.02), (0.65, 0.94));
    path.curve_to((0.75, 0.94), (0.80, 0.87), (0.81, 0.80));
    path.curve_to((0.85, 0.76), (0.89, 0.76), (0.91, 0.79));
    path.curve_to((0.92, 0.84), (1.0, 0.81), (0.98, 0.75));
    path.curve_to((0.95, 0.65), (0.86, 0.63), (0.80, 0.67));
    path.curve_to((0.79, 0.60), (0.76, 0.55), (0.73, 0.51));
    path.curve_to((0.90, 0.47), (0.89, 0.34), (0.84, 0.21));
    path.line_to((0.80, ear_tip));
    path.line_to((0.64, brow));
    path.curve_to((0.54, brow - 0.025), (0.40, brow - 0.025), (0.30, brow));
    path.close_path();
    path
}

fn pct(value: f32, max: f64) -> f64 {
    f64::from(value).clamp(0.0, max) / 100.0
}

fn polyline(points: &[(f64, f64)]) -> BezPath {
    let mut path = BezPath::new();
    for (i, (x, y)) in points.iter().enumerate() {
        if i == 0 {
            path.move_to((*x, *y));
        } else {
            path.line_to((*x, *y));
        }
    }
    path.close_path();
    path
}

fn ellipse(w: f64, h: f64, ratio: f64) -> BezPath {
    let mut path = Ellipse::new(Point::new(w * 0.5, h * 0.5), Vec2::new(w * 0.5 * ratio, h * 0.5 * ratio), 0.0)
        .to_path(OUTLINE_TOLERANCE);
    path.close_path();
    path
}

fn arc_point(centre: Point, rx: f64, ry: f64, angle: f64) -> Point {
    Point::new(centre.x + rx * angle.cos(), centre.y + ry * angle.sin())
}

#[derive(Debug, Clone, PartialEq)]
pub struct DimensionGeometry {
    pub line: [[f64; 2]; 2],
    pub extensions: Vec<[[f64; 2]; 2]>,
    pub ticks: [[[f64; 2]; 2]; 2],
    pub label_at: [f64; 2],
    pub label_angle_deg: f64,
    pub label: String,
}

pub fn dimension_label(dimension: &Dimension) -> String {
    let decimals = crate::commands::measure::readout_decimals(&dimension.units, false);
    let fixed = format!("{:.decimals$}", dimension.length);
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    format!("{trimmed} {}", dimension.units)
}

pub fn dimension_geometry(object: &Object) -> Option<DimensionGeometry> {
    let ObjectKind::Dimension(dimension) = &object.kind else {
        return None;
    };
    let origin = [f64::from(object.pos[0]), f64::from(object.pos[1])];
    let a = [dimension.start[0] + origin[0], dimension.start[1] + origin[1]];
    let b = [dimension.end[0] + origin[0], dimension.end[1] + origin[1]];
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy);
    if !length.is_finite() || length <= 0.0 || !dimension.offset.is_finite() {
        return None;
    }
    let u = [dx / length, dy / length];
    let n = [-u[1], u[0]];
    let shift = |p: [f64; 2], k: f64| [p[0] + n[0] * k, p[1] + n[1] * k];
    let line = [shift(a, dimension.offset), shift(b, dimension.offset)];
    let extensions = if dimension.offset == 0.0 { Vec::new() } else { vec![[a, line[0]], [b, line[1]]] };
    let half = DIMENSION_TICK * 0.5 * FRAC_1_SQRT_2;
    let s = [(u[0] + n[0]) * half, (u[1] + n[1]) * half];
    let tick = |p: [f64; 2]| [[p[0] - s[0], p[1] - s[1]], [p[0] + s[0], p[1] + s[1]]];
    let side = if dimension.offset > 0.0 { 1.0 } else { -1.0 };
    let middle = [(line[0][0] + line[1][0]) * 0.5, (line[0][1] + line[1][1]) * 0.5];
    let mut angle = dy.atan2(dx).to_degrees();
    if angle > 90.0 {
        angle -= 180.0;
    } else if angle <= -90.0 {
        angle += 180.0;
    }
    Some(DimensionGeometry {
        line,
        extensions,
        ticks: [tick(line[0]), tick(line[1])],
        label_at: shift(middle, side * DIMENSION_LABEL_GAP),
        label_angle_deg: angle,
        label: dimension_label(dimension),
    })
}

fn corner_radii(corner: &CornerRecipe, w: f64, h: f64) -> [f64; 4] {
    let short = w.min(h);
    corner.radii.map(|r| {
        let r = f64::from(r);
        let value = if corner.absolute { r } else { short * r.clamp(0.0, 50.0) / 100.0 };
        value.clamp(0.0, short * 0.5)
    })
}

fn radial(w: f64, h: f64, count: usize, radius: impl Fn(usize) -> f64) -> BezPath {
    let mut path = BezPath::new();
    for i in 0..count {
        let angle = -PI * 0.5 + 2.0 * PI * i as f64 / count as f64;
        let r = radius(i);
        let point = Point::new(w * 0.5 + w * 0.5 * r * angle.cos(), h * 0.5 + h * 0.5 * r * angle.sin());
        if i == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close_path();
    path
}

fn signed_pct(value: f32) -> f64 {
    f64::from(value).clamp(-100.0, 100.0) / 100.0
}

fn curved_radial(w: f64, h: f64, count: usize, radius: impl Fn(usize) -> f64, curvature: [f64; 2], smooth: bool) -> BezPath {
    let points: Vec<_> = (0..count).map(|index| {
        let angle = -PI * 0.5 + 2.0 * PI * index as f64 / count as f64;
        let r = radius(index);
        Point::new(w * 0.5 * (1.0 + r * angle.cos()), h * 0.5 * (1.0 + r * angle.sin()))
    }).collect();
    let tangent = |index| {
        let before = points[(index + count - 1) % count];
        let after = points[(index + 1) % count];
        (after - before) * 0.5
    };
    let mut path = BezPath::new();
    path.move_to(points[0]);
    for index in 0..count {
        let next = (index + 1) % count;
        let (start, end) = (points[index], points[next]);
        let edge = end - start;
        let curve = curvature[index % 2];
        let (first, second) = if smooth {
            let amount = (4.0_f64 / 3.0) / (1.0 + (PI / count as f64).cos()) / 2.0;
            let length = amount * (1.0 + curve * 0.6);
            (start + tangent(index) * length, end - tangent(next) * length)
        } else {
            let bulge = Vec2::new(edge.y, -edge.x) * (curve / 3.0);
            (start + edge / 3.0 + bulge, end - edge / 3.0 + bulge)
        };
        path.curve_to(first, second, end);
    }
    path.close_path();
    path
}

pub const CORNER_KINDS: &[&str] = &["none", "rounded", "straight", "concave", "cutout"];
const CORNER_EPS: f64 = 1e-6;

fn c_sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn c_add(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + b[0] * t, a[1] + b[1] * t]
}

fn c_len(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

fn c_unit(a: [f64; 2]) -> Option<[f64; 2]> {
    let l = c_len(a);
    (l > CORNER_EPS).then(|| [a[0] / l, a[1] / l])
}

pub fn corner_eligible(contour: &Contour, index: usize) -> bool {
    if !crate::curve::corner_eligible(contour, index) {
        return false;
    }
    let n = contour.points.len();
    let p = &contour.points[index];
    let prev = &contour.points[(index + n - 1) % n];
    let next = &contour.points[(index + 1) % n];
    if p.handle_in.is_some() || p.handle_out.is_some() || prev.handle_out.is_some() || next.handle_in.is_some() {
        return false;
    }
    match (c_unit(c_sub(prev.pos, p.pos)), c_unit(c_sub(next.pos, p.pos))) {
        (Some(u1), Some(u2)) => (u1[0] * u2[0] + u1[1] * u2[1]) > -1.0 + 1e-9,
        _ => false,
    }
}

pub fn apply_corners(contour: &Contour, corners: &[Option<(&str, f64)>]) -> Contour {
    let n = contour.points.len();
    let mut next_id = contour.points.iter().map(|p| p.id.0).max().unwrap_or(0) + 1;
    let mut fresh = || {
        let id = PointId(next_id);
        next_id += 1;
        id
    };
    let mut points = Vec::with_capacity(n);
    for (i, node) in contour.points.iter().enumerate() {
        let spec = corners.get(i).copied().flatten().filter(|(kind, r)| *kind != "none" && r.is_finite() && *r > 0.0 && CORNER_KINDS.contains(kind));
        let Some((kind, radius)) = spec.filter(|_| corner_eligible(contour, i)) else {
            points.push(node.clone());
            continue;
        };
        let v = node.pos;
        let (prev, next) = (contour.points[(i + n - 1) % n].pos, contour.points[(i + 1) % n].pos);
        let (Some(u1), Some(u2)) = (c_unit(c_sub(prev, v)), c_unit(c_sub(next, v))) else {
            points.push(node.clone());
            continue;
        };
        let max_d = c_len(c_sub(prev, v)).min(c_len(c_sub(next, v))) * 0.5;
        let cos = (u1[0] * u2[0] + u1[1] * u2[1]).clamp(-1.0, 1.0);
        let theta = cos.acos();
        let d = match kind {
            "rounded" => radius / (theta * 0.5).tan(),
            _ => radius,
        }
        .min(max_d);
        let p1 = c_add(v, u1, d);
        let p2 = c_add(v, u2, d);
        let mut a = PathPoint::sharp(node.id, p1);
        let mut b = PathPoint::sharp(fresh(), p2);
        match kind {
            "rounded" => {
                let phi = std::f64::consts::PI - theta;
                let k = if (phi * 0.5).tan().abs() > CORNER_EPS { 4.0 / 3.0 * (phi * 0.25).tan() / (phi * 0.5).tan() } else { 0.0 };
                a.handle_out = Some(c_add(p1, u1, -d * k));
                b.handle_in = Some(c_add(p2, u2, -d * k));
                points.extend([a, b]);
            }
            "concave" => {
                let w1 = c_unit(c_add(u2, u1, -cos)).unwrap_or([0.0, 0.0]);
                let w2 = c_unit(c_add(u1, u2, -cos)).unwrap_or([0.0, 0.0]);
                let c = 4.0 / 3.0 * (theta * 0.25).tan() * d;
                a.handle_out = Some(c_add(p1, w1, c));
                b.handle_in = Some(c_add(p2, w2, c));
                points.extend([a, b]);
            }
            "cutout" => {
                let inner = PathPoint::sharp(fresh(), c_add(p1, u2, d));
                points.extend([a, inner, b]);
            }
            _ => points.extend([a, b]),
        }
    }
    Contour { points, closed: contour.closed }
}

fn corner_name(kind: CornerKind) -> &'static str {
    match kind {
        CornerKind::None => "none",
        CornerKind::Rounded => "rounded",
        CornerKind::Straight => "straight",
        CornerKind::Concave => "concave",
        CornerKind::Cutout => "cutout",
    }
}

pub fn cornered_contour(contour: &Contour) -> Contour {
    let corners: Vec<Option<(&str, f64)>> = contour
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| p.corner.map(|c| (corner_name(c.kind), crate::curve::effective_radius(contour, i))))
        .collect();
    if corners.iter().all(|c| c.is_none_or(|(kind, r)| kind == "none" || r <= 0.0)) {
        contour.clone()
    } else {
        apply_corners(contour, &corners)
    }
}

pub fn cornered(curve: &Curve) -> Curve {
    Curve { contour: cornered_contour(&curve.contour), holes: curve.holes.iter().map(cornered_contour).collect(), source_object: curve.source_object }
}

#[cfg(test)]
mod contracts {
    use super::*;
    use crate::shape::{ArrowRecipe, CalloutRectangleRecipe, CogRecipe, DiamondRecipe, DonutRecipe, PieRecipe, PolygonRecipe, ShapeRecipe, SquareStarRecipe, TrapezoidRecipe, TriangleRecipe};

    fn recipe(kind: ShapeKind) -> ShapeRecipe {
        ShapeRecipe { kind, size: [100.0, 50.0], corner: None, triangle: None, star: None, arrow: None, callout_ellipse: None, callout_rectangle: None, donut: None, pie: None, heart: None, cog: None, tear: None, crescent: None, cloud: None, segment: None, cat: None, square_star: None, double_star: None, diamond: None, trapezoid: None, polygon: None, spiral: None }
    }

    #[test]
    fn rect_ellipse_polygon_outlines_fit_size() {
        let mut object = Object { size: [100.0, 50.0], ..Object::default() };
        let rect = object_outline(&object).unwrap();
        assert_eq!(crate::geometry::bez_path_bounds(&rect), [0.0, 0.0, 100.0, 50.0]);
        object.shape = Some(recipe(ShapeKind::Ellipse));
        let ellipse = crate::geometry::bez_path_bounds(&object_outline(&object).unwrap());
        assert!((ellipse[2] - 100.0).abs() < 0.5 && (ellipse[3] - 50.0).abs() < 0.5);
        let mut polygon = recipe(ShapeKind::Polygon);
        polygon.polygon = Some(PolygonRecipe { number_of_sides: 6, curve: 0.0, smooth_points: false });
        object.shape = Some(polygon);
        let flat = flatten_subpaths(&object_outline(&object).unwrap(), 0.25);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].0.len(), 6);
        assert!(flat[0].1);
        object.shape = Some(recipe(ShapeKind::Heart));
        assert!(object_outline(&object).is_none());
    }

    #[test]
    fn world_outline_applies_pos() {
        let object = Object { pos: [10.0, 20.0], size: [4.0, 2.0], ..Object::default() };
        let bounds = crate::geometry::bez_path_bounds(&object_outline_world(&object).unwrap());
        assert_eq!(bounds, [10.0, 20.0, 14.0, 22.0]);
    }

    #[test]
    fn remaining_shape_kinds_fill_bounds() {
        let mut object = Object { size: [100.0, 50.0], ..Object::default() };
        let mut triangle = recipe(ShapeKind::Triangle);
        triangle.triangle = Some(TriangleRecipe { points: [0.5, 1.0, 0.5] });
        let mut arrow = recipe(ShapeKind::Arrow);
        arrow.arrow = Some(ArrowRecipe { head_width: 50.0, head_length: 50.0, shaft_width: 25.0, shaft_length: 50.0, tail_width: 25.0 });
        let mut diamond = recipe(ShapeKind::Diamond);
        diamond.diamond = Some(DiamondRecipe { indent: 50.0 });
        let mut trapezoid = recipe(ShapeKind::Trapezoid);
        trapezoid.trapezoid = Some(TrapezoidRecipe { indent: 25.0 });
        let mut donut = recipe(ShapeKind::Donut);
        donut.donut = Some(DonutRecipe { inner_radius: 50.0, outer_radius: 100.0 });
        let mut pie = recipe(ShapeKind::Pie);
        pie.pie = Some(PieRecipe { start_angle: 0.0, end_angle: 360.0, inner_radius: 0.0, outer_radius: 100.0 });
        let mut square = recipe(ShapeKind::SquareStar);
        square.square_star = Some(SquareStarRecipe { number_of_sides: 4, cutout: 50.0 });
        for shape in [triangle, arrow, diamond, trapezoid, donut, pie, square] {
            let kind = shape.kind;
            object.shape = Some(shape);
            let bounds = crate::geometry::bez_path_bounds(&object_outline(&object).unwrap());
            let expected = [0.0, 0.0, 100.0, 50.0];
            assert!(bounds.iter().zip(expected).all(|(got, want)| (got - want).abs() < 0.5), "{kind:?} {bounds:?}");
        }
        let mut cog = recipe(ShapeKind::Cog);
        cog.cog = Some(CogRecipe { number_of_teeth: 12, tooth_depth: 10.0, tooth_width: 50.0, inner_radius: 40.0, outer_radius: 50.0 });
        object.shape = Some(cog);
        assert_eq!(flatten_subpaths(&object_outline(&object).unwrap(), 0.25)[0].0.len(), 48);
        let mut callout = recipe(ShapeKind::CalloutRectangle);
        callout.callout_rectangle = Some(CalloutRectangleRecipe { corner_radius: 0.0, absolute_radii: false, lock_radii: true, pointer_angle: 45.0, pointer_length: 20.0, pointer_width: 10.0 });
        object.shape = Some(callout);
        assert!(crate::geometry::bez_path_bounds(&object_outline(&object).unwrap())[3] > 50.0);
    }

    #[test]
    fn dimension_geometry_offsets_line_and_labels_length() {
        let dimension = Dimension { start: [0.0, 0.0], end: [30.0, 0.0], offset: 5.0, length: 30.0, units: "px".to_string() };
        let object = Object { pos: [10.0, 10.0], size: [30.0, 0.0], kind: ObjectKind::Dimension(dimension), ..Object::default() };
        let geometry = dimension_geometry(&object).unwrap();
        assert_eq!(geometry.line, [[10.0, 15.0], [40.0, 15.0]]);
        assert_eq!(geometry.extensions.len(), 2);
        assert_eq!(geometry.label, "30 px");
        assert_eq!(geometry.label_angle_deg, 0.0);
        assert!(geometry.label_at[1] > 15.0);
    }
}
