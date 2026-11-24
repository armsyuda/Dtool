use crate::command::Session;
use crate::document::Document;
use crate::object::ObjectId;
use crate::pixel::PixelLayer;
use dtool_protocol::{Command, CommandError, CommandResult};

pub const PERSPECTIVE_APPLY: &str = "pixel.perspective.apply";
pub const PERSPECTIVE_PREVIEW: &str = "pixel.perspective.preview";
pub const PERSPECTIVE_CANCEL: &str = "pixel.perspective.cancel";
pub const PERSPECTIVE_RESTORE: &str = "pixel.perspective.restore";

#[derive(Clone, Copy, PartialEq)]
pub enum PerspectiveMode {
    SinglePlane,
    TwoPlanes,
}

#[derive(Clone)]
pub struct PixelPerspective {
    pub layer: ObjectId,
    pub mode: PerspectiveMode,
    pub src: [[f32; 2]; 4],
    pub dst: [[f32; 2]; 4],
    pub preserve_alpha: bool,
    pub show_grid: bool,
    pub src2: [[f32; 2]; 4],
    pub dst2: [[f32; 2]; 4],
}

impl PixelPerspective {
    pub fn rect_corners(x0: f32, y0: f32, x1: f32, y1: f32) -> [[f32; 2]; 4] {
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    pub fn for_layer(layer: ObjectId, width: u32, height: u32, mode: PerspectiveMode) -> Self {
        let w = width as f32;
        let h = height as f32;
        let (src, src2) = match mode {
            PerspectiveMode::SinglePlane => {
                let full = Self::rect_corners(0.0, 0.0, w, h);
                (full, full)
            }
            PerspectiveMode::TwoPlanes => {
                let mid = w * 0.5;
                (Self::rect_corners(0.0, 0.0, mid, h), Self::rect_corners(mid, 0.0, w, h))
            }
        };
        Self {
            layer,
            mode,
            src,
            dst: src,
            preserve_alpha: true,
            show_grid: true,
            src2,
            dst2: src2,
        }
    }

    pub fn plane_count(&self) -> usize {
        match self.mode {
            PerspectiveMode::SinglePlane => 1,
            PerspectiveMode::TwoPlanes => 2,
        }
    }

    pub fn is_identity(&self) -> bool {
        let same = |a: &[[f32; 2]; 4], b: &[[f32; 2]; 4]| a.iter().zip(b.iter()).all(|(p, q)| p[0] == q[0] && p[1] == q[1]);
        match self.mode {
            PerspectiveMode::SinglePlane => same(&self.src, &self.dst),
            PerspectiveMode::TwoPlanes => same(&self.src, &self.dst) && same(&self.src2, &self.dst2),
        }
    }
}

fn mode_name(mode: PerspectiveMode) -> &'static str {
    match mode {
        PerspectiveMode::SinglePlane => "single",
        PerspectiveMode::TwoPlanes => "two",
    }
}

fn quad_ok(q: &[[f32; 2]; 4]) -> bool {
    if !q.iter().all(|p| p[0].is_finite() && p[1].is_finite()) {
        return false;
    }
    let mut area = 0.0f64;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let c = q[(i + 2) % 4];
        let cross = (b[0] as f64 - a[0] as f64) * (c[1] as f64 - b[1] as f64) - (b[1] as f64 - a[1] as f64) * (c[0] as f64 - b[0] as f64);
        if cross.abs() < 1e-6 {
            return false;
        }
        area += a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64;
    }
    area.abs() > 1e-6
}

fn solve8(mut m: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let mut pivot = col;
        for row in col + 1..8 {
            if m[row][col].abs() > m[pivot][col].abs() {
                pivot = row;
            }
        }
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        let d = m[col][col];
        for k in col..9 {
            m[col][k] /= d;
        }
        for row in 0..8 {
            if row != col {
                let f = m[row][col];
                if f != 0.0 {
                    for k in col..9 {
                        m[row][k] -= f * m[col][k];
                    }
                }
            }
        }
    }
    let mut out = [0.0; 8];
    for i in 0..8 {
        out[i] = m[i][8];
    }
    Some(out)
}

pub fn homography(from: &[[f32; 2]; 4], to: &[[f32; 2]; 4]) -> Result<[f64; 9], CommandError> {
    if !quad_ok(from) || !quad_ok(to) {
        return Err(CommandError::invalid_params());
    }
    let mut m = [[0.0f64; 9]; 8];
    for i in 0..4 {
        let x = from[i][0] as f64;
        let y = from[i][1] as f64;
        let u = to[i][0] as f64;
        let v = to[i][1] as f64;
        m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    let h = solve8(m).ok_or_else(CommandError::invalid_params)?;
    if !h.iter().all(|v| v.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    Ok([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0])
}

pub fn map_point(h: &[f64; 9], x: f64, y: f64) -> Option<[f64; 2]> {
    let w = h[6] * x + h[7] * y + h[8];
    if w.abs() < 1e-12 || !w.is_finite() {
        return None;
    }
    Some([(h[0] * x + h[1] * y + h[2]) / w, (h[3] * x + h[4] * y + h[5]) / w])
}

pub fn point_in_quad(q: &[[f32; 2]; 4], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = 3;
    for i in 0..4 {
        let xi = q[i][0] as f64;
        let yi = q[i][1] as f64;
        let xj = q[j][0] as f64;
        let yj = q[j][1] as f64;
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn sample(src: &[u8], width: u32, height: u32, sx: f64, sy: f64, premul: bool) -> [u8; 4] {
    let fx = sx - 0.5;
    let fy = sy - 0.5;
    if !fx.is_finite() || !fy.is_finite() || fx < -1.0 || fy < -1.0 || fx > width as f64 || fy > height as f64 {
        return [0; 4];
    }
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = fx - x0;
    let ty = fy - y0;
    let mut acc = [0.0f64; 4];
    let taps = [(0.0, 0.0, (1.0 - tx) * (1.0 - ty)), (1.0, 0.0, tx * (1.0 - ty)), (0.0, 1.0, (1.0 - tx) * ty), (1.0, 1.0, tx * ty)];
    for (dx, dy, wgt) in taps {
        if wgt <= 1e-9 {
            continue;
        }
        let xi = x0 + dx;
        let yi = y0 + dy;
        if xi < 0.0 || yi < 0.0 || xi >= width as f64 || yi >= height as f64 {
            continue;
        }
        let idx = (yi as usize * width as usize + xi as usize) * 4;
        let a = src[idx + 3] as f64;
        if premul {
            let k = wgt * a / 255.0;
            acc[0] += src[idx] as f64 * k;
            acc[1] += src[idx + 1] as f64 * k;
            acc[2] += src[idx + 2] as f64 * k;
        } else {
            acc[0] += src[idx] as f64 * wgt;
            acc[1] += src[idx + 1] as f64 * wgt;
            acc[2] += src[idx + 2] as f64 * wgt;
        }
        acc[3] += a * wgt;
    }
    let to_u8 = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    let alpha = to_u8(acc[3]);
    if premul {
        if alpha == 0 {
            return [0; 4];
        }
        let inv = 255.0 / acc[3];
        [to_u8(acc[0] * inv), to_u8(acc[1] * inv), to_u8(acc[2] * inv), alpha]
    } else {
        [to_u8(acc[0]), to_u8(acc[1]), to_u8(acc[2]), alpha]
    }
}

pub fn warp_pixel_layer(
    layer: &PixelLayer,
    perspective: &PixelPerspective,
) -> Result<PixelLayer, CommandError> {
    let h1 = homography(&perspective.dst, &perspective.src)?;
    let h2 = match perspective.mode {
        PerspectiveMode::SinglePlane => None,
        PerspectiveMode::TwoPlanes => Some(homography(&perspective.dst2, &perspective.src2)?),
    };
    if perspective.is_identity() {
        return Ok(layer.clone());
    }
    let width = layer.width;
    let height = layer.height;
    let src = layer.to_rgba();
    let mut out = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let h = match h2 {
                Some(ref h2) if point_in_quad(&perspective.dst2, px, py) => h2,
                _ => &h1,
            };
            let rgba = match map_point(h, px, py) {
                Some([sx, sy]) => sample(&src, width, height, sx, sy, perspective.preserve_alpha),
                None => [0; 4],
            };
            let idx = (y as usize * width as usize + x as usize) * 4;
            out[idx..idx + 4].copy_from_slice(&rgba);
        }
    }
    let mut warped = PixelLayer::from_rgba(width, height, &out);
    warped.compact();
    Ok(warped)
}

fn changed_stats(a: &PixelLayer, b: &PixelLayer) -> (u64, Option<[u32; 4]>) {
    let ra = a.to_rgba();
    let rb = b.to_rgba();
    let width = a.width.min(b.width);
    let height = a.height.min(b.height);
    let mut count = 0u64;
    let mut bounds: Option<[u32; 4]> = None;
    for y in 0..height {
        for x in 0..width {
            let ia = (y as usize * a.width as usize + x as usize) * 4;
            let ib = (y as usize * b.width as usize + x as usize) * 4;
            if ra[ia..ia + 4] != rb[ib..ib + 4] {
                count += 1;
                bounds = Some(match bounds {
                    None => [x, y, x, y],
                    Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
                });
            }
        }
    }
    (count, bounds.map(|[x0, y0, x1, y1]| [x0, y0, x1 - x0 + 1, y1 - y0 + 1]))
}

pub fn perspective_params(perspective: &PixelPerspective) -> serde_json::Value {
    serde_json::json!({
        "layer": perspective.layer.0,
        "mode": mode_name(perspective.mode),
        "src": perspective.src,
        "dst": perspective.dst,
        "src2": perspective.src2,
        "dst2": perspective.dst2,
        "preserve_alpha": perspective.preserve_alpha,
        "show_grid": perspective.show_grid,
    })
}

fn parse_quad(v: Option<&serde_json::Value>) -> Option<[[f32; 2]; 4]> {
    let arr = v?.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    let mut q = [[0.0f32; 2]; 4];
    for (i, p) in arr.iter().enumerate() {
        let xy = p.as_array()?;
        if xy.len() != 2 {
            return None;
        }
        q[i] = [xy[0].as_f64()? as f32, xy[1].as_f64()? as f32];
    }
    Some(q)
}

pub fn parse_perspective(params: &serde_json::Value) -> Result<PixelPerspective, CommandError> {
    let layer = params.get("layer").and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)?;
    let mode = match params.get("mode").and_then(|v| v.as_str()).unwrap_or("single") {
        "single" => PerspectiveMode::SinglePlane,
        "two" => PerspectiveMode::TwoPlanes,
        _ => return Err(CommandError::invalid_params()),
    };
    let src = parse_quad(params.get("src")).ok_or_else(CommandError::invalid_params)?;
    let dst = parse_quad(params.get("dst")).ok_or_else(CommandError::invalid_params)?;
    let src2 = parse_quad(params.get("src2")).unwrap_or(src);
    let dst2 = parse_quad(params.get("dst2")).unwrap_or(src2);
    Ok(PixelPerspective {
        layer: ObjectId(layer),
        mode,
        src,
        dst,
        preserve_alpha: params.get("preserve_alpha").and_then(|v| v.as_bool()).unwrap_or(true),
        show_grid: params.get("show_grid").and_then(|v| v.as_bool()).unwrap_or(false),
        src2,
        dst2,
    })
}

pub fn pixel_perspective_preview(
    session: &Session,
    doc: &Document,
    perspective: &PixelPerspective,
) -> Result<CommandResult, CommandError> {
    let original = doc.pixel_layers.get(perspective.layer).ok_or_else(CommandError::not_found)?;
    let warped = warp_pixel_layer(original, perspective)?;
    let (changed, bounds) = changed_stats(original, &warped);
    Ok(Ok(serde_json::json!({
        "preview": true,
        "doc_version": session.generation,
        "layer": perspective.layer.0,
        "mode": mode_name(perspective.mode),
        "width": warped.width,
        "height": warped.height,
        "changed_pixels": changed,
        "changed_bounds": bounds,
        "show_grid": perspective.show_grid,
        "preserve_alpha": perspective.preserve_alpha,
    })))
}

fn apply_parts(
    undo: &mut crate::undo::UndoStack,
    generation: &mut u64,
    doc: &mut Document,
    perspective: &PixelPerspective,
) -> Result<CommandResult, CommandError> {
    let original = doc.pixel_layers.get(perspective.layer).ok_or_else(CommandError::not_found)?.clone();
    let warped = warp_pixel_layer(&original, perspective)?;
    let (changed, bounds) = changed_stats(&original, &warped);
    if changed == 0 {
        return Ok(Ok(serde_json::json!({
            "doc_version": *generation,
            "layer": perspective.layer.0,
            "changed": false,
            "changed_pixels": 0,
        })));
    }
    let before = serde_json::to_value(&original).map_err(|_| CommandError::invalid_params())?;
    doc.pixel_layers.insert(perspective.layer, warped);
    undo.push(crate::undo::Change {
        name: PERSPECTIVE_APPLY.into(),
        redo: Command::with_params(PERSPECTIVE_APPLY, perspective_params(perspective)),
        undo: Command::with_params(PERSPECTIVE_RESTORE, serde_json::json!({ "layer": perspective.layer.0, "pixels": before })),
    });
    *generation += 1;
    doc.version += 1;
    Ok(Ok(serde_json::json!({
        "doc_version": *generation,
        "layer": perspective.layer.0,
        "changed": true,
        "changed_pixels": changed,
        "changed_bounds": bounds,
    })))
}

pub fn pixel_perspective_apply(
    session: &mut Session,
    doc: &mut Document,
    perspective: &PixelPerspective,
) -> Result<CommandResult, CommandError> {
    apply_parts(&mut session.undo, &mut session.generation, doc, perspective)
}

pub fn pixel_perspective_cancel(
    session: &mut Session,
    doc: &mut Document,
) -> Result<CommandResult, CommandError> {
    let _ = (session, doc);
    Ok(Ok(serde_json::json!({ "changed": false })))
}

pub fn pixel_perspective_revert(doc: &mut Document, layer: ObjectId, original: PixelLayer) -> bool {
    if !doc.pixel_layers.contains(layer) {
        return false;
    }
    let same = doc.pixel_layers.get(layer).is_some_and(|cur| {
        cur.width == original.width && cur.height == original.height && cur.to_rgba() == original.to_rgba()
    });
    if same {
        return false;
    }
    doc.pixel_layers.insert(layer, original);
    doc.version += 1;
    true
}

fn ours(cmd: &Command) -> bool {
    matches!(cmd.name.as_str(), PERSPECTIVE_APPLY | PERSPECTIVE_PREVIEW | PERSPECTIVE_CANCEL)
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    if !ours(cmd) {
        return None;
    }
    if cmd.name == PERSPECTIVE_CANCEL {
        return Some(true);
    }
    Some(parse_perspective(&cmd.params).map_or(false, |p| session.document.pixel_layers.contains(p.layer)))
}

fn flatten(r: Result<CommandResult, CommandError>) -> CommandResult {
    match r {
        Ok(inner) => inner,
        Err(e) => Err(e),
    }
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    if !ours(cmd) {
        return None;
    }
    if cmd.name == PERSPECTIVE_CANCEL {
        return Some(Ok(serde_json::json!({})));
    }
    let p = match parse_perspective(&cmd.params) {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    if cmd.name == PERSPECTIVE_PREVIEW {
        let shared: &Session = session;
        return Some(flatten(pixel_perspective_preview(shared, &shared.document, &p)));
    }
    let s = session;
    Some(flatten(apply_parts(&mut s.undo, &mut s.generation, &mut s.document, &p)))
}

fn restore(session: &mut Session, cmd: &Command) -> Result<(), CommandError> {
    let layer = cmd.params.get("layer").and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)?;
    let pixels: PixelLayer = serde_json::from_value(cmd.params.get("pixels").cloned().unwrap_or_default()).map_err(|_| CommandError::invalid_params())?;
    if !session.document.pixel_layers.contains(ObjectId(layer)) {
        return Err(CommandError::not_found());
    }
    session.document.pixel_layers.insert(ObjectId(layer), pixels);
    session.document.version += 1;
    Ok(())
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<Result<(), CommandError>> {
    if cmd.name == PERSPECTIVE_RESTORE {
        return Some(restore(session, cmd));
    }
    if cmd.name != PERSPECTIVE_APPLY {
        return None;
    }
    Some((|| {
        let p = parse_perspective(&cmd.params)?;
        let original = session.document.pixel_layers.get(p.layer).ok_or_else(CommandError::not_found)?;
        let warped = warp_pixel_layer(original, &p)?;
        session.document.pixel_layers.insert(p.layer, warped);
        session.document.version += 1;
        Ok(())
    })())
}
