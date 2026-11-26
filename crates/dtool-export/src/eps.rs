use std::fmt::Write;

use dtool_core::commands::appearance;
use dtool_core::paint::Paint;
use dtool_core::{Document, Object, ObjectKind};
use dtool_protocol::{CommandError, CommandResult};
use crate::{DocumentExportExt, ExportArea, ExportControl, ExportReceipt, ExportRequest, ObjectExportExt};

pub const EPS_HEADER: &str = "%!PS-Adobe-3.0 EPSF-3.0";
pub const EPS_FONT: &str = "Helvetica";
pub const EPS_LINE_HEIGHT: f32 = 1.2;
pub const EPS_DEFAULT_FONT_SIZE: f32 = 12.0;
const FLATTEN_TOLERANCE: f64 = 0.05;

struct Frame {
    ox: f32,
    oy: f32,
    width: f32,
    height: f32,
}

impl Frame {
    fn point(&self, x: f32, y: f32) -> String {
        format!("{} {}", num(x - self.ox), num(self.height - (y - self.oy)))
    }
}

fn area_frame(doc: &Document, area: &ExportArea) -> Result<Frame, CommandError> {
    let (ox, oy, width, height) = match area {
        ExportArea::AllPages | ExportArea::AllSpreads | ExportArea::PageRun { .. } => return Err(crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.export.option_unavailable")),
        area @ (ExportArea::WholeDocument | ExportArea::CurrentPage | ExportArea::CurrentSpread | ExportArea::Page(_) | ExportArea::Spread(_)) => doc.area_bounds(area).ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.export.bad_area"))?,
        ExportArea::Rect([x, y, w, h]) => (*x, *y, *w, *h),
        ExportArea::Artboard(id) => crate::raster::artboard_region(doc, *id).ok_or_else(|| crate::export_error("NOT_FOUND", "error.export.bad_area"))?,
        ExportArea::SelectionWithBackground | ExportArea::SelectionWithoutBackground => {
            doc.selection_bounds().ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.export.bad_area"))?
        }
    };
    if !(width.is_finite() && height.is_finite() && ox.is_finite() && oy.is_finite()) || width <= 0.0 || height <= 0.0 {
        return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area"));
    }
    Ok(Frame { ox, oy, width, height })
}

pub fn num(value: f32) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let mut text = format!("{:.3}", value);
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        text = "0".to_string();
    }
    text
}

pub fn ps_string(text: &str) -> (String, bool) {
    let mut out = String::with_capacity(text.len() + 2);
    let mut missing = false;
    out.push('(');
    for ch in text.chars() {
        match ch {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '\t' => out.push(' '),
            ' '..='~' => out.push(ch),
            _ if (0xA0..=0xFF).contains(&(ch as u32)) => out.push_str(&format!("\\{:03o}", ch as u32)),
            _ => {
                out.push('?');
                missing = true;
            }
        }
    }
    out.push(')');
    (out, missing)
}

fn ascii_title(title: &str) -> String {
    let cleaned: String = title.chars().map(|c| if (' '..='~').contains(&c) { c } else { '_' }).collect();
    ps_string(&cleaned).0
}

fn colour_op(paint: &Paint, id: u64, warnings: &mut Vec<String>) -> Option<String> {
    if matches!(paint, Paint::None) {
        return None;
    }
    if let Some(op) = crate::eps_paint::paint_to_eps(paint) {
        if crate::paint_colour(paint).is_some_and(|colour| colour.a < 255) {
            warnings.push(format!("export.eps.transparency_flattened:{id}"));
        }
        return Some(op);
    }
    let colour = crate::paint_colour(paint)?;
    warnings.push(format!("export.eps.paint_flattened:{id}"));
    Some(format!("{} {} {} setrgbcolor", num(f32::from(colour.r) / 255.0), num(f32::from(colour.g) / 255.0), num(f32::from(colour.b) / 255.0)))
}

fn object_contours(doc: &Document, object: &Object) -> Option<Vec<(Vec<[f64; 2]>, bool)>> {
    if let Some(path) = dtool_core::outline::object_outline_world(object) {
        let path = dtool_core::commands::warp::live_warp_outline(doc, object.id, &path).unwrap_or(path);
        let contours = dtool_core::outline::flatten_subpaths(&path, FLATTEN_TOLERANCE);
        if !contours.is_empty() {
            return Some(contours);
        }
    }
    if matches!(object.kind, ObjectKind::Shape) {
        let points: Vec<[f64; 2]> = object.outline_points().into_iter().map(|(x, y)| [f64::from(x), f64::from(y)]).collect();
        if points.len() > 1 {
            return Some(vec![(points, true)]);
        }
    }
    None
}

fn write_contours(out: &mut String, frame: &Frame, contours: &[(Vec<[f64; 2]>, bool)]) {
    out.push_str("newpath\n");
    for (points, closed) in contours {
        for (index, point) in points.iter().enumerate() {
            let op = if index == 0 { "moveto" } else { "lineto" };
            out.push_str(&format!("{} {}\n", frame.point(point[0] as f32, point[1] as f32), op));
        }
        if *closed {
            out.push_str("closepath\n");
        }
    }
}

fn write_stroke(out: &mut String, object: &Object, warnings: &mut Vec<String>) -> bool {
    let width = object.stroke.width;
    if !width.is_finite() || width <= 0.0 {
        return false;
    }
    let Some(colour) = colour_op(&object.stroke.paint, object.id.0, warnings) else { return false; };
    let dash: Vec<String> = if object.stroke.dash.iter().any(|length| *length > 0.0 && length.is_finite()) {
        object.stroke.dash.iter().filter(|v| v.is_finite() && **v >= 0.0).map(|v| num(*v)).collect()
    } else {
        Vec::new()
    };
    use dtool_core::paint::{Cap, Join};
    let cap = match object.stroke.cap {
        Cap::Butt => 0,
        Cap::Round | Cap::RoundInset => 1,
        Cap::Square => 2,
    };
    let join = match object.stroke.join {
        Join::Miter { .. } => 0,
        Join::Round => 1,
        Join::Bevel => 2,
    };
    if let Join::Miter { limit } = object.stroke.join
        && limit.is_finite() && limit >= 1.0
    {
        let _ = writeln!(out, "{} setmiterlimit", num(limit));
    }
    let offset = if dash.is_empty() { 0.0 } else { object.stroke.dash_offset };
    let _ = writeln!(out, "{colour}\n{} setlinewidth\n{cap} setlinecap\n{join} setlinejoin\n[{}] {} setdash\nstroke", num(width), dash.join(" "), num(offset));
    true
}

fn world_paint_op(paint: &Paint, id: u64, frame: &Frame, warnings: &mut Vec<String>) -> Option<String> {
    crate::eps_paint::gradient_pattern(paint, [f64::from(frame.ox), f64::from(frame.oy + frame.height)])
        .or_else(|| colour_op(paint, id, warnings))
}

fn write_appearance(out: &mut String, warnings: &mut Vec<String>, doc: &Document, object: &Object, frame: &Frame) {
    let Some(layers) = appearance::vector_paths(doc, object, &object.fill, false) else {
        warnings.push(format!("export.eps.object_skipped:{}", object.id.0));
        return;
    };
    for layer in layers {
        let Some(colour) = world_paint_op(&layer.paint, object.id.0, frame, warnings) else { continue };
        let contours = dtool_core::outline::flatten_subpaths(&layer.path, FLATTEN_TOLERANCE);
        write_contours(out, frame, &contours);
        let op = if layer.even_odd { "eofill" } else { "fill" };
        let _ = writeln!(out, "{colour}\n{op}");
    }
}

fn write_vector(out: &mut String, warnings: &mut Vec<String>, doc: &Document, object: &Object, frame: &Frame) {
    let Some(contours) = object_contours(doc, object) else {
        warnings.push(format!("export.eps.object_skipped:{}", object.id.0));
        return;
    };
    let fill = if contours.iter().any(|(_, closed)| *closed) { colour_op(&object.fill, object.id.0, warnings) } else { None };
    write_contours(out, frame, &contours);
    if let Some(colour) = fill {
        let op = if object.fill_rule.is_even_odd() { "eofill" } else { "fill" };
        out.push_str(&format!("gsave\n{colour}\n{op}\ngrestore\n"));
    }
    let arrows = if write_stroke(out, object, warnings) { crate::arrowheads_for(Some(doc), object) } else { Vec::new() };
    if !arrows.is_empty() {
        // The shaft has already set the stroke colour. Arrowheads are always solid.
        out.push_str("[] 0 setdash\n");
        for head in arrows {
            let points = head.points.into_iter().map(|point| point.map(f64::from)).collect();
            write_contours(out, frame, &[(points, head.filled)]);
            if head.filled {
                out.push_str("fill\n");
            } else {
                let _ = writeln!(out, "{} setlinewidth\nstroke", num(head.line_width));
            }
        }
    }
    out.push_str("newpath\n");
}

fn write_text(out: &mut String, warnings: &mut Vec<String>, doc: &Document, object: &Object, frame: &Frame) {
    if dtool_core::scene::has_live_text_warp(doc, object.id) {
        dtool_text::canvas_text::install();
        let lines = dtool_core::text_lines::text_lines(doc, object.id);
        if let Some(ink) = dtool_core::scene::warped_text_ink(doc, object, &lines, object.opacity) {
            for prim in ink {
                let dtool_core::scene::Prim::Fill { contours, paint, .. } = prim else { continue };
                let Some(colour) = world_paint_op(&paint, object.id.0, frame, warnings) else { continue };
                let rings: Vec<_> = contours.into_iter()
                    .map(|contour| (contour.into_iter().map(|point| point.map(f64::from)).collect(), true))
                    .collect();
                write_contours(out, frame, &rings);
                let _ = writeln!(out, "{colour}\neofill");
            }
            return;
        }
        warnings.push(format!("export.eps.object_skipped:{}", object.id.0));
        return;
    }
    let Some(text) = doc.object_text(object) else {
        warnings.push(format!("export.eps.object_skipped:{}", object.id.0));
        return;
    };
    if matches!(object.kind, ObjectKind::PathText(_)) {
        let library = dtool_text::FontLibrary::shared();
        if let Some(glyphs) = dtool_text::path_text::export_path_text_contours(&library, doc, object) {
            let source = dtool_text::path_text::path_text_paint(doc, object);
            let paint = colour_op(&source, object.id.0, warnings).unwrap_or_else(|| "0 0 0 setrgbcolor".into());
            let contours: Vec<_> = glyphs.into_iter()
                .filter(|contour| contour.len() >= 3)
                .map(|contour| (contour.into_iter().map(|point| point.map(f64::from)).collect(), true))
                .collect();
            out.push_str(&paint);
            out.push('\n');
            write_contours(out, frame, &contours);
            out.push_str("eofill\n");
            return;
        }
        warnings.push(format!("export.eps.path_text_flattened:{}", object.id.0));
    }
    let size = object.font_size().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(EPS_DEFAULT_FONT_SIZE);
    let colour = colour_op(&object.fill, object.id.0, warnings).unwrap_or_else(|| "0 0 0 setrgbcolor".to_string());
    out.push_str(&format!("{colour}\n/DToolFont findfont {} scalefont setfont\n", num(size)));
    let mut missing = false;
    for (index, line) in text.lines().enumerate() {
        let baseline = object.y() + size + index as f32 * size * EPS_LINE_HEIGHT;
        let (literal, line_missing) = ps_string(line);
        missing |= line_missing;
        out.push_str(&format!("{} moveto\n{literal} show\n", frame.point(object.x(), baseline)));
    }
    if missing {
        warnings.push(format!("export.eps.text_glyph_missing:{}", object.id.0));
    }
}

fn write_dimension(out: &mut String, warnings: &mut Vec<String>, object: &Object, frame: &Frame) {
    let Some(geometry) = dtool_core::outline::dimension_geometry(object) else {
        warnings.push(format!("export.eps.object_skipped:{}", object.id.0));
        return;
    };
    let colour = colour_op(&object.stroke.paint, object.id.0, warnings)
        .or_else(|| colour_op(&object.fill, object.id.0, warnings))
        .unwrap_or_else(|| "0 0 0 setrgbcolor".to_string());
    let width = if object.stroke.width.is_finite() && object.stroke.width > 0.0 { object.stroke.width } else { 1.0 };
    let mut segments = vec![geometry.line];
    segments.extend(geometry.extensions.iter().copied());
    segments.extend(geometry.ticks.iter().copied());
    out.push_str(&format!("{colour}\n{} setlinewidth\n[] 0 setdash\nnewpath\n", num(width)));
    for [a, b] in segments {
        out.push_str(&format!("{} moveto\n{} lineto\n", frame.point(a[0] as f32, a[1] as f32), frame.point(b[0] as f32, b[1] as f32)));
    }
    out.push_str("stroke\n");
    let (literal, missing) = ps_string(&geometry.label);
    if missing {
        warnings.push(format!("export.eps.text_glyph_missing:{}", object.id.0));
    }
    out.push_str(&format!(
        "gsave\n{} translate\n{} rotate\n/DToolFont findfont {} scalefont setfont\n{literal} dup stringwidth pop -2 div 0 moveto show\ngrestore\n",
        frame.point(geometry.label_at[0] as f32, geometry.label_at[1] as f32),
        num(-(geometry.label_angle_deg as f32)),
        num(dtool_core::outline::DIMENSION_LABEL_SIZE as f32)
    ));
}

fn write_crop(out: &mut String, object: &Object, frame: &Frame) {
    let Some(quad) = dtool_core::scene::vector_crop_quad(object) else { return; };
    out.push_str(&format!(
        "newpath\n{} moveto\n{} lineto\n{} lineto\n{} lineto\nclosepath\nclip\nnewpath\n",
        frame.point(quad[0][0], quad[0][1]),
        frame.point(quad[1][0], quad[1][1]),
        frame.point(quad[2][0], quad[2][1]),
        frame.point(quad[3][0], quad[3][1])
    ));
}

fn write_header(out: &mut String, frame: &Frame, title: &str, language_level: u8) {
    out.push_str(EPS_HEADER);
    out.push('\n');
    out.push_str(&format!("%%BoundingBox: 0 0 {} {}\n", frame.width.ceil() as i64, frame.height.ceil() as i64));
    out.push_str(&format!("%%HiResBoundingBox: 0 0 {} {}\n", num(frame.width), num(frame.height)));
    out.push_str("%%Creator: DTool\n");
    out.push_str(&format!("%%Title: {}\n", ascii_title(title)));
    let _ = writeln!(out, "%%LanguageLevel: {language_level}\n%%Pages: 1\n%%DocumentData: Clean7Bit");
    out.push_str(&format!("%%DocumentNeededResources: font {EPS_FONT}\n%%EndComments\n"));
    out.push_str("%%BeginProlog\n");
    out.push_str(&format!(
        "/DToolFont /{EPS_FONT} findfont dup length dict begin {{1 index /FID ne {{def}} {{pop pop}} ifelse}} forall /Encoding ISOLatin1Encoding def currentdict end definefont pop\n"
    ));
    out.push_str("%%EndProlog\n%%Page: 1 1\n");
}

fn needs_composite(doc: &Document, object: &Object) -> bool {
    !object.blend_mode.is_normal()
        || object.blend_ranges.is_some()
        || object.is_group() && (object.opacity < 1.0 || !matches!(object.transparency, dtool_core::paint::Transparency::None))
        || doc.layer_masks.iter().any(|mask| mask.object == object.id)
}

fn uses_vector_appearance(object: &Object) -> bool {
    appearance::has_layers(object)
        || matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Compound(_))
            && (matches!(object.fill, Paint::Gradient { .. }) || matches!(object.stroke.paint, Paint::Gradient { .. })
                || object.stroke.cap == dtool_core::paint::Cap::RoundInset)
}

fn appearance_needs_composite(object: &Object) -> bool {
    if !uses_vector_appearance(object) {
        return false;
    }
    let fills = appearance::visible_fills(object, &object.fill);
    let strokes = appearance::visible_strokes(object)
        .filter(|stroke| stroke.width > 0.0)
        .map(|stroke| &stroke.paint);
    object.opacity < 1.0 || !object.appearance_blends.is_empty()
        || !matches!(object.transparency, dtool_core::paint::Transparency::None)
        || fills.chain(strokes).any(|paint| match paint {
            Paint::None => false,
            Paint::Solid { colour } => colour.a < 255,
            Paint::Gradient { .. } => crate::eps_paint::gradient_pattern(paint, [0.0; 2]).is_none(),
            _ => true,
        })
}

fn write_composite(out: &mut String, doc: &Document, area: &ExportArea, frame: &Frame, control: &ExportControl) -> Result<(), CommandError> {
    let per_unit = crate::raster::document_unit_points(doc) * doc.dpi.max(1.0) / 72.0;
    let region = (frame.ox, frame.oy, frame.width, frame.height);
    let pixel_region = (0.0, 0.0, frame.width * per_unit, frame.height * per_unit);
    let opts = crate::raster::ExportOptions::for_format(crate::ExportFormat::Png);
    let (width, height) = crate::raster::target_size(pixel_region, doc.dpi, &opts)
        .map_err(|_| crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.export.option_unavailable"))?;
    let rgba = crate::raster::render_rgba_for_area(doc, area, region, [width as usize, height as usize]);
    if control.is_cancelled() {
        return Err(crate::export_cancelled());
    }
    // Level 2 PostScript has no alpha compositing against the destination.
    let rgb = crate::flatten_rgba(&rgba, [255; 3]);
    let _ = writeln!(out, "gsave\n{} {} scale\n/DeviceRGB setcolorspace\n1 dict begin", num(frame.width), num(frame.height));
    let _ = writeln!(out, "{{ /pixels currentfile /ASCIIHexDecode filter def\n<< /ImageType 1 /Width {width} /Height {height}\n/BitsPerComponent 8 /Decode [0 1 0 1 0 1]\n/ImageMatrix [{width} 0 0 -{height} 0 {height}] /DataSource pixels >> image\npixels flushfile }} exec");
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for row in rgb.chunks(width as usize * 3) {
        if control.is_cancelled() {
            return Err(crate::export_cancelled());
        }
        for chunk in row.chunks(36) {
            for byte in chunk {
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 15) as usize] as char);
            }
            out.push('\n');
        }
    }
    out.push_str(">\nend\ngrestore\n");
    Ok(())
}

pub fn eps_document(doc: &Document, area: &ExportArea, title: &str, control: &ExportControl) -> Result<(String, Vec<String>), CommandError> {
    if control.is_cancelled() {
        return Err(crate::export_cancelled());
    }
    let frame = area_frame(doc, area)?;
    let mut out = String::new();
    let mut warnings = Vec::new();
    let region = (frame.ox, frame.oy, frame.width, frame.height);
    let members = crate::artboard_members(doc, area);
    let eligible = |object: &Object| members.as_ref().is_none_or(|ids| ids.contains(&object.id))
        && !dtool_core::artboard::is_artboard(object)
        && crate::export_visible(doc, object)
        && !dtool_core::clip_mask::is_mask(doc, object);
    if doc.objects().iter().any(|object| {
        eligible(object)
            && (needs_composite(doc, object)
                || crate::vector_bounds_intersect(crate::vector_object_bounds(doc, object), region)
                    && (appearance_needs_composite(object)
                        || matches!(object.fill, Paint::Bitmap(_))
                        || dtool_core::path_brush::Ink::new(doc, object).is_some()))
    }) {
        write_header(&mut out, &frame, title, 2);
        write_composite(&mut out, doc, area, &frame, control)?;
        out.push_str("showpage\n%%Trailer\n%%EOF\n");
        return Ok((out, vec!["export.eps.composite_rasterized".to_string()]));
    }
    for object in doc.objects() {
        if control.is_cancelled() {
            return Err(crate::export_cancelled());
        }
        if !eligible(object) {
            continue;
        }
        if object.opacity < 1.0 {
            warnings.push(format!("export.eps.transparency_flattened:{}", object.id.0));
        }
        if object.is_group() {
            continue;
        }
        let has_appearance = uses_vector_appearance(object);
        if has_appearance && !crate::vector_bounds_intersect(crate::vector_object_bounds(doc, object), region) {
            continue;
        }
        let clip = dtool_core::clip_mask::layer_clip_region(doc, object.id);
        if clip.as_ref().is_some_and(Vec::is_empty) {
            continue;
        }
        out.push_str("gsave\n");
        if let Some(board) = dtool_core::artboard::owner(doc, object.id) {
            write_contours(&mut out, &frame, &[(crate::artboard_ring(board), true)]);
            out.push_str("clip\nnewpath\n");
        }
        if let Some(clip) = clip {
            let contours: Vec<_> = clip.into_iter().map(|ring| (ring, true)).collect();
            write_contours(&mut out, &frame, &contours);
            out.push_str("eoclip\nnewpath\n");
        }
        write_crop(&mut out, object, &frame);
        match &object.kind {
            _ if has_appearance => write_appearance(&mut out, &mut warnings, doc, object, &frame),
            ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Compound(_) => write_vector(&mut out, &mut warnings, doc, object, &frame),
            ObjectKind::Text(_) | ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_) => {
                write_text(&mut out, &mut warnings, doc, object, &frame)
            }
            ObjectKind::Table(_) => {
                for prim in dtool_render::scene::table_prims(doc, object) {
                    let dtool_core::scene::Prim::Fill { contours, paint, .. } = prim else { continue };
                    let Some(colour) = colour_op(&paint, object.id.0, &mut warnings) else { continue };
                    let rings: Vec<_> = contours.into_iter().filter(|contour| contour.len() >= 3)
                        .map(|contour| (contour.into_iter().map(|point| point.map(f64::from)).collect(), true))
                        .collect();
                    out.push_str(&colour);
                    out.push('\n');
                    write_contours(&mut out, &frame, &rings);
                    out.push_str("eofill\n");
                }
            }
            ObjectKind::Dimension(_) => write_dimension(&mut out, &mut warnings, object, &frame),
            _ => warnings.push(format!("export.eps.object_skipped:{}", object.id.0)),
        }
        out.push_str("grestore\n");
    }
    out.push_str("showpage\n%%Trailer\n%%EOF\n");
    let language_level = if out.contains("makepattern setpattern\n") { 3 } else { 2 };
    let mut document = String::new();
    write_header(&mut document, &frame, title, language_level);
    document.push_str(&out);
    Ok((document, warnings))
}

pub fn export_eps(doc: &Document, req: &ExportRequest, control: &ExportControl) -> Result<ExportReceipt, CommandError> {
    let (bytes, warnings) = encode_eps(doc, req, control)?;
    if control.is_cancelled() {
        return Err(crate::export_cancelled());
    }
    crate::write_atomic(&req.path, &bytes).map_err(crate::export_io_error)?;
    Ok(ExportReceipt { path: req.path.clone(), bytes: bytes.len() as u64, warnings })
}

pub(crate) fn encode_eps(doc: &Document, req: &ExportRequest, control: &ExportControl) -> Result<(Vec<u8>, Vec<String>), CommandError> {
    let title = req.path.file_stem().map(|stem| stem.to_string_lossy().to_string()).unwrap_or_default();
    let (text, warnings) = eps_document(doc, &req.area, &title, control)?;
    Ok((text.into_bytes(), warnings))
}

pub fn export_eps_value(doc: &Document, req: &ExportRequest, control: &ExportControl) -> CommandResult {
    let receipt = export_eps(doc, req, control)?;
    Ok(serde_json::json!({
        "path": receipt.path.to_string_lossy(),
        "bytes": receipt.bytes,
        "warnings": receipt.warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
    use dtool_core::document::{Story, StoryId, Units};
    use dtool_core::object::ObjectId;
    use dtool_core::paint::Colour;
    use serde_json::json;

    fn red() -> Paint {
        Paint::Solid { colour: Colour::new(255, 0, 0, 255) }
    }

    fn fixture() -> Document {
        let mut doc = Document::new([100.0, 50.0], Units::Pt, 72.0, 1);
        let mut rect = Object { id: ObjectId(1), name: "rect".to_owned(), pos: [10.0, 10.0], size: [20.0, 10.0], fill: red(), kind: ObjectKind::Shape, ..Object::default() };
        rect.stroke.paint = Paint::Solid { colour: Colour::new(0, 0, 255, 255) };
        rect.stroke.width = 2.0;
        doc.objects.push(rect);
        let mut line = Object {
            id: ObjectId(2),
            name: "line".to_owned(),
            pos: [0.0, 0.0],
            size: [100.0, 50.0],
            kind: ObjectKind::Curve(Curve {
                contour: Contour { points: vec![PathPoint::sharp(PointId(1), [5.0, 40.0]), PathPoint::sharp(PointId(2), [95.0, 40.0])], closed: false },
                holes: Vec::new(),
                source_object: None,
            }),
            ..Object::default()
        };
        line.stroke.paint = Paint::Solid { colour: Colour::new(0, 0, 0, 255) };
        line.stroke.width = 1.0;
        doc.objects.push(line);
        doc.stories.push(Story { id: StoryId(1), text: "Hi (EPS) \u{e9}\u{d55c}".to_owned(), ranges: vec![json!({"start":0,"end":10,"size_pt":12.0})], ..Story::default() });
        doc.objects.push(Object { id: ObjectId(3), name: "text".to_owned(), pos: [40.0, 5.0], size: [50.0, 12.0], fill: Paint::Solid { colour: Colour::new(0, 0, 0, 255) }, kind: ObjectKind::ArtText { story: StoryId(1) }, ..Object::default() });
        doc.next_object_id = 4;
        doc
    }

    fn render(doc: &Document) -> (String, Vec<String>) {
        eps_document(doc, &ExportArea::WholeDocument, "fixture", &ExportControl::new()).unwrap()
    }

    #[test]
    fn header_is_dsc_epsf_with_bounding_box() {
        let (text, _) = render(&fixture());
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("%!PS-Adobe-3.0 EPSF-3.0"));
        assert_eq!(lines.next(), Some("%%BoundingBox: 0 0 100 50"));
        assert_eq!(lines.next(), Some("%%HiResBoundingBox: 0 0 100 50"));
        assert!(text.contains("%%Title: (fixture)"));
        assert!(text.contains("%%EndComments\n"));
        assert!(text.ends_with("showpage\n%%Trailer\n%%EOF\n"));
        assert!(text.is_ascii());
    }

    #[test]
    fn shape_is_written_as_closed_path_with_fill_and_stroke() {
        let (text, _) = render(&fixture());
        assert!(text.contains("10 40 moveto\n"));
        assert!(text.contains("30 40 lineto\n"));
        assert!(text.contains("30 30 lineto\n"));
        assert!(text.contains("10 30 lineto\nclosepath\n"));
        assert!(text.contains("gsave\n1 0 0 setrgbcolor\nfill\ngrestore\n"));
        assert!(text.contains("10 setmiterlimit\n0 0 1 setrgbcolor\n2 setlinewidth\n0 setlinecap\n0 setlinejoin\n[] 0 setdash\nstroke\n"));
    }

    #[test]
    fn open_curve_is_stroked_and_not_filled() {
        let (text, _) = render(&fixture());
        assert!(text.contains("5 10 moveto\n95 10 lineto\n10 setmiterlimit\n0 0 0 setrgbcolor\n1 setlinewidth\n0 setlinecap\n0 setlinejoin\n[] 0 setdash\nstroke\n"));
    }

    #[test]
    fn text_is_positioned_show_with_escaped_latin1_and_missing_glyph_warning() {
        let (text, warnings) = render(&fixture());
        assert!(text.contains("/DToolFont /Helvetica findfont"));
        assert!(text.contains("ISOLatin1Encoding"));
        assert!(text.contains("/DToolFont findfont 12 scalefont setfont\n40 33 moveto\n(Hi \\(EPS\\) \\351?) show\n"));
        assert!(warnings.contains(&"export.eps.text_glyph_missing:3".to_string()));
    }

    #[test]
    fn rect_area_shifts_origin_and_bounding_box() {
        let doc = fixture();
        let (text, _) = eps_document(&doc, &ExportArea::Rect([10.0, 10.0, 20.0, 10.0]), "crop", &ExportControl::new()).unwrap();
        assert!(text.contains("%%BoundingBox: 0 0 20 10\n"));
        assert!(text.contains("0 10 moveto\n20 10 lineto\n20 0 lineto\n0 0 lineto\nclosepath\n"));
    }

    #[test]
    fn bad_area_and_cancel_are_errors() {
        let doc = fixture();
        assert_eq!(eps_document(&doc, &ExportArea::Rect([0.0, 0.0, 0.0, 10.0]), "x", &ExportControl::new()).unwrap_err().code, "INVALID_PARAMS");
        let control = ExportControl::new();
        control.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(eps_document(&doc, &ExportArea::WholeDocument, "x", &control).unwrap_err().code, "CANCELLED");
    }

    #[test]
    fn ps_string_escapes_specials() {
        assert_eq!(ps_string("a(b)c\\d"), ("(a\\(b\\)c\\\\d)".to_string(), false));
        assert_eq!(ps_string("\u{e9}"), ("(\\351)".to_string(), false));
        assert_eq!(ps_string("\u{d55c}"), ("(?)".to_string(), true));
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(2.0), "2");
    }

    #[test]
    fn export_writes_file_atomically_and_leaves_source_unchanged() {
        let doc = fixture();
        let before = serde_json::to_value(&doc).unwrap();
        let dir = std::env::temp_dir().join(format!("dtool_c13_eps_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.eps");
        let req = ExportRequest { format: crate::ExportFormat::Eps, area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) };
        let value = export_eps_value(&doc, &req, &ExportControl::new()).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.starts_with(EPS_HEADER));
        assert_eq!(value["bytes"], written.len() as u64);
        assert!(!path.with_extension("eps.tmp").exists());
        assert_eq!(serde_json::to_value(&doc).unwrap(), before);
    }
}
