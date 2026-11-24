use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use dtool_core::Document;
use dtool_protocol::CommandError;

use crate::{DocumentExportExt, ExportControl, ExportReceipt, ExportRequest, ObjectExportExt};

#[derive(Debug, Clone)]
pub struct WmfOptions {
    pub text_as_curves: bool,
    pub clip_transparency: bool,
}

impl Default for WmfOptions {
    fn default() -> Self {
        Self {
            text_as_curves: false,
            clip_transparency: true,
        }
    }
}

const WMF_HEADER_SIZE: usize = 34;
const PLACEABLE_KEY: u32 = 0x9AC6CDD7;

fn write_u16(w: &mut impl Write, v: u16) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn write_i16(w: &mut impl Write, v: i16) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn write_u32(w: &mut impl Write, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn write_i32(w: &mut impl Write, v: i32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn write_color_ref(w: &mut impl Write, r: u8, g: u8, b: u8) -> std::io::Result<()> {
    write_u32(w, (r as u32) | ((g as u32) << 8) | ((b as u32) << 16))?;
    Ok(())
}

fn wmf_header(w: &mut impl Write, width_mm: i32, height_mm: i32) -> std::io::Result<()> {
    write_u32(w, PLACEABLE_KEY)?;
    write_i32(w, 0)?;
    write_i32(w, 0)?;
    write_i32(w, width_mm)?;
    write_i32(w, height_mm)?;
    write_u16(w, WMF_HEADER_SIZE as u16)?;
    write_u16(w, 0x0300)?;
    write_u32(w, 0)?;
    write_u16(w, 0)?;
    write_u16(w, 0)?;
    Ok(())
}

fn wmf_metafile_header(w: &mut impl Write) -> std::io::Result<()> {
    write_u32(w, 0x00000009)?;
    write_u16(w, 0x0400)?;
    write_u16(w, 0x0000)?;
    write_u16(w, 0x0000)?;
    write_u32(w, 0x00000000)?;
    write_u16(w, 0x0000)?;
    write_u16(w, 0x0000)?;
    Ok(())
}

fn wmf_escape(w: &mut impl Write, function: u32, data: &[u8]) -> std::io::Result<()> {
    write_u32(w, 0x002B)?;
    write_u16(w, data.len() as u16)?;
    write_u32(w, function)?;
    w.write_all(data)?;
    if data.len() % 2 != 0 {
        w.write_all(&[0])?;
    }
    Ok(())
}

fn wmf_set_bk_color(w: &mut impl Write, r: u8, g: u8, b: u8) -> std::io::Result<()> {
    write_u32(w, 0x0102)?;
    write_color_ref(w, r, g, b)?;
    Ok(())
}

fn wmf_set_text_color(w: &mut impl Write, r: u8, g: u8, b: u8) -> std::io::Result<()> {
    write_u32(w, 0x0109)?;
    write_color_ref(w, r, g, b)?;
    Ok(())
}

fn wmf_create_pen_indirect(w: &mut impl Write, style: u16, width: i16, r: u8, g: u8, b: u8) -> std::io::Result<()> {
    write_u32(w, 0x02F9)?;
    write_u16(w, 0)?;
    write_u16(w, style)?;
    write_i16(w, width)?;
    write_color_ref(w, r, g, b)?;
    Ok(())
}

fn wmf_create_brush_indirect(w: &mut impl Write, style: u16, color_index: u16, r: u8, g: u8, b: u8) -> std::io::Result<()> {
    write_u32(w, 0x02FB)?;
    write_u16(w, 0)?;
    write_u16(w, style)?;
    write_color_ref(w, r, g, b)?;
    write_u16(w, color_index)?;
    Ok(())
}

fn wmf_select_object(w: &mut impl Write, handle: u16) -> std::io::Result<()> {
    write_u32(w, 0x0143)?;
    write_u16(w, handle)?;
    Ok(())
}

fn wmf_polygon(w: &mut impl Write, points: &[(i32, i32)]) -> std::io::Result<()> {
    write_u32(w, 0x0409)?;
    write_u16(w, points.len() as u16)?;
    for (x, y) in points {
        write_i16(w, *x as i16)?;
        write_i16(w, *y as i16)?;
    }
    Ok(())
}

fn wmf_polyline(w: &mut impl Write, points: &[(i32, i32)]) -> std::io::Result<()> {
    write_u32(w, 0x0415)?;
    write_u16(w, points.len() as u16)?;
    for (x, y) in points {
        write_i16(w, *x as i16)?;
        write_i16(w, *y as i16)?;
    }
    Ok(())
}

fn wmf_text_out(w: &mut impl Write, x: i32, y: i32, text: &str) -> std::io::Result<()> {
    let bytes = text.as_bytes();
    write_u32(w, 0x062A)?;
    write_i16(w, x as i16)?;
    write_i16(w, y as i16)?;
    write_u16(w, bytes.len() as u16)?;
    w.write_all(bytes)?;
    if bytes.len() % 2 != 0 {
        w.write_all(&[0])?;
    }
    Ok(())
}

fn wmf_end_of_file(w: &mut impl Write) -> std::io::Result<()> {
    write_u32(w, 0x0000)?;
    Ok(())
}

fn wmf_error(e: std::io::Error) -> CommandError {
    crate::export_io_error(e)
}

fn story_for<'a>(doc: &'a Document, object: &dtool_core::Object) -> Option<&'a dtool_core::Story> {
    doc.story(object)
}

fn object_colours(object: &dtool_core::Object) -> ((u8, u8, u8), (u8, u8, u8)) {
    let fill_color = match crate::paint_colour(&object.fill) {
        Some(p) => (p.r, p.g, p.b),
        None => (0u8, 0u8, 0u8),
    };

    let stroke_color = match crate::paint_colour(&object.stroke.paint) {
        Some(s) => (s.r, s.g, s.b),
        None => fill_color,
    };

    (fill_color, stroke_color)
}

fn create_wmf_file(path: &Path) -> Result<BufWriter<fs::File>, CommandError> {
    let file = fs::File::create(path).map_err(wmf_error)?;
    Ok(BufWriter::new(file))
}

pub fn export_wmf(
    doc: &Document,
    req: &ExportRequest,
    opts: &WmfOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    if control.is_cancelled() {
        return Err(crate::export_cancelled());
    }

    let path: PathBuf = req.path.clone();
    let mut warnings = Vec::new();

    let width_mm = doc.width() as i32;
    let height_mm = doc.height() as i32;

    let mut writer = create_wmf_file(&path)?;

    wmf_header(&mut writer, width_mm, height_mm)
        .map_err(wmf_error)?;
    wmf_metafile_header(&mut writer)
        .map_err(wmf_error)?;
    wmf_set_bk_color(&mut writer, 255, 255, 255)
        .map_err(wmf_error)?;

    let mut handle_counter = 1u16;

    for object in doc.objects() {
        if !crate::export_visible(doc, object) {
            continue;
        }

        if control.is_cancelled() {
            return Err(crate::export_cancelled());
        }

        let (fill_color, stroke_color) = object_colours(object);
        let has_fill = crate::paint_colour(&object.fill).is_some();

        if opts.clip_transparency {
            let alpha = object.opacity;
            if alpha < 0.5 {
                warnings.push(format!("object {} skipped due to low opacity", object.id.0));
                continue;
            }
        }

        match &object.kind {
            dtool_core::ObjectKind::Shape => {
                let points: Vec<(i32, i32)> = object.outline_points().iter().map(|p| (p.0 as i32, p.1 as i32)).collect();
                
                if !points.is_empty() {
                    wmf_create_pen_indirect(&mut writer, 0x0000, object.stroke.width.max(1.0) as i16, stroke_color.0, stroke_color.1, stroke_color.2)
                        .map_err(wmf_error)?;
                    let pen_handle = handle_counter;
                    handle_counter += 1;
                    wmf_select_object(&mut writer, pen_handle)
                        .map_err(wmf_error)?;

                    if has_fill {
                        wmf_create_brush_indirect(&mut writer, 0x0000, 0, fill_color.0, fill_color.1, fill_color.2)
                            .map_err(wmf_error)?;
                        let brush_handle = handle_counter;
                        handle_counter += 1;
                        wmf_select_object(&mut writer, brush_handle)
                            .map_err(wmf_error)?;
                        
                        wmf_polygon(&mut writer, &points)
                            .map_err(wmf_error)?;
                    } else {
                        let mut closed = points.clone();
                        closed.push(points[0]);
                        wmf_polyline(&mut writer, &closed)
                            .map_err(wmf_error)?;
                    }
                }
            }
            dtool_core::ObjectKind::Text(_)
            | dtool_core::ObjectKind::ArtText { .. }
            | dtool_core::ObjectKind::FrameText { .. }
            | dtool_core::ObjectKind::PathText(_) => {
                if opts.text_as_curves {
                    warnings.push(format!("text object {} rendered as curves placeholder", object.id.0));
                } else {
                    let text_content = match object.text_content() {
                        Some(content) => content.to_string(),
                        None => story_for(doc, object).map(|story| story.text.clone()).unwrap_or_default(),
                    };
                    wmf_set_text_color(&mut writer, fill_color.0, fill_color.1, fill_color.2)
                        .map_err(wmf_error)?;
                    let line_height = object.font_size().unwrap_or(12.0) * 1.2;
                    for (index, line) in text_content.lines().enumerate() {
                        let y = object.y() + line_height * (index as f32 + 1.0);
                        wmf_text_out(&mut writer, object.x() as i32, y as i32, line)
                            .map_err(wmf_error)?;
                    }
                }
            }
            _ => {
                warnings.push(format!("object {} kind not supported in WMF export", object.id.0));
            }
        }
    }

    wmf_end_of_file(&mut writer)
        .map_err(wmf_error)?;

    writer.flush().map_err(wmf_error)?;
    drop(writer);

    control.set_progress(1.0);

    let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    Ok(ExportReceipt {
        path,
        bytes,
        warnings,
    })
}
