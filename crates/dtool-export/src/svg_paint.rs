use dtool_core::paint::{Paint, Colour, GradientKind, Hatch};
use std::fmt::Write;

pub fn paint_to_svg(paint: &Paint) -> String {
    match paint {
        Paint::None => "none".to_string(),
        Paint::Solid { colour } => colour_to_hex(colour),
        Paint::Gradient { kind, stops, matrix } => gradient_to_svg(kind, stops, matrix),
        Paint::Hatch(hatch) => hatch_to_svg(hatch),
        Paint::Bitmap { resource_id, matrix } => bitmap_to_svg(resource_id, matrix),
        Paint::Pattern { resource_id, matrix } => pattern_to_svg(resource_id, matrix),
        Paint::Mesh { .. } => mesh_to_svg(),
        Paint::Diffusion { .. } => diffusion_to_svg(),
    }
}

pub fn text_paint_to_svg(paint: &Paint) -> String {
    match paint {
        Paint::Pattern { resource_id, matrix } => pattern_to_svg(resource_id, matrix),
        _ => paint_to_svg(paint),
    }
}

fn colour_to_hex(colour: &Colour) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
}

fn gradient_to_svg(kind: &GradientKind, stops: &[dtool_core::paint::GradientStop], matrix: &[f64; 6]) -> String {
    let mut s = String::new();
    match kind {
        GradientKind::Linear => {
            write!(&mut s, "url(#linear-gradient-{}-{})", matrix[0], matrix[3]).unwrap();
        }
        GradientKind::Radial | GradientKind::Elliptical => {
            write!(&mut s, "url(#radial-gradient-{}-{})", matrix[0], matrix[3]).unwrap();
        }
        GradientKind::Conical => {
            write!(&mut s, "url(#conical-gradient-{}-{})", matrix[0], matrix[3]).unwrap();
        }
    }
    s
}

fn hatch_to_svg(hatch: &Hatch) -> String {
    let colour = colour_to_hex(&hatch.colour);
    format!("url(#hatch-pattern-{})", colour)
}

fn bitmap_to_svg(resource_id: &str, matrix: &[f64; 6]) -> String {
    format!("url(#bitmap-{}-{})", resource_id, matrix[0])
}

fn pattern_to_svg(resource_id: &str, matrix: &[f64; 6]) -> String {
    format!("url(#pattern-{}-{})", resource_id, matrix[0])
}

fn mesh_to_svg() -> String {
    "url(#mesh-gradient)".to_string()
}

fn diffusion_to_svg() -> String {
    "url(#diffusion-gradient)".to_string()
}
