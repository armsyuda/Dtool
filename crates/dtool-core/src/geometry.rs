use kurbo::{BezPath, Point, Rect, Shape};
use dtool_protocol::CommandError;

pub fn rect_to_bez_path(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to(Point::new(x, y));
    path.line_to(Point::new(x + w, y));
    path.line_to(Point::new(x + w, y + h));
    path.line_to(Point::new(x, y + h));
    path.close_path();
    path
}

pub fn ellipse_to_bez_path(cx: f64, cy: f64, rx: f64, ry: f64) -> BezPath {
    let mut path = BezPath::new();
    let k = 0.552284749831793;
    let x0 = cx - rx;
    let y0 = cy - ry;
    let x1 = cx + rx;
    let y1 = cy + ry;

    path.move_to(Point::new(cx, y0));
    path.curve_to(
        Point::new(cx + k * rx, y0),
        Point::new(x1, cy - k * ry),
        Point::new(x1, cy),
    );
    path.curve_to(
        Point::new(x1, cy + k * ry),
        Point::new(cx + k * rx, y1),
        Point::new(cx, y1),
    );
    path.curve_to(
        Point::new(cx - k * rx, y1),
        Point::new(x0, cy + k * ry),
        Point::new(x0, cy),
    );
    path.curve_to(
        Point::new(x0, cy - k * ry),
        Point::new(cx - k * rx, y0),
        Point::new(cx, y0),
    );
    path.close_path();
    path
}

pub fn rounded_rect_to_bez_path(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius_tl: f64,
    radius_tr: f64,
    radius_br: f64,
    radius_bl: f64,
) -> BezPath {
    let mut path = BezPath::new();
    let k = 0.552284749831793;

    let x0 = x;
    let y0 = y;
    let x1 = x + w;
    let y1 = y + h;

    path.move_to(Point::new(x0 + radius_tl, y0));
    path.line_to(Point::new(x1 - radius_tr, y0));

    if radius_tr > 0.0 {
        path.curve_to(
            Point::new(x1 - radius_tr + k * radius_tr, y0),
            Point::new(x1, y0 + radius_tr - k * radius_tr),
            Point::new(x1, y0 + radius_tr),
        );
    }

    path.line_to(Point::new(x1, y1 - radius_br));

    if radius_br > 0.0 {
        path.curve_to(
            Point::new(x1, y1 - radius_br + k * radius_br),
            Point::new(x1 - radius_br + k * radius_br, y1),
            Point::new(x1 - radius_br, y1),
        );
    }

    path.line_to(Point::new(x0 + radius_bl, y1));

    if radius_bl > 0.0 {
        path.curve_to(
            Point::new(x0 + radius_bl - k * radius_bl, y1),
            Point::new(x0, y1 - radius_bl + k * radius_bl),
            Point::new(x0, y1 - radius_bl),
        );
    }

    path.line_to(Point::new(x0, y0 + radius_tl));

    if radius_tl > 0.0 {
        path.curve_to(
            Point::new(x0, y0 + radius_tl - k * radius_tl),
            Point::new(x0 + radius_tl - k * radius_tl, y0),
            Point::new(x0 + radius_tl, y0),
        );
    }

    path.close_path();
    path
}

pub fn bez_path_bounds(path: &BezPath) -> [f32; 4] {
    let rect = path.bounding_box();
    [
        rect.x0 as f32,
        rect.y0 as f32,
        rect.x1 as f32,
        rect.y1 as f32,
    ]
}

pub fn transform_bez_path(
    path: &BezPath,
    tx: f64,
    ty: f64,
    scale_x: f64,
    scale_y: f64,
    rotation_rad: f64,
) -> BezPath {
    let cos_r = rotation_rad.cos();
    let sin_r = rotation_rad.sin();

    let mut transformed = BezPath::new();

    for el in path.elements() {
        match el {
            kurbo::PathEl::MoveTo(p) => {
                transformed.move_to(transform_point(*p, tx, ty, scale_x, scale_y, cos_r, sin_r));
            }
            kurbo::PathEl::LineTo(p) => {
                transformed.line_to(transform_point(*p, tx, ty, scale_x, scale_y, cos_r, sin_r));
            }
            kurbo::PathEl::QuadTo(c, p) => {
                transformed.quad_to(
                    transform_point(*c, tx, ty, scale_x, scale_y, cos_r, sin_r),
                    transform_point(*p, tx, ty, scale_x, scale_y, cos_r, sin_r),
                );
            }
            kurbo::PathEl::CurveTo(c1, c2, p) => {
                transformed.curve_to(
                    transform_point(*c1, tx, ty, scale_x, scale_y, cos_r, sin_r),
                    transform_point(*c2, tx, ty, scale_x, scale_y, cos_r, sin_r),
                    transform_point(*p, tx, ty, scale_x, scale_y, cos_r, sin_r),
                );
            }
            kurbo::PathEl::ClosePath => {
                transformed.close_path();
            }
        }
    }

    transformed
}

fn transform_point(
    p: Point,
    tx: f64,
    ty: f64,
    scale_x: f64,
    scale_y: f64,
    cos_r: f64,
    sin_r: f64,
) -> Point {
    let sx = p.x * scale_x;
    let sy = p.y * scale_y;
    let rx = sx * cos_r - sy * sin_r;
    let ry = sx * sin_r + sy * cos_r;
    Point::new(rx + tx, ry + ty)
}

pub fn path_length(path: &BezPath) -> f64 {
    path.perimeter(1e-6)
}

pub fn gradient_line_from_legacy_angle(angle: f64, bounds: [f64; 4]) -> ([f64; 2], [f64; 2]) {
    if !angle.is_finite() || bounds.iter().any(|v| !v.is_finite()) {return ([0.0;2],[0.0;2]);}
    let [x,y,w,h]=bounds;
    let center=[x+w*0.5,y+h*0.5];
    if center.iter().any(|v| !v.is_finite()) {return ([0.0;2],[0.0;2]);}
    let angle=angle.rem_euclid(360.0);
    let direction=match angle {
        0.0=>[1.0,0.0],45.0=>[1.0,-1.0],90.0=>[0.0,-1.0],135.0=>[-1.0,-1.0],
        180.0=>[-1.0,0.0],225.0=>[-1.0,1.0],270.0=>[0.0,1.0],315.0=>[1.0,1.0],
        _=>{let radians=angle.to_radians();[radians.cos(),-radians.sin()]},
    };
    let tx=if direction[0]==0.0 {f64::INFINITY} else {w.abs()*0.5/direction[0].abs()};
    let ty=if direction[1]==0.0 {f64::INFINITY} else {h.abs()*0.5/direction[1].abs()};
    let t=tx.min(ty);
    ([center[0]-t*direction[0],center[1]-t*direction[1]],[center[0]+t*direction[0],center[1]+t*direction[1]])
}

#[cfg(test)]
mod gradient_geometry_contracts {
    use super::*;
    #[test]
    fn exact_legacy_angles_through_bounds_center() {
        let bounds=[0.0,0.0,100.0,50.0];
        for (angle,start,end) in [(0.0,[0.0,25.0],[100.0,25.0]),(45.0,[25.0,50.0],[75.0,0.0]),(90.0,[50.0,50.0],[50.0,0.0]),(180.0,[100.0,25.0],[0.0,25.0])] {
            assert_eq!(gradient_line_from_legacy_angle(angle,bounds),(start,end));
            let geom=crate::paint::GradientGeom::from_legacy_angle(angle,bounds);assert_eq!((geom.start,geom.end),(start,end));
        }
    }
    #[test]
    fn translated_reversed_and_degenerate_bounds() {
        assert_eq!(gradient_line_from_legacy_angle(405.0,[10.0,20.0,100.0,50.0]),([35.0,70.0],[85.0,20.0]));
        assert_eq!(gradient_line_from_legacy_angle(0.0,[100.0,50.0,-100.0,-50.0]),([0.0,25.0],[100.0,25.0]));
        assert_eq!(gradient_line_from_legacy_angle(90.0,[5.0,10.0,0.0,40.0]),([5.0,50.0],[5.0,10.0]));
    }
}

pub const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub fn object_linear(rotation_deg: f32, shear_deg: f32) -> [f32; 4] {
    let (sin, cos) = rotation_deg.to_radians().sin_cos();
    let tan = shear_deg.to_radians().tan();
    [cos, sin, cos * tan - sin, sin * tan + cos]
}

pub fn box_centre(obj: &crate::object::Object) -> [f32; 2] {
    [obj.pos[0] + obj.size[0].abs() * 0.5, obj.pos[1] + obj.size[1].abs() * 0.5]
}

pub fn object_matrix(obj: &crate::object::Object) -> [f32; 6] {
    let flip = [if obj.size[0] < 0.0 { -1.0 } else { 1.0 }, if obj.size[1] < 0.0 { -1.0 } else { 1.0 }];
    if obj.rotation_deg == 0.0 && obj.shear_deg == 0.0 && flip == [1.0, 1.0] {
        return IDENTITY;
    }
    let [l0, l1, l2, l3] = object_linear(obj.rotation_deg, obj.shear_deg);
    let (a, b, c, d) = (l0 * flip[0], l1 * flip[0], l2 * flip[1], l3 * flip[1]);
    let [cx, cy] = box_centre(obj);
    [a, b, c, d, cx - (a * cx + c * cy), cy - (b * cx + d * cy)]
}

/// Map shaped text coordinates, which already include the layer origin, into the document.
pub fn text_matrix(object: &crate::Object) -> [f32; 6] {
    if object.is_path_text() {
        return object.path_text_linear.map_or(IDENTITY, |[a, b, c, d]| [a, b, c, d, 0.0, 0.0]);
    }
    if !matches!(object.kind, crate::ObjectKind::ArtText { .. } | crate::ObjectKind::FrameText { .. }) {
        return IDENTITY;
    }
    let [sx, sy] = object.text_scale;
    let [x, y] = object.pos;
    let scale = [sx, 0.0, 0.0, sy, x * (1.0 - sx), y * (1.0 - sy)];
    let matrix = compose(&object_matrix(object), &scale);
    if object.is_vertical_art_text() {
        let column_width = object.size[0].abs() / sx.abs().max(f32::EPSILON);
        compose(&matrix, &[0.0, 1.0, -1.0, 0.0, x + y + column_width, y - x])
    } else {
        matrix
    }
}

pub fn rect_quad([x, y, width, height]: [f32; 4], matrix: &[f32; 6]) -> [[f32; 2]; 4] {
    [[x, y], [x + width, y], [x + width, y + height], [x, y + height]]
        .map(|point| apply_matrix(matrix, point))
}

pub fn apply_matrix(m: &[f32; 6], p: [f32; 2]) -> [f32; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

pub fn invert_matrix(m: &[f32; 6]) -> Option<[f32; 6]> {
    if m.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let det = m[0] as f64 * m[3] as f64 - m[1] as f64 * m[2] as f64;
    if det.abs() <= 1e-12 {
        return None;
    }
    let (a, b, c, d, e, f) = (m[0] as f64, m[1] as f64, m[2] as f64, m[3] as f64, m[4] as f64, m[5] as f64);
    let inverse = [d / det, -b / det, -c / det, a / det, (c * f - d * e) / det, (b * e - a * f) / det];
    Some(inverse.map(|value| value as f32))
}

pub fn compose(outer: &[f32; 6], inner: &[f32; 6]) -> [f32; 6] {
    [
        outer[0] * inner[0] + outer[2] * inner[1],
        outer[1] * inner[0] + outer[3] * inner[1],
        outer[0] * inner[2] + outer[2] * inner[3],
        outer[1] * inner[2] + outer[3] * inner[3],
        outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
        outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
    ]
}

pub fn object_world_quad(obj: &crate::object::Object) -> [[f32; 2]; 4] {
    let x0 = obj.pos[0];
    let y0 = obj.pos[1];
    let x1 = x0 + obj.size[0].abs();
    let y1 = y0 + obj.size[1].abs();
    let m = object_matrix(obj);
    [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| apply_matrix(&m, p))
}

pub fn object_world_bounds(obj: &crate::object::Object) -> [f32; 4] {
    let quad = object_world_quad(obj);
    let min_x = quad.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let min_y = quad.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_x = quad.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
    let max_y = quad.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
    [min_x, min_y, max_x - min_x, max_y - min_y]
}

/// Return the left, top, right and bottom of a finite point set.
pub(crate) fn points_bounds(points: &[[f32; 2]]) -> Option<[f32; 4]> {
    let &[x, y] = points.first()?;
    points.iter().try_fold([x, y, x, y], |b, &[x, y]| {
        (x.is_finite() && y.is_finite()).then(|| [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)])
    })
}

pub(crate) fn clip_convex_polygon(subject: Vec<[f32; 2]>, clip: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let Some(&origin) = clip.first() else { return Vec::new() };
    let area: f64 = (0..clip.len()).map(|i| {
        let a = clip[i];
        let b = clip[(i + 1) % clip.len()];
        let x = f64::from(a[0] - origin[0]);
        let y = f64::from(a[1] - origin[1]);
        x * f64::from(b[1] - origin[1]) - y * f64::from(b[0] - origin[0])
    }).sum();
    if !area.is_finite() || area == 0.0 {
        return Vec::new();
    }
    let sign = area.signum() as f32;
    let side = |a: [f32; 2], b: [f32; 2], p: [f32; 2]| sign * ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]));
    let mut out = subject;
    for i in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let a = clip[i];
        let b = clip[(i + 1) % clip.len()];
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let p = input[j];
            let q = input[(j + 1) % input.len()];
            let sp = side(a, b, p);
            let sq = side(a, b, q);
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }
    out
}

pub fn doc_to_object_local(obj: &crate::object::Object, p: [f32; 2]) -> Option<[f32; 2]> {
    invert_matrix(&object_matrix(obj)).map(|inverse| apply_matrix(&inverse, p))
}

pub fn wrap_degrees(value: f32) -> f32 {
    let wrapped = value.rem_euclid(360.0);
    if wrapped > 180.0 { wrapped - 360.0 } else { wrapped }
}
