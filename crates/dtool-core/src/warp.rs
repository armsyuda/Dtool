use crate::object::ObjectId as NodeId;
use kurbo::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarpPreset {
    Mesh,
    Quad,
    Perspective,
    ArcVertical,
    ArcHorizontal,
    BendVertical,
    BendHorizontal,
    FishEye,
    Twist,
    Arch,
    UpperArc,
    LowerArc,
    Bulge,
    Flag,
    Rise,
    Wave,
    Pinch,
    Twirl,
    Spiral,
}

pub const OFFERED_PRESETS: [WarpPreset; 15] = [
    WarpPreset::Mesh,
    WarpPreset::Quad,
    WarpPreset::Perspective,
    WarpPreset::ArcVertical,
    WarpPreset::ArcHorizontal,
    WarpPreset::BendVertical,
    WarpPreset::BendHorizontal,
    WarpPreset::FishEye,
    WarpPreset::Twist,
    WarpPreset::Arch,
    WarpPreset::UpperArc,
    WarpPreset::LowerArc,
    WarpPreset::Bulge,
    WarpPreset::Flag,
    WarpPreset::Rise,
];

pub const PENDING_PRESETS: [WarpPreset; 4] = [
    WarpPreset::Wave,
    WarpPreset::Pinch,
    WarpPreset::Twirl,
    WarpPreset::Spiral,
];

pub fn preset_slug(preset: &WarpPreset) -> &'static str {
    match preset {
        WarpPreset::Mesh => "mesh",
        WarpPreset::Quad => "quad",
        WarpPreset::Perspective => "perspective",
        WarpPreset::ArcVertical => "arc_vertical",
        WarpPreset::ArcHorizontal => "arc_horizontal",
        WarpPreset::BendVertical => "bend_vertical",
        WarpPreset::BendHorizontal => "bend_horizontal",
        WarpPreset::FishEye => "fish_eye",
        WarpPreset::Twist => "twist",
        WarpPreset::Arch => "arch",
        WarpPreset::UpperArc => "upper_arc",
        WarpPreset::LowerArc => "lower_arc",
        WarpPreset::Bulge => "bulge",
        WarpPreset::Flag => "flag",
        WarpPreset::Rise => "rise",
        WarpPreset::Wave => "wave",
        WarpPreset::Pinch => "pinch",
        WarpPreset::Twirl => "twirl",
        WarpPreset::Spiral => "spiral",
    }
}

pub fn preset_display_name(preset: &WarpPreset) -> &'static str {
    match preset {
        WarpPreset::Mesh => "Mesh",
        WarpPreset::Quad => "Quad",
        WarpPreset::Perspective => "Perspective",
        WarpPreset::ArcVertical => "Arc - Vertical",
        WarpPreset::ArcHorizontal => "Arc - Horizontal",
        WarpPreset::BendVertical => "Bend - Vertical",
        WarpPreset::BendHorizontal => "Bend - Horizontal",
        WarpPreset::FishEye => "Fish Eye",
        WarpPreset::Twist => "Twist",
        WarpPreset::Arch => "Arch",
        WarpPreset::UpperArc => "Upper Arc",
        WarpPreset::LowerArc => "Lower Arc",
        WarpPreset::Bulge => "Bulge",
        WarpPreset::Flag => "Flag",
        WarpPreset::Rise => "Rise",
        WarpPreset::Wave => "Wave",
        WarpPreset::Pinch => "Pinch",
        WarpPreset::Twirl => "Twirl",
        WarpPreset::Spiral => "Spiral",
    }
}

pub fn is_offered_preset(preset: WarpPreset) -> bool {
    OFFERED_PRESETS.contains(&preset)
}

pub fn is_pending_preset(preset: WarpPreset) -> bool {
    PENDING_PRESETS.contains(&preset)
}

fn preset_key(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn parse_preset(name: &str) -> Option<WarpPreset> {
    let key = preset_key(name);
    Some(match key.as_str() {
        "mesh" => WarpPreset::Mesh,
        "quad" => WarpPreset::Quad,
        "perspective" => WarpPreset::Perspective,
        "arcvertical" | "arcv" => WarpPreset::ArcVertical,
        "archorizontal" => WarpPreset::ArcHorizontal,
        "arch" | "arc" => WarpPreset::Arch,
        "bendvertical" | "bendv" => WarpPreset::BendVertical,
        "bendhorizontal" | "bendh" => WarpPreset::BendHorizontal,
        "fisheye" => WarpPreset::FishEye,
        "twist" => WarpPreset::Twist,
        "upperarc" => WarpPreset::UpperArc,
        "lowerarc" => WarpPreset::LowerArc,
        "bulge" => WarpPreset::Bulge,
        "flag" => WarpPreset::Flag,
        "rise" => WarpPreset::Rise,
        "wave" => WarpPreset::Wave,
        "pinch" => WarpPreset::Pinch,
        "twirl" => WarpPreset::Twirl,
        "spiral" => WarpPreset::Spiral,
        _ => return None,
    })
}


#[derive(Clone, Debug, PartialEq)]
pub struct MeshPoint {
    pub id: u64,
    pub pos: [f64; 2],
    pub handles: [[f64; 2]; 4],
}

#[derive(Clone, Debug)]
pub struct WarpMesh {
    pub cols: u32,
    pub rows: u32,
    pub points: Vec<MeshPoint>,
    pub(crate) column_knots: Vec<f64>,
    pub(crate) row_knots: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct WarpGroup {
    pub id: NodeId,
    pub preset: WarpPreset,
    pub value: f64,
    pub mesh: WarpMesh,
    pub muted: bool,
    pub children: Vec<NodeId>,
}

#[derive(Clone, Debug)]
pub struct PathData {
    pub points: Vec<[f64; 2]>,
}

pub const HANDLE_LEFT: usize = 0;
pub const HANDLE_RIGHT: usize = 1;
pub const HANDLE_UP: usize = 2;
pub const HANDLE_DOWN: usize = 3;
pub const MESH_EPSILON: f64 = 1e-9;

pub fn bounds_rect(bounds: [f64; 4]) -> Rect {
    Rect::new(bounds[0], bounds[1], bounds[2], bounds[3])
}

pub fn identity_mesh(cols: u32, rows: u32, bounds: Rect) -> WarpMesh {
    let cols = cols.max(2);
    let rows = rows.max(2);
    let bounds = bounds.abs();
    let mut points = Vec::with_capacity(cols as usize * rows as usize);
    for r in 0..rows {
        for c in 0..cols {
            let x = bounds.x0 + bounds.width() * f64::from(c) / f64::from(cols - 1);
            let y = bounds.y0 + bounds.height() * f64::from(r) / f64::from(rows - 1);
            points.push(MeshPoint {
                id: u64::from(r) * u64::from(cols) + u64::from(c),
                pos: [x, y],
                handles: [[x, y]; 4],
            });
        }
    }
    WarpMesh { cols, rows, points, column_knots: Vec::new(), row_knots: Vec::new() }
}

fn finite2(v: [f64; 2]) -> bool {
    v[0].is_finite() && v[1].is_finite()
}

fn lerp2(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

pub fn edge_control(from: &MeshPoint, handle: usize, to: &MeshPoint) -> [f64; 2] {
    let h = from.handles[handle];
    let set = finite2(h) && ((h[0] - from.pos[0]).abs() > MESH_EPSILON || (h[1] - from.pos[1]).abs() > MESH_EPSILON);
    if set {
        h
    } else {
        lerp2(from.pos, to.pos, 1.0 / 3.0)
    }
}

fn cubic(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    let a = u * u * u;
    let b = 3.0 * u * u * t;
    let c = 3.0 * u * t * t;
    let d = t * t * t;
    [
        a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
        a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
    ]
}

fn edge(from: &MeshPoint, from_handle: usize, to: &MeshPoint, to_handle: usize, t: f64) -> [f64; 2] {
    cubic(from.pos, edge_control(from, from_handle, to), edge_control(to, to_handle, from), to.pos, t)
}

pub fn mesh_is_valid(mesh: &WarpMesh) -> bool {
    mesh.cols >= 2
        && mesh.rows >= 2
        && (mesh.cols as usize).checked_mul(mesh.rows as usize).is_some_and(|n| n <= mesh.points.len())
        && mesh.points.iter().all(|pt| finite2(pt.pos))
        && valid_knots(&mesh.column_knots, mesh.cols)
        && valid_knots(&mesh.row_knots, mesh.rows)
}

fn valid_knots(knots: &[f64], count: u32) -> bool {
    knots.is_empty() || (knots.len() == count as usize
        && knots.first() == Some(&0.0) && knots.last() == Some(&1.0)
        && knots.windows(2).all(|pair| pair[0] < pair[1]))
}

fn locate_cell(value: f64, count: u32, knots: &[f64]) -> (usize, f64) {
    let last = count as usize - 2;
    if knots.is_empty() {
        let scaled = value * f64::from(count - 1);
        let cell = (scaled.floor() as usize).min(last);
        return (cell, (scaled - cell as f64).clamp(0.0, 1.0));
    }
    let cell = knots.partition_point(|&knot| knot <= value).saturating_sub(1).min(last);
    let fraction = (value - knots[cell]) / (knots[cell + 1] - knots[cell]);
    (cell, fraction.clamp(0.0, 1.0))
}

pub fn evaluate_patch(mesh: &WarpMesh, col: usize, row: usize, s: f64, t: f64) -> Option<[f64; 2]> {
    let cols = mesh.cols as usize;
    let rows = mesh.rows as usize;
    if cols < 2 || rows < 2 || col + 1 >= cols || row + 1 >= rows {
        return None;
    }
    let at = |r: usize, c: usize| mesh.points.get(r * cols + c);
    let tl = at(row, col)?;
    let tr = at(row, col + 1)?;
    let bl = at(row + 1, col)?;
    let br = at(row + 1, col + 1)?;
    let top = edge(tl, HANDLE_RIGHT, tr, HANDLE_LEFT, s);
    let bottom = edge(bl, HANDLE_RIGHT, br, HANDLE_LEFT, s);
    let left = edge(tl, HANDLE_DOWN, bl, HANDLE_UP, t);
    let right = edge(tr, HANDLE_DOWN, br, HANDLE_UP, t);
    let mut out = [0.0; 2];
    for k in 0..2 {
        let ruled_s = (1.0 - t) * top[k] + t * bottom[k];
        let ruled_t = (1.0 - s) * left[k] + s * right[k];
        let corners = (1.0 - s) * (1.0 - t) * tl.pos[k]
            + s * (1.0 - t) * tr.pos[k]
            + (1.0 - s) * t * bl.pos[k]
            + s * t * br.pos[k];
        out[k] = ruled_s + ruled_t - corners;
    }
    finite2(out).then_some(out)
}

pub trait WarpBounds { fn to_rect(self) -> Rect; }
impl WarpBounds for Rect { fn to_rect(self) -> Rect { self } }
impl WarpBounds for [f64;4] { fn to_rect(self) -> Rect { bounds_rect(self) } }

pub fn evaluate(mesh: &WarpMesh, bounds: impl WarpBounds, p: [f64; 2]) -> [f64; 2] {
    let bounds=bounds.to_rect();
    if !finite2(p) || !mesh_is_valid(mesh) {
        return p;
    }
    let bounds = bounds.abs();
    let width = bounds.width();
    let height = bounds.height();
    if !width.is_finite() || !height.is_finite() || width <= MESH_EPSILON || height <= MESH_EPSILON {
        return p;
    }
    let inside = [p[0].clamp(bounds.x0, bounds.x1), p[1].clamp(bounds.y0, bounds.y1)];
    let outside = [p[0] - inside[0], p[1] - inside[1]];
    let (col, s) = locate_cell((inside[0] - bounds.x0) / width, mesh.cols, &mesh.column_knots);
    let (row, t) = locate_cell((inside[1] - bounds.y0) / height, mesh.rows, &mesh.row_knots);
    match evaluate_patch(mesh, col, row, s, t) {
        Some(q) => [q[0] + outside[0], q[1] + outside[1]],
        None => p,
    }
}

pub fn warp_path(mesh: &WarpMesh, bounds: impl WarpBounds, path: &PathData) -> PathData {
    let bounds=bounds.to_rect();
    let new_points = path.points.iter().map(|p| evaluate(mesh, bounds, *p)).collect();
    PathData { points: new_points }
}

pub type Quad = [[f64; 2]; 4];

#[derive(Clone, Debug)]
pub struct PerspectiveSpec {
    pub planes: u8,
    pub source: Vec<Quad>,
    pub target: Vec<Quad>,
}

pub fn homography(src: Quad, dst: Quad) -> [[f64; 3]; 3] {
    let mut a = [[0.0; 8]; 8];
    let mut b = [0.0; 8];

    for i in 0..4 {
        let sx = src[i][0];
        let sy = src[i][1];
        let dx = dst[i][0];
        let dy = dst[i][1];

        a[i * 2] = [sx, sy, 1.0, 0.0, 0.0, 0.0, -dx * sx, -dx * sy];
        b[i * 2] = dx;

        a[i * 2 + 1] = [0.0, 0.0, 0.0, sx, sy, 1.0, -dy * sx, -dy * sy];
        b[i * 2 + 1] = dy;
    }

    let h = solve_linear_system(a, b);

    [
        [h[0], h[1], h[2]],
        [h[3], h[4], h[5]],
        [h[6], h[7], 1.0],
    ]
}

fn solve_linear_system(mut a: [[f64; 8]; 8], mut b: [f64; 8]) -> [f64; 8] {
    for i in 0..8 {
        let mut max_row = i;
        for k in (i + 1)..8 {
            if a[k][i].abs() > a[max_row][i].abs() {
                max_row = k;
            }
        }

        if max_row != i {
            a.swap(i, max_row);
            b.swap(i, max_row);
        }

        let pivot = a[i][i];
        if pivot.abs() < 1e-12 {
            continue;
        }

        for j in (i + 1)..8 {
            let factor = a[j][i] / pivot;
            for k in i..8 {
                a[j][k] -= factor * a[i][k];
            }
            b[j] -= factor * b[i];
        }
    }

    let mut x = [0.0; 8];
    for i in (0..8).rev() {
        let mut sum = b[i];
        for j in (i + 1)..8 {
            sum -= a[i][j] * x[j];
        }
        if a[i][i].abs() > 1e-12 {
            x[i] = sum / a[i][i];
        }
    }

    x
}

pub fn preset_default_value(preset: &WarpPreset) -> f64 {
    match preset {
        WarpPreset::Mesh | WarpPreset::Quad => 0.0,
        _ => 25.0,
    }
}

pub fn preset_mesh_dims(preset: &WarpPreset) -> (u32, u32) {
    match preset {
        WarpPreset::Quad | WarpPreset::Perspective => (2, 2),
        _ => (4, 4),
    }
}

pub fn create_mesh_for_preset(preset: &WarpPreset, bounds: Rect) -> WarpMesh {
    let (cols, rows) = preset_mesh_dims(preset);
    preset_mesh(preset, &identity_mesh(cols, rows, bounds), bounds, preset_default_value(preset))
}
pub const PRESET_VALUE_LIMIT: f64 = 100.0;

pub fn preset_value_is_valid(value: f64) -> bool {
    value.is_finite() && value.abs() <= PRESET_VALUE_LIMIT
}

fn peak(bounds: Rect, value: f64, axis_is_x: bool) -> f64 {
    let span = if axis_is_x { bounds.width() } else { bounds.height() };
    value / PRESET_VALUE_LIMIT * span * 0.5
}

fn reset_handles(point: &mut MeshPoint) {
    point.handles = [point.pos; 4];
}

fn identity_then(cols: u32, rows: u32, bounds: Rect, value: f64, map: impl Fn(f64, f64, [f64; 2], f64, Rect) -> [f64; 2]) -> WarpMesh {
    let mut mesh = identity_mesh(cols, rows, bounds);
    let bounds = bounds.abs();
    let width = bounds.width();
    let height = bounds.height();
    if !preset_value_is_valid(value) || !width.is_finite() || !height.is_finite() || width <= MESH_EPSILON || height <= MESH_EPSILON {
        return mesh;
    }
    for point in mesh.points.iter_mut() {
        let u = (point.pos[0] - bounds.x0) / width;
        let v = (point.pos[1] - bounds.y0) / height;
        point.pos = map(u, v, point.pos, value, bounds);
        reset_handles(point);
    }
    mesh
}

pub fn arch_mesh(cols: u32, rows: u32, bounds: Rect, value: f64) -> WarpMesh {
    arch_like_mesh(cols, rows, bounds, value, 1.0)
}

fn arch_like_mesh(cols: u32, rows: u32, bounds: Rect, value: f64, v_weight: f64) -> WarpMesh {
    let mut mesh = identity_mesh(cols, rows, bounds);
    let bounds = bounds.abs();
    let width = bounds.width();
    let height = bounds.height();
    if !preset_value_is_valid(value) || !width.is_finite() || !height.is_finite() || width <= MESH_EPSILON {
        return mesh;
    }
    let peak = value / PRESET_VALUE_LIMIT * height * 0.5;
    if peak.abs() <= MESH_EPSILON {
        return mesh;
    }
    let cols_n = mesh.cols as usize;
    let rows_n = mesh.rows as usize;
    let reach = width / f64::from(mesh.cols - 1) / 3.0;
    for (index, point) in mesh.points.iter_mut().enumerate() {
        let col = index % cols_n;
        let row = index / cols_n;
        let u = (point.pos[0] - bounds.x0) / width;
        let v = if rows_n <= 1 { 0.0 } else { row as f64 / (rows_n as f64 - 1.0) };
        let weight = if v_weight < 0.0 {
            v
        } else if v_weight > 1.0 {
            1.0 - v
        } else {
            v_weight
        };
        let lift = peak * 4.0 * u * (1.0 - u) * weight;
        let slope = -peak * 4.0 * (1.0 - 2.0 * u) / width * weight;
        let pos = [point.pos[0], point.pos[1] - lift];
        point.pos = pos;
        point.handles = [pos; 4];
        if col > 0 {
            point.handles[HANDLE_LEFT] = [pos[0] - reach, pos[1] - slope * reach];
        }
        if col + 1 < cols_n {
            point.handles[HANDLE_RIGHT] = [pos[0] + reach, pos[1] + slope * reach];
        }
    }
    mesh
}

fn horizontal_arch_mesh(cols: u32, rows: u32, bounds: Rect, value: f64) -> WarpMesh {
    let mut mesh = identity_mesh(cols, rows, bounds);
    let bounds = bounds.abs();
    let width = bounds.width();
    let height = bounds.height();
    if !preset_value_is_valid(value) || !width.is_finite() || !height.is_finite() || height <= MESH_EPSILON {
        return mesh;
    }
    let peak = value / PRESET_VALUE_LIMIT * width * 0.5;
    if peak.abs() <= MESH_EPSILON {
        return mesh;
    }
    let cols_n = mesh.cols as usize;
    let reach = height / f64::from(mesh.rows - 1) / 3.0;
    for (index, point) in mesh.points.iter_mut().enumerate() {
        let row = index / cols_n;
        let v = (point.pos[1] - bounds.y0) / height;
        let shift = peak * 4.0 * v * (1.0 - v);
        let slope = -peak * 4.0 * (1.0 - 2.0 * v) / height;
        let pos = [point.pos[0] + shift, point.pos[1]];
        point.pos = pos;
        point.handles = [pos; 4];
        if row > 0 {
            point.handles[HANDLE_UP] = [pos[0] - slope * reach, pos[1] - reach];
        }
        if row + 1 < mesh.rows as usize {
            point.handles[HANDLE_DOWN] = [pos[0] + slope * reach, pos[1] + reach];
        }
    }
    mesh
}

pub fn preset_mesh(preset: &WarpPreset, current: &WarpMesh, bounds: Rect, value: f64) -> WarpMesh {
    let bounds = bounds.abs();
    let (cols, rows) = match preset {
        WarpPreset::Mesh if mesh_is_valid(current) => (current.cols, current.rows),
        _ => {
            let (d_c, d_r) = preset_mesh_dims(preset);
            if mesh_is_valid(current) { (current.cols, current.rows) } else { (d_c, d_r) }
        }
    };
    match preset {
        WarpPreset::Mesh if mesh_is_valid(current) => current.clone(),
        WarpPreset::Mesh => identity_mesh(cols, rows, bounds),
        WarpPreset::Quad => identity_mesh(cols.max(2), rows.max(2), bounds),
        WarpPreset::Perspective => identity_then(cols, rows, bounds, value, |_u, v, pos, value, bounds| {
            let mid = (bounds.x0 + bounds.x1) / 2.0;
            let k = (value / PRESET_VALUE_LIMIT) * 0.45 * (1.0 - v);
            [mid + (pos[0] - mid) * (1.0 - k), pos[1]]
        }),
        WarpPreset::Arch | WarpPreset::ArcVertical => arch_like_mesh(cols, rows, bounds, value, 1.0),
        WarpPreset::UpperArc => arch_like_mesh(cols, rows, bounds, value, 2.0),
        WarpPreset::LowerArc => arch_like_mesh(cols, rows, bounds, value, -1.0),
        WarpPreset::ArcHorizontal => horizontal_arch_mesh(cols, rows, bounds, value),
        WarpPreset::BendVertical => identity_then(cols, rows, bounds, value, |_u, v, pos, value, bounds| {
            [pos[0] + peak(bounds, value, true) * (v * std::f64::consts::PI).sin(), pos[1]]
        }),
        WarpPreset::BendHorizontal => identity_then(cols, rows, bounds, value, |u, _v, pos, value, bounds| {
            [pos[0], pos[1] + peak(bounds, value, false) * (u * std::f64::consts::PI).sin()]
        }),
        WarpPreset::FishEye | WarpPreset::Bulge => identity_then(cols, rows, bounds, value, |u, v, pos, value, bounds| {
            let cx = (bounds.x0 + bounds.x1) / 2.0;
            let cy = (bounds.y0 + bounds.y1) / 2.0;
            let dx = pos[0] - cx;
            let dy = pos[1] - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let r_max = ((bounds.width() / 2.0).hypot(bounds.height() / 2.0)).max(MESH_EPSILON);
            let k = value / PRESET_VALUE_LIMIT * 0.45 * (1.0 - r / r_max);
            let _ = (u, v);
            [cx + dx * (1.0 + k), cy + dy * (1.0 + k)]
        }),
        WarpPreset::Pinch => identity_then(cols, rows, bounds, value, |u, v, pos, value, bounds| {
            let cx = (bounds.x0 + bounds.x1) / 2.0;
            let cy = (bounds.y0 + bounds.y1) / 2.0;
            let dx = pos[0] - cx;
            let dy = pos[1] - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let r_max = ((bounds.width() / 2.0).hypot(bounds.height() / 2.0)).max(MESH_EPSILON);
            let k = value / PRESET_VALUE_LIMIT * 0.45 * (1.0 - r / r_max);
            let _ = (u, v);
            [cx + dx * (1.0 - k), cy + dy * (1.0 - k)]
        }),
        WarpPreset::Twist | WarpPreset::Twirl => identity_then(cols, rows, bounds, value, |u, v, pos, value, bounds| {
            let cx = (bounds.x0 + bounds.x1) / 2.0;
            let cy = (bounds.y0 + bounds.y1) / 2.0;
            let dx = pos[0] - cx;
            let dy = pos[1] - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let r_max = ((bounds.width() / 2.0).hypot(bounds.height() / 2.0)).max(MESH_EPSILON);
            let angle = value / PRESET_VALUE_LIMIT * std::f64::consts::PI * (1.0 - r / r_max);
            let (sin, cos) = angle.sin_cos();
            let _ = (u, v);
            [cx + dx * cos - dy * sin, cy + dx * sin + dy * cos]
        }),
        WarpPreset::Spiral => identity_then(cols, rows, bounds, value, |u, v, pos, value, bounds| {
            let cx = (bounds.x0 + bounds.x1) / 2.0;
            let cy = (bounds.y0 + bounds.y1) / 2.0;
            let dx = pos[0] - cx;
            let dy = pos[1] - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let r_max = ((bounds.width() / 2.0).hypot(bounds.height() / 2.0)).max(MESH_EPSILON);
            let t = r / r_max;
            let angle = value / PRESET_VALUE_LIMIT * std::f64::consts::TAU * t;
            let (sin, cos) = angle.sin_cos();
            let scale = 1.0 + value / PRESET_VALUE_LIMIT * 0.15 * t;
            let _ = (u, v);
            [cx + (dx * cos - dy * sin) * scale, cy + (dx * sin + dy * cos) * scale]
        }),
        WarpPreset::Flag => identity_then(cols, rows, bounds, value, |u, v, pos, value, bounds| {
            [pos[0], pos[1] + peak(bounds, value, false) * (u * std::f64::consts::TAU).sin() * v]
        }),
        WarpPreset::Wave => identity_then(cols, rows, bounds, value, |u, _v, pos, value, bounds| {
            [pos[0], pos[1] + peak(bounds, value, false) * (u * std::f64::consts::TAU * 2.0).sin()]
        }),
        WarpPreset::Rise => identity_then(cols, rows, bounds, value, |u, _v, pos, value, bounds| {
            [pos[0], pos[1] - peak(bounds, value, false) * u]
        }),
    }
}

pub fn apply_preset(group: &mut WarpGroup, preset: WarpPreset, value: f64, bounds: Rect) {
    group.mesh = preset_mesh(&preset, &group.mesh, bounds, value);
    group.preset = preset;
    group.value = value;
}

pub fn reset_mesh(group: &mut WarpGroup, bounds: Rect) {
    group.mesh = identity_mesh(group.mesh.cols, group.mesh.rows, bounds);
}

pub fn mark_mesh_edited(group: &mut WarpGroup) {
    group.preset = WarpPreset::Mesh;
    group.value = 0.0;
}


#[cfg(test)]
mod evaluation_contracts {
    use super::*;
    fn close(a:[f64;2],b:[f64;2]) { assert!((a[0]-b[0]).abs()<1e-8 && (a[1]-b[1]).abs()<1e-8,"{a:?} != {b:?}"); }
    #[test]
    fn identity_accepts_rect_and_legacy_array_bounds() {
        let rect=Rect::new(10.0,20.0,110.0,220.0);let mesh=identity_mesh(3,4,rect);
        for p in [[10.0,20.0],[110.0,220.0],[42.0,93.0],[0.0,250.0]] {
            close(evaluate(&mesh,rect,p),p);close(evaluate(&mesh,[10.0,20.0,110.0,220.0],p),p);
        }
    }
    #[test]
    fn translated_mesh_maps_every_cell_and_path() {
        let rect=Rect::new(0.0,0.0,100.0,100.0);let mut mesh=identity_mesh(3,3,rect);
        for pt in &mut mesh.points {pt.pos[0]+=7.0;pt.pos[1]-=4.0;pt.handles=[pt.pos;4];}
        let path=PathData {points:vec![[0.0,0.0],[25.0,75.0],[100.0,100.0]]};
        let out=warp_path(&mesh,[0.0,0.0,100.0,100.0],&path);
        for (actual,old) in out.points.into_iter().zip(path.points) {close(actual,[old[0]+7.0,old[1]-4.0]);}
    }
    #[test]
    fn boundary_handles_curve_the_patch_and_shared_edges_agree() {
        let rect=Rect::new(0.0,0.0,100.0,100.0);let mut mesh=identity_mesh(3,2,rect);
        mesh.points[0].handles[HANDLE_RIGHT]=[50.0/3.0,-20.0];mesh.points[1].handles[HANDLE_LEFT]=[100.0/3.0,-20.0];
        close(evaluate(&mesh,rect,[25.0,0.0]),[25.0,-15.0]);
        for t in [0.0,0.25,0.5,1.0] {close(evaluate_patch(&mesh,0,0,1.0,t).unwrap(),evaluate_patch(&mesh,1,0,0.0,t).unwrap());}
    }
    #[test]
    fn malformed_mesh_and_zero_bounds_preserve_input() {
        let rect=Rect::new(0.0,0.0,100.0,100.0);let mut mesh=identity_mesh(2,2,rect);mesh.points.pop();
        assert!(!mesh_is_valid(&mesh),"malformed meshes must not run interpolation");
        close(evaluate(&mesh,rect,[25.0,50.0]),[25.0,50.0]);
        close(evaluate(&identity_mesh(2,2,rect),Rect::ZERO,[25.0,50.0]),[25.0,50.0]);
    }
}

#[cfg(test)]
mod preset_contracts {
    use super::*;

    #[test]
    fn offered_names_match_affinity_menu() {
        let names: Vec<&str> = OFFERED_PRESETS.iter().map(preset_display_name).collect();
        assert_eq!(
            names,
            [
                "Mesh",
                "Quad",
                "Perspective",
                "Arc - Vertical",
                "Arc - Horizontal",
                "Bend - Vertical",
                "Bend - Horizontal",
                "Fish Eye",
                "Twist",
                "Arch",
                "Upper Arc",
                "Lower Arc",
                "Bulge",
                "Flag",
                "Rise",
            ]
        );
        assert_eq!(OFFERED_PRESETS.len(), 15);
        for preset in PENDING_PRESETS {
            assert!(is_pending_preset(preset));
            assert!(!is_offered_preset(preset));
        }
    }

    #[test]
    fn parse_accepts_display_slugs_and_legacy_aliases() {
        assert_eq!(parse_preset("Arc - Vertical"), Some(WarpPreset::ArcVertical));
        assert_eq!(parse_preset("arc_vertical"), Some(WarpPreset::ArcVertical));
        assert_eq!(parse_preset("Fish Eye"), Some(WarpPreset::FishEye));
        assert_eq!(parse_preset("fish_eye"), Some(WarpPreset::FishEye));
        assert_eq!(parse_preset("arch"), Some(WarpPreset::Arch));
        assert_eq!(parse_preset("arc"), Some(WarpPreset::Arch));
        assert_eq!(parse_preset("Arc - Horizontal"), Some(WarpPreset::ArcHorizontal));
        assert_eq!(parse_preset("wave"), Some(WarpPreset::Wave));
        assert_eq!(parse_preset("not-a-preset"), None);
    }

    #[test]
    fn offered_presets_build_distinct_meshes() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 80.0);
        let identity = identity_mesh(4, 4, bounds);
        for preset in OFFERED_PRESETS {
            let mesh = create_mesh_for_preset(&preset, bounds);
            assert!(mesh_is_valid(&mesh), "{}", preset_display_name(&preset));
            if matches!(preset, WarpPreset::Mesh | WarpPreset::Quad) {
                continue;
            }
            let moved = mesh.points.iter().any(|p| {
                identity.points.iter().all(|q| (q.pos[0] - p.pos[0]).abs() > 1e-6 || (q.pos[1] - p.pos[1]).abs() > 1e-6)
            }) || mesh.cols != identity.cols || mesh.rows != identity.rows;
            assert!(moved, "preset {} should deform or resize the mesh", preset_display_name(&preset));
        }
        let wave = create_mesh_for_preset(&WarpPreset::Wave, bounds);
        assert!(mesh_is_valid(&wave));
        assert!(wave.points.iter().any(|p| identity.points.iter().all(|q| (q.pos[1] - p.pos[1]).abs() > 1e-6 || (q.pos[0] - p.pos[0]).abs() > 1e-6)));
    }
}
