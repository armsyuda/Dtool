use std::fmt::Write;

use dtool_core::paint::{Colour, GradientKind, GradientStop, Paint};

/// Make a Level 3 pattern in EPS page coordinates from a world-space gradient.
pub(crate) fn gradient_pattern(paint: &Paint, page_origin: [f64; 2]) -> Option<String> {
    let Paint::Gradient { kind, stops, geom } = paint else { return None };
    let dx = geom.end[0] - geom.start[0];
    let dy = geom.end[1] - geom.start[1];
    let length = dx.hypot(dy);
    if stops.is_empty() || !length.is_finite() || length <= 1e-14
        || stops.iter().any(|stop| !(0.0..=1.0).contains(&stop.offset) || stop.colour.a < 255)
        || stops.windows(2).any(|pair| pair[0].offset >= pair[1].offset)
    {
        return None;
    }
    let ratio = match kind {
        GradientKind::Linear | GradientKind::Radial => 1.0,
        GradientKind::Elliptical if geom.ratio.is_finite() && geom.ratio.abs() > 1e-14 => geom.ratio,
        GradientKind::Elliptical => 1.0,
        GradientKind::Conical => return None,
    };
    let x = geom.start[0] - page_origin[0];
    let y = page_origin[1] - geom.start[1];
    let [first, last] = [*stops.first()?, *stops.last()?];
    let mut stops = stops.clone();
    if first.offset > 0.0 {
        stops.insert(0, GradientStop { offset: 0.0, ..first });
    }
    if last.offset < 1.0 {
        stops.push(GradientStop { offset: 1.0, ..last });
    }
    let (kind, coords) = if *kind == GradientKind::Linear { (2, "0 0 1 0") } else { (3, "0 0 0 0 0 1") };
    let mut pattern = format!("[/Pattern] setcolorspace\n<< /PatternType 2 /Shading << /ShadingType {kind} /ColorSpace /DeviceRGB /Coords [{coords}] /Extend [true true] /Function ");
    if stops.len() == 2 {
        write_segment(&mut pattern, stops[0].colour, stops[1].colour);
    } else {
        pattern.push_str("<< /FunctionType 3 /Domain [0 1] /Functions [\n");
        for pair in stops.windows(2) {
            write_segment(&mut pattern, pair[0].colour, pair[1].colour);
        }
        pattern.push_str("] /Bounds [");
        for stop in &stops[1..stops.len() - 1] {
            let _ = write!(pattern, "{} ", stop.offset);
        }
        pattern.push_str("] /Encode [");
        for _ in stops.windows(2) {
            pattern.push_str("0 1 ");
        }
        pattern.push_str("] >>\n");
    }
    let _ = writeln!(pattern, ">> >> [{dx} {} {} {} {x} {y}] makepattern setpattern", -dy, -dy * ratio, -dx * ratio);
    Some(pattern)
}

fn write_segment(out: &mut String, from: Colour, to: Colour) {
    let rgb = |colour: Colour| [colour.r, colour.g, colour.b].map(|value| f64::from(value) / 255.0);
    let [r0, g0, b0] = rgb(from);
    let [r1, g1, b1] = rgb(to);
    let _ = writeln!(out, "<< /FunctionType 2 /Domain [0 1] /C0 [{r0} {g0} {b0}] /C1 [{r1} {g1} {b1}] /N 1 >>");
}

pub fn paint_to_eps(paint: &Paint) -> Option<String> {
    match paint {
        Paint::Solid { colour } => Some(solid_colour_to_eps(colour)),
        _ => None,
    }
}

fn solid_colour_to_eps(colour: &Colour) -> String {
    let r = colour.r as f64 / 255.0;
    let g = colour.g as f64 / 255.0;
    let b = colour.b as f64 / 255.0;
    format!("{} {} {} setrgbcolor", r, g, b)
}

pub fn paint_to_eps_fill(paint: &Paint) -> Option<String> {
    match paint {
        Paint::Solid { colour } => Some(format!(
            "{} {} {} setrgbcolor fill",
            colour.r as f64 / 255.0,
            colour.g as f64 / 255.0,
            colour.b as f64 / 255.0
        )),
        _ => None,
    }
}

pub fn paint_to_eps_stroke(paint: &Paint) -> Option<String> {
    match paint {
        Paint::Solid { colour } => Some(format!(
            "{} {} {} setrgbcolor stroke",
            colour.r as f64 / 255.0,
            colour.g as f64 / 255.0,
            colour.b as f64 / 255.0
        )),
        _ => None,
    }
}
