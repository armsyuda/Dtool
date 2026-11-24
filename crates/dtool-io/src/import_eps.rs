use std::fs;
use std::io::Read;
use std::path::Path;

use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::{Document, ObjectKind};

use crate::import_report::{ImportReport, LossKind};
use crate::native::{IoError, IoErrorKind};

pub fn import_eps(path: &Path) -> Result<(Document, ImportReport), IoError> {
    let mut buf = Vec::new();
    fs::File::open(path).map_err(|e| IoError::from_io(&e))?.read_to_end(&mut buf).map_err(|e| IoError::from_io(&e))?;

    let eps_data = extract_eps_postscript(&buf);
    let ps_text = String::from_utf8_lossy(&eps_data).to_string();
    if !ps_text.trim_start().starts_with("%!PS") {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    let bbox = parse_bounding_box(&ps_text);
    if !(bbox.2 > 0.0 && bbox.3 > 0.0 && bbox.2.is_finite() && bbox.3.is_finite()) {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    let mut doc = Document::new([(bbox.2) as f32, (bbox.3) as f32], dtool_core::document::Units::Px, 72.0, 1);
    let mut report = ImportReport::default();

    interpret_postscript(&ps_text, bbox, &mut doc, &mut report);

    Ok((doc, report))
}

fn extract_eps_postscript(data: &[u8]) -> Vec<u8> {
    if data.len() >= 4 && data[0..2] == [0x25, 0x21] {
        return data.to_vec();
    }

    let mut start = 0;
    while start < data.len() {
        if data[start..].starts_with(b"%!PS-Adobe") {
            break;
        }
        start += 1;
    }

    if start == 0 && !data.starts_with(b"%!PS-Adobe") {
        return data.to_vec();
    }

    let mut end = data.len();
    while end > start {
        if data[end - 4..end] == [0x25, 0x21, 0x0A, 0x0A] || data[end - 3..end] == [0x25, 0x21, 0x0A] {
            break;
        }
        end -= 1;
    }

    if end <= start {
        return data[start..].to_vec();
    }

    data[start..end].to_vec()
}

fn parse_bounding_box(ps: &str) -> (f64, f64, f64, f64) {
    for line in ps.lines() {
        if line.starts_with("%%BoundingBox:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                let x0 = parts[1].parse::<f64>().unwrap_or(0.0);
                let y0 = parts[2].parse::<f64>().unwrap_or(0.0);
                let x1 = parts[3].parse::<f64>().unwrap_or(100.0);
                let y1 = parts[4].parse::<f64>().unwrap_or(100.0);
                return (x0, y0, x1 - x0, y1 - y0);
            }
        }
    }
    (0.0, 0.0, 100.0, 100.0)
}

enum Token {
    Number(f64),
    Text(String),
    Name(String),
}

fn tokens(ps: &str) -> Vec<Token> {
    let chars: Vec<char> = ps.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '%' {
            while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                i += 1;
            }
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '(' {
            let mut depth = 1;
            let mut text = String::new();
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() {
                    text.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                text.push(ch);
                i += 1;
            }
            out.push(Token::Text(text));
            continue;
        }
        if matches!(c, '{' | '}' | '[' | ']') {
            out.push(Token::Name(c.to_string()));
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && !matches!(chars[i], '(' | ')' | '{' | '}' | '[' | ']' | '%') {
            i += 1;
        }
        if i == start {
            i += 1;
            continue;
        }
        let word: String = chars[start..i].iter().collect();
        match word.parse::<f64>() {
            Ok(value) if value.is_finite() => out.push(Token::Number(value)),
            _ => out.push(Token::Name(word)),
        }
    }
    out
}

struct PsPath {
    subpaths: Vec<(Vec<(f64, f64)>, bool)>,
}

impl PsPath {
    fn point(&mut self, p: (f64, f64), start: bool) {
        if start || self.subpaths.is_empty() {
            self.subpaths.push((vec![p], false));
        } else if let Some(last) = self.subpaths.last_mut() {
            last.0.push(p);
        }
    }
}

fn interpret_postscript(ps: &str, bbox: (f64, f64, f64, f64), doc: &mut Document, report: &mut ImportReport) {
    let map = |x: f64, y: f64| (x - bbox.0, bbox.1 + bbox.3 - y);
    let mut stack: Vec<f64> = Vec::new();
    let mut strings: Vec<String> = Vec::new();
    let mut path = PsPath { subpaths: Vec::new() };
    let mut colour = (0.0, 0.0, 0.0);
    let mut line_width = 1.0;
    let mut saved = Vec::new();
    let mut unsupported = 0u32;
    let mut curves = 0u32;
    let mut cmyk = 0u32;
    for token in tokens(ps) {
        let name = match token {
            Token::Number(value) => {
                stack.push(value);
                continue;
            }
            Token::Text(text) => {
                strings.push(text);
                continue;
            }
            Token::Name(name) => name,
        };
        if name.starts_with('/') || matches!(name.as_str(), "{" | "}" | "[" | "]") {
            continue;
        }
        match (name.as_str(), stack.as_slice()) {
            ("newpath", _) => path.subpaths.clear(),
            ("moveto", [.., x, y]) => path.point(map(*x, *y), true),
            ("lineto", [.., x, y]) => path.point(map(*x, *y), false),
            ("curveto", [.., _, _, _, _, x, y]) => {
                curves += 1;
                path.point(map(*x, *y), false);
            }
            ("closepath", _) => {
                if let Some(last) = path.subpaths.last_mut() {
                    last.1 = true;
                }
            }
            ("fill" | "eofill", _) => {
                if add_shape(doc, &path.subpaths, Some(colour), None, line_width) {
                    report.objects += 1;
                }
                path.subpaths.clear();
            }
            ("stroke", _) => {
                if add_shape(doc, &path.subpaths, None, Some(colour), line_width) {
                    report.objects += 1;
                }
                path.subpaths.clear();
            }
            ("setrgbcolor", [.., r, g, b]) => colour = (*r, *g, *b),
            ("setgray", [.., g]) => colour = (*g, *g, *g),
            ("setcmykcolor", [.., c, m, y, k]) => {
                cmyk += 1;
                colour = (1.0 - (c + k).min(1.0), 1.0 - (m + k).min(1.0), 1.0 - (y + k).min(1.0));
            }
            ("setlinewidth", [.., w]) => line_width = *w,
            ("gsave", _) => saved.push((colour, line_width)),
            ("grestore", _) => {
                if let Some((c, w)) = saved.pop() {
                    colour = c;
                    line_width = w;
                }
            }
            ("show", _) => {
                if let Some(text) = strings.pop() {
                    add_text(doc, &text, 12.0);
                    report.texts += 1;
                }
            }
            ("showpage", _) => {}
            _ => unsupported += 1,
        }
        stack.clear();
        strings.clear();
    }
    if curves > 0 {
        report.add_loss(LossKind::Other, curves, false, "PostScript curves approximated by their end points");
    }
    if cmyk > 0 {
        report.add_loss(LossKind::Other, cmyk, false, "CMYK color space converted to RGB approximation");
    }
    if unsupported > 0 {
        report.add_loss(LossKind::Other, unsupported, false, "PostScript operators not interpreted");
    }
}

fn add_shape(doc: &mut Document, subpaths: &[(Vec<(f64, f64)>, bool)], fill: Option<(f64, f64, f64)>, stroke: Option<(f64, f64, f64)>, width: f64) -> bool {
    let all: Vec<(f64, f64)> = subpaths.iter().flat_map(|(points, _)| points.iter().copied()).collect();
    if all.len() < 2 {
        return false;
    }
    let min_x = all.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = all.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let min_y = all.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = all.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let w = max_x - min_x;
    let h = max_y - min_y;
    if !(w.is_finite() && h.is_finite()) || (w <= 0.0 && h <= 0.0) {
        return false;
    }
    let mut next = 0u64;
    let mut contours = subpaths
        .iter()
        .filter(|(points, _)| points.len() >= 2)
        .map(|(points, closed)| Contour {
            points: points
                .iter()
                .map(|p| {
                    next += 1;
                    PathPoint::sharp(PointId(next), [p.0 - min_x, p.1 - min_y])
                })
                .collect(),
            closed: *closed || fill.is_some(),
        })
        .collect::<Vec<_>>()
        .into_iter();
    let first = contours.next().unwrap_or_default();
    let mut obj = dtool_core::Object::default();
    obj.name = "eps_path".into();
    obj.kind = ObjectKind::Curve(Curve::from_contour_and_holes(first, contours.collect()));
    obj.pos = [min_x as f32, min_y as f32];
    obj.size = [w.max(0.0) as f32, h.max(0.0) as f32];
    let solid = |(r, g, b): (f64, f64, f64)| dtool_core::paint::Paint::Solid {
        colour: dtool_core::paint::Colour::new(
            (r * 255.0).clamp(0.0, 255.0) as u8,
            (g * 255.0).clamp(0.0, 255.0) as u8,
            (b * 255.0).clamp(0.0, 255.0) as u8,
            255,
        ),
    };
    if let Some(colour) = fill {
        obj.fill = solid(colour);
    }
    if let Some(colour) = stroke {
        obj.stroke.paint = solid(colour);
        obj.stroke.width = width as f32;
    }
    doc.create_object(obj);
    true
}

fn add_text(doc: &mut Document, text: &str, size: f64) {
    if text.is_empty() {
        return;
    }
    let mut obj = dtool_core::Object::default();
    obj.kind = ObjectKind::Text(text.to_string());
    obj.pos = [0.0, 0.0];
    obj.size = [(text.len() as f64 * size * 0.5) as f32, size as f32];
    doc.create_object(obj);
}
