use dtool_core::paint::{Paint, Colour, GradientKind, Hatch};
use dtool_core::object::ObjectId;
use std::collections::HashMap;

pub struct RasterContext {
    pub width: u32,
    pub height: u32,
    pub seed: Option<u64>,
}

pub struct RasterPixel {
    pub x: u32,
    pub y: u32,
    pub colour: Colour,
}

pub fn rasterise_paint(
    paint: &Paint,
    context: &RasterContext,
    outline: &[(f64, f64)],
) -> Vec<RasterPixel> {
    match paint {
        Paint::None => vec![],
        Paint::Solid { colour } => rasterise_solid(colour, context, outline),
        Paint::Gradient { kind, stops, matrix } => {
            rasterise_gradient(kind, stops, matrix, context, outline)
        }
        Paint::Hatch(hatch) => rasterise_hatch(hatch, context, outline),
        Paint::Bitmap { .. } => vec![],
        Paint::Pattern { .. } => vec![],
        Paint::Mesh { .. } => rasterise_mesh(context, outline),
        Paint::Diffusion { .. } => rasterise_diffusion(context, outline),
    }
}

fn rasterise_solid(
    colour: &Colour,
    context: &RasterContext,
    outline: &[(f64, f64)],
) -> Vec<RasterPixel> {
    let mut pixels = Vec::new();
    for y in 0..context.height {
        for x in 0..context.width {
            if point_in_polygon(x as f64 + 0.5, y as f64 + 0.5, outline) {
                pixels.push(RasterPixel {
                    x,
                    y,
                    colour: *colour,
                });
            }
        }
    }
    pixels
}

fn rasterise_gradient(
    kind: &GradientKind,
    stops: &[dtool_core::paint::GradientStop],
    matrix: &[f64; 6],
    context: &RasterContext,
    outline: &[(f64, f64)],
) -> Vec<RasterPixel> {
    let mut pixels = Vec::new();
    for y in 0..context.height {
        for x in 0..context.width {
            if !point_in_polygon(x as f64 + 0.5, y as f64 + 0.5, outline) {
                continue;
            }
            let t = gradient_parameter(kind, x as f64, y as f64, matrix);
            let colour = interpolate_stops(stops, t);
            pixels.push(RasterPixel { x, y, colour });
        }
    }
    pixels
}

fn gradient_parameter(
    kind: &GradientKind,
    x: f64,
    y: f64,
    matrix: &[f64; 6],
) -> f32 {
    let tx = matrix[0] * x + matrix[2] * y + matrix[4];
    let ty = matrix[1] * x + matrix[3] * y + matrix[5];
    match kind {
        GradientKind::Linear => ((tx - 0.5) as f32).clamp(0.0, 1.0),
        GradientKind::Radial => (tx.hypot(ty) as f32).clamp(0.0, 1.0),
        GradientKind::Elliptical => {
            let dx = tx - 0.5;
            let dy = ty - 0.5;
            ((dx * dx + dy * dy).sqrt() as f32).clamp(0.0, 1.0)
        }
        GradientKind::Conical => {
            let angle = (ty - 0.5).atan2(tx - 0.5);
            ((angle + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) as f32
        }
    }
}

fn interpolate_stops(
    stops: &[dtool_core::paint::GradientStop],
    t: f32,
) -> Colour {
    if stops.is_empty() {
        return Colour { r: 0, g: 0, b: 0, a: 255 };
    }
    if stops.len() == 1 || t <= stops[0].offset {
        return stops[0].colour;
    }
    for i in 0..stops.len() - 1 {
        let s0 = &stops[i];
        let s1 = &stops[i + 1];
        if t >= s0.offset && t <= s1.offset {
            let range = s1.offset - s0.offset;
            if range == 0.0 {
                return s0.colour;
            }
            let ratio = (t - s0.offset) / range;
            return Colour {
                r: lerp_u8(s0.colour.r, s1.colour.r, ratio),
                g: lerp_u8(s0.colour.g, s1.colour.g, ratio),
                b: lerp_u8(s0.colour.b, s1.colour.b, ratio),
                a: lerp_u8(s0.colour.a, s1.colour.a, ratio),
            };
        }
    }
    stops[stops.len() - 1].colour
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let v = a as f32 + (b as f32 - a as f32) * t;
    v.round().clamp(0.0, 255.0) as u8
}

fn rasterise_hatch(
    hatch: &Hatch,
    context: &RasterContext,
    outline: &[(f64, f64)],
) -> Vec<RasterPixel> {
    let mut pixels = Vec::new();
    for y in 0..context.height {
        for x in 0..context.width {
            if !point_in_polygon(x as f64 + 0.5, y as f64 + 0.5, outline) {
                continue;
            }
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let mut hit = false;
            for line in &hatch.lines {
                if hatch_line_hit(px, py, line) {
                    hit = true;
                    break;
                }
            }
            if hit {
                pixels.push(RasterPixel {
                    x,
                    y,
                    colour: hatch.colour,
                });
            } else if let Some(bg) = &hatch.background {
                pixels.push(RasterPixel {
                    x,
                    y,
                    colour: *bg,
                });
            }
        }
    }
    pixels
}

fn hatch_line_hit(
    px: f64,
    py: f64,
    line: &dtool_core::paint::HatchLine,
) -> bool {
    let dx = px - line.origin[0];
    let dy = py - line.origin[1];
    let cos_a = line.angle_rad.cos();
    let sin_a = line.angle_rad.sin();
    let proj = dx * cos_a + dy * sin_a;
    let perp = -dx * sin_a + dy * cos_a;
    if perp.abs() > hatch.weight_half(line) {
        return false;
    }
    if line.dash.is_empty() {
        return true;
    }
    let mut pos = proj % line.spacing;
    if pos < 0.0 {
        pos += line.spacing;
    }
    let mut acc = 0.0;
    for &d in &line.dash {
        if pos >= acc && pos < acc + d {
            return true;
        }
        acc += d;
    }
    false
}

fn hatch_weight_half(line: &dtool_core::paint::HatchLine) -> f64 {
    0.5
}

fn rasterise_mesh(
    context: &RasterContext,
    outline: &[(f64, f64)],
) -> Vec<RasterPixel> {
    let seed = context.seed.unwrap_or(0);
    let mut pixels = Vec::new();
    for y in 0..context.height {
        for x in 0..context.width {
            if !point_in_polygon(x as f64 + 0.5, y as f64 + 0.5, outline) {
                continue;
            }
            let noise = seeded_noise(seed, x, y);
            pixels.push(RasterPixel {
                x,
                y,
                colour: Colour {
                    r: (noise * 255.0) as u8,
                    g: ((1.0 - noise) * 255.0) as u8,
                    b: 128,
                    a: 255,
                },
            });
        }
    }
    pixels
}

fn rasterise_diffusion(
    context: &RasterContext,
    outline: &[(f64, f64)],
) -> Vec<RasterPixel> {
    let seed = context.seed.unwrap_or(0);
    let mut pixels = Vec::new();
    for y in 0..context.height {
        for x in 0..context.width {
            if !point_in_polygon(x as f64 + 0.5, y as f64 + 0.5, outline) {
                continue;
            }
            let noise = seeded_noise(seed, x, y);
            pixels.push(RasterPixel {
                x,
                y,
                colour: Colour {
                    r: 255,
                    g: (noise * 255.0) as u8,
                    b: ((1.0 - noise) * 255.0) as u8,
                    a: 255,
                },
            });
        }
    }
    pixels
}

fn seeded_noise(seed: u64, x: u32, y: u32) -> f64 {
    let mut h = seed ^ (x as u64).wrapping_mul(0x9E3779B97F4A7C15);
    h ^= (y as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h = h.wrapping_mul(0x94D049BB133111EB);
    h ^= h >> 32;
    (h as f64) / (u64::MAX as f64)
}

fn point_in_polygon(px: f64, py: f64, polygon: &[(f64, f64)]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let n = polygon.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let xi = polygon[i].0;
        let yi = polygon[i].1;
        let xj = polygon[j].0;
        let yj = polygon[j].1;
        if ((yi > py) != (yj > py))
            && (px < (xj - xi) * (py - yi) / (yj - yi) + xi)
        {
            inside = !inside;
        }
    }
    inside
}

pub fn rasterise_stroke(
    stroke: &dtool_core::paint::StrokeStyle,
    context: &RasterContext,
    path: &[(f64, f64)],
) -> Vec<RasterPixel> {
    if stroke.width <= 0.0 {
        return vec![];
    }
    let mut pixels = Vec::new();
    for y in 0..context.height {
        for x in 0..context.width {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            if point_near_path(px, py, path, stroke.width as f64) {
                let colour = match &stroke.paint {
                    Paint::Solid { colour } => *colour,
                    _ => Colour { r: 0, g: 0, b: 0, a: 255 },
                };
                pixels.push(RasterPixel { x, y, colour });
            }
        }
    }
    pixels
}

pub fn rasterise_glyph(
    paint: &Paint,
    context: &RasterContext,
    glyph_outline: &[(f64, f64)],
) -> Vec<RasterPixel> {
    rasterise_paint(paint, context, glyph_outline)
}

fn point_near_path(px: f64, py: f64, path: &[(f64, f64)], radius: f64) -> bool {
    if path.len() < 2 {
        return false;
    }
    for i in 0..path.len() - 1 {
        let ax = path[i].0;
        let ay = path[i].1;
        let bx = path[i + 1].0;
        let by = path[i + 1].1;
        let dx = bx - ax;
        let dy = by - ay;
        let len_sq = dx * dx + dy * dy;
        if len_sq == 0.0 {
            continue;
        }
        let t = ((px - ax) * dx + (py - ay) * dy) / len_sq;
        let t_clamped = t.clamp(0.0, 1.0);
        let cx = ax + t_clamped * dx;
        let cy = ay + t_clamped * dy;
        let dist_sq = (px - cx) * (px - cx) + (py - cy) * (py - cy);
        if dist_sq <= radius * radius {
            return true;
        }
    }
    false
}
