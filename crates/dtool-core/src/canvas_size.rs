use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::command::Session;
use crate::document::{Document, Units};
use crate::object::ObjectKind;
use crate::pixel::PixelLayer;
use crate::pixel_art::{DOC_RESIZE_PIXEL_ART, DOC_RESIZE_PIXEL_ART_RESTORE};
use crate::selection::PixelSelection;
use crate::undo::Change;

pub const DOC_RESIZE: &str = "doc.resize";
pub const DOC_RESIZE_RESTORE: &str = "doc.resize.restore";
pub const DOC_RESIZE_CANVAS: &str = "doc.resize_canvas";
pub const DOC_RESIZE_CANVAS_RESTORE: &str = "doc.resize_canvas.restore";
pub const DOC_TURN_CANVAS: &str = "doc.turn_canvas";
pub const DOC_TURN_CANVAS_RESTORE: &str = "doc.turn_canvas.restore";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResampleMethod {
    Nearest,
    #[default]
    Bilinear,
    Bicubic,
    Lanczos3Separable,
    Lanczos3NonSeparable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasTurn {
    RotateLeft,
    RotateRight,
    FlipH,
    FlipV,
}

pub fn unit_to_px(value: f32, units: Units, dpi: f32) -> f32 {
    if !value.is_finite() || !dpi.is_finite() || dpi <= 0.0 {
        return f32::NAN;
    }
    crate::units::to_px(value, units, dpi).unwrap_or(f32::NAN)
}

pub fn px_to_unit(px: f32, units: Units, dpi: f32) -> f32 {
    if !px.is_finite() || !dpi.is_finite() || dpi <= 0.0 {
        return f32::NAN;
    }
    crate::units::from_px(px, units, dpi).unwrap_or(f32::NAN)
}

pub fn document_pixel_size(doc: &Document) -> Result<(u32, u32), CommandError> {
    let page = doc.pages.first().ok_or_else(CommandError::invalid_params)?;
    let w = unit_to_px(page.size[0], doc.units, doc.dpi);
    let h = unit_to_px(page.size[1], doc.units, doc.dpi);
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    Ok((w.round().max(1.0) as u32, h.round().max(1.0) as u32))
}

fn clamp_u32(v: f64) -> u32 {
    if !v.is_finite() {
        return 1;
    }
    v.round().clamp(1.0, u32::MAX as f64) as u32
}

pub fn resize_document(
    doc: &mut Document,
    w: u32,
    h: u32,
    dpi: f32,
    m: ResampleMethod,
) -> Result<(), CommandError> {
    if w == 0 || h == 0 || !dpi.is_finite() || dpi <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    let (old_w, old_h) = document_pixel_size(doc)?;
    let old_page = doc
        .pages
        .first()
        .map(|page| page.size)
        .ok_or_else(CommandError::invalid_params)?;
    if old_page.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(CommandError::invalid_params());
    }
    let new_page = [
        px_to_unit(w as f32, doc.units, dpi),
        px_to_unit(h as f32, doc.units, dpi),
    ];
    if new_page.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(CommandError::invalid_params());
    }
    let sx = new_page[0] / old_page[0];
    let sy = new_page[1] / old_page[1];
    if !sx.is_finite() || !sy.is_finite() || sx <= 0.0 || sy <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    scale_document(doc, sx, sy, new_page, dpi)?;
    resample_pixel_layers(doc, old_w, old_h, w, h, m)?;
    let pixel_sx = w as f32 / old_w as f32;
    let pixel_sy = h as f32 / old_h as f32;
    resample_object_rasters(doc, pixel_sx, pixel_sy, m)?;
    for mask in &mut doc.layer_masks {
        let mw = clamp_u32(mask.coverage.width as f64 * w as f64 / old_w as f64);
        let mh = clamp_u32(mask.coverage.height as f64 * h as f64 / old_h as f64);
        mask.coverage = resample_selection(&mask.coverage, mw, mh, m)?;
    }
    if let Some(selection) = doc.pixel_selection.take() {
        doc.pixel_selection = Some(resample_selection(&selection, w, h, m)?);
    }
    Ok(())
}

pub(crate) fn scale_document(
    doc: &mut Document,
    sx: f32,
    sy: f32,
    new_page: [f32; 2],
    dpi: f32,
) -> Result<(), CommandError> {
    if doc.pages.is_empty() {
        doc.push_page(new_page);
    } else {
        for page in &mut doc.pages {
            page.size = new_page;
        }
    }
    doc.dpi = dpi;
    for object in &mut doc.objects {
        object.pos[0] *= sx;
        object.pos[1] *= sy;
        object.size[0] *= sx;
        object.size[1] *= sy;
        if !object.pos[0].is_finite()
            || !object.pos[1].is_finite()
            || !object.size[0].is_finite()
            || !object.size[1].is_finite()
        {
            return Err(CommandError::invalid_params());
        }
        if let ObjectKind::Curve(curve) = &mut object.kind {
            scale_curve(curve, sx as f64, sy as f64);
        }
    }
    for slice in &mut doc.slices {
        slice.bounds[0] *= sx;
        slice.bounds[1] *= sy;
        slice.bounds[2] *= sx;
        slice.bounds[3] *= sy;
    }
    doc.setup.margins.left *= sx;
    doc.setup.margins.right *= sx;
    doc.setup.margins.top *= sy;
    doc.setup.margins.bottom *= sy;
    doc.setup.bleed.left *= sx;
    doc.setup.bleed.right *= sx;
    doc.setup.bleed.top *= sy;
    doc.setup.bleed.bottom *= sy;
    Ok(())
}

fn scale_contour(contour: &mut crate::curve::Contour, sx: f64, sy: f64) {
    for point in &mut contour.points {
        point.pos[0] *= sx;
        point.pos[1] *= sy;
        if let Some(handle) = point.handle_in.as_mut() {
            handle[0] *= sx;
            handle[1] *= sy;
        }
        if let Some(handle) = point.handle_out.as_mut() {
            handle[0] *= sx;
            handle[1] *= sy;
        }
    }
}

pub fn scale_curve(curve: &mut crate::curve::Curve, sx: f64, sy: f64) {
    scale_contour(&mut curve.contour, sx, sy);
    for hole in &mut curve.holes {
        scale_contour(hole, sx, sy);
    }
}

fn scale_guides(session: &mut Session, sx: f32, sy: f32) {
    scale_guide_positions(&mut session.guides, sx, sy);
}

fn scale_guide_positions(guides: &mut crate::guides::Guides, sx: f32, sy: f32) {
    for guide in &mut guides.items {
        match guide.kind {
            crate::guides::GuideKind::Vertical => guide.position *= sx,
            crate::guides::GuideKind::Horizontal => guide.position *= sy,
            crate::guides::GuideKind::Column => guide.position *= sx,
        }
    }
}

fn resample_pixel_layers(
    doc: &mut Document,
    old_w: u32,
    old_h: u32,
    new_w: u32,
    new_h: u32,
    method: ResampleMethod,
) -> Result<(), CommandError> {
    if old_w == 0 || old_h == 0 {
        return Err(CommandError::invalid_params());
    }
    let ids = doc.pixel_layers.ids();
    for id in ids {
        let layer = match doc.pixel_layers.get(id) {
            Some(layer) => layer.clone(),
            None => continue,
        };
        let lw = clamp_u32(layer.width as f64 * new_w as f64 / old_w as f64);
        let lh = clamp_u32(layer.height as f64 * new_h as f64 / old_h as f64);
        let resized = resample_layer(&layer, lw, lh, method)?;
        doc.pixel_layers.insert(id, resized);
    }
    Ok(())
}

fn resample_object_rasters(
    doc: &mut Document,
    sx: f32,
    sy: f32,
    method: ResampleMethod,
) -> Result<(), CommandError> {
    for object in &mut doc.objects {
        let (data, size) = match (object.raster_data.as_ref(), object.raster_size) {
            (Some(data), Some(size)) => (data, size),
            _ => continue,
        };
        if size.0 == 0 || size.1 == 0 {
            continue;
        }
        let expected = size.0 as usize * size.1 as usize * 4;
        if data.len() < expected {
            continue;
        }
        let src = PixelLayer::from_rgba(size.0, size.1, &data[..expected]);
        let nw = clamp_u32(size.0 as f64 * sx as f64);
        let nh = clamp_u32(size.1 as f64 * sy as f64);
        let resized = resample_layer(&src, nw, nh, method)?;
        object.raster_data = Some(resized.to_rgba());
        object.raster_size = Some((nw, nh));
    }
    Ok(())
}

fn resample_selection(
    selection: &PixelSelection,
    new_w: u32,
    new_h: u32,
    method: ResampleMethod,
) -> Result<PixelSelection, CommandError> {
    if selection.width == 0 || selection.height == 0 || new_w == 0 || new_h == 0 {
        return Err(CommandError::invalid_params());
    }
    let mut rgba = vec![0u8; selection.width as usize * selection.height as usize * 4];
    let n = (selection.width as usize * selection.height as usize).min(selection.coverage.len());
    for i in 0..n {
        let v = selection.coverage[i];
        let o = i * 4;
        rgba[o] = v;
        rgba[o + 1] = v;
        rgba[o + 2] = v;
        rgba[o + 3] = 255;
    }
    let src = PixelLayer::from_rgba(selection.width, selection.height, &rgba);
    let dst = resample_layer(&src, new_w, new_h, method)?;
    let out = dst.to_rgba();
    let mut coverage = vec![0u8; new_w as usize * new_h as usize];
    for (i, slot) in coverage.iter_mut().enumerate() {
        let o = i * 4;
        if o + 2 < out.len() {
            *slot = out[o];
        }
    }
    Ok(PixelSelection::from_coverage(new_w, new_h, coverage))
}

fn resample_layer(
    src: &PixelLayer,
    new_w: u32,
    new_h: u32,
    method: ResampleMethod,
) -> Result<PixelLayer, CommandError> {
    if new_w == 0 || new_h == 0 || src.width == 0 || src.height == 0 {
        return Err(CommandError::invalid_params());
    }
    if new_w == src.width && new_h == src.height {
        return Ok(src.clone());
    }
    let rgba = match method {
        ResampleMethod::Nearest => resample_nearest(src, new_w, new_h),
        ResampleMethod::Bilinear => resample_separable(src, new_w, new_h, 1, kernel_bilinear),
        ResampleMethod::Bicubic => resample_separable(src, new_w, new_h, 2, kernel_bicubic),
        ResampleMethod::Lanczos3Separable => resample_separable(src, new_w, new_h, 3, kernel_lanczos3),
        ResampleMethod::Lanczos3NonSeparable => resample_lanczos_nonsep(src, new_w, new_h),
    };
    Ok(PixelLayer::from_rgba(new_w, new_h, &rgba))
}

fn map_src(dest: u32, src_len: u32, dst_len: u32) -> f64 {
    (dest as f64 + 0.5) * (src_len as f64 / dst_len as f64) - 0.5
}

fn sample_clamp(layer: &PixelLayer, x: i64, y: i64) -> [u8; 4] {
    let max_x = (layer.width as i64 - 1).max(0);
    let max_y = (layer.height as i64 - 1).max(0);
    let x = x.clamp(0, max_x) as u32;
    let y = y.clamp(0, max_y) as u32;
    layer.get(x, y)
}

fn premultiply(c: [u8; 4]) -> [f64; 4] {
    let a = c[3] as f64 / 255.0;
    [
        c[0] as f64 / 255.0 * a,
        c[1] as f64 / 255.0 * a,
        c[2] as f64 / 255.0 * a,
        a,
    ]
}

fn unpremultiply(c: [f64; 4]) -> [u8; 4] {
    if c[3] <= 0.0 {
        return [0, 0, 0, 0];
    }
    let r = (c[0] / c[3] * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (c[1] / c[3] * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (c[2] / c[3] * 255.0).round().clamp(0.0, 255.0) as u8;
    let a = (c[3] * 255.0).round().clamp(0.0, 255.0) as u8;
    [r, g, b, a]
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        let p = std::f64::consts::PI * x;
        p.sin() / p
    }
}

fn kernel_bilinear(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        1.0 - x
    } else {
        0.0
    }
}

fn kernel_bicubic(x: f64) -> f64 {
    let a = -0.5;
    let x = x.abs();
    if x <= 1.0 {
        (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
    } else if x < 2.0 {
        a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
    } else {
        0.0
    }
}

fn kernel_lanczos3(x: f64) -> f64 {
    if x.abs() >= 3.0 {
        0.0
    } else {
        sinc(x) * sinc(x / 3.0)
    }
}

fn resample_nearest(src: &PixelLayer, new_w: u32, new_h: u32) -> Vec<u8> {
    let mut out = vec![0u8; new_w as usize * new_h as usize * 4];
    for y in 0..new_h {
        for x in 0..new_w {
            let sx = map_src(x, src.width, new_w).round() as i64;
            let sy = map_src(y, src.height, new_h).round() as i64;
            let px = sample_clamp(src, sx, sy);
            let o = (y as usize * new_w as usize + x as usize) * 4;
            out[o] = px[0];
            out[o + 1] = px[1];
            out[o + 2] = px[2];
            out[o + 3] = px[3];
        }
    }
    out
}

fn filter_1d(samples: &[[f64; 4]], center: f64, radius: i32, kernel: fn(f64) -> f64) -> [f64; 4] {
    let mut acc = [0.0; 4];
    let mut wsum = 0.0;
    let left = (center.floor() as i64 - radius as i64 + 1).max(0);
    let right = (center.floor() as i64 + radius as i64).min(samples.len() as i64 - 1);
    let mut i = left;
    while i <= right {
        let w = kernel(center - i as f64);
        if w != 0.0 {
            let s = samples[i as usize];
            acc[0] += s[0] * w;
            acc[1] += s[1] * w;
            acc[2] += s[2] * w;
            acc[3] += s[3] * w;
            wsum += w;
        }
        i += 1;
    }
    if wsum.abs() < 1e-12 {
        [0.0, 0.0, 0.0, 0.0]
    } else {
        [acc[0] / wsum, acc[1] / wsum, acc[2] / wsum, acc[3] / wsum]
    }
}

fn resample_separable(
    src: &PixelLayer,
    new_w: u32,
    new_h: u32,
    radius: i32,
    kernel: fn(f64) -> f64,
) -> Vec<u8> {
    let src_w = src.width as usize;
    let src_h = src.height as usize;
    let mut tmp = vec![[0.0; 4]; new_w as usize * src_h];
    for y in 0..src.height {
        let mut row = vec![[0.0; 4]; src_w];
        for x in 0..src.width {
            row[x as usize] = premultiply(src.get(x, y));
        }
        for x in 0..new_w {
            let cx = map_src(x, src.width, new_w);
            tmp[y as usize * new_w as usize + x as usize] = filter_1d(&row, cx, radius, kernel);
        }
    }
    let mut out = vec![0u8; new_w as usize * new_h as usize * 4];
    for x in 0..new_w {
        let mut col = vec![[0.0; 4]; src_h];
        for y in 0..src.height {
            col[y as usize] = tmp[y as usize * new_w as usize + x as usize];
        }
        for y in 0..new_h {
            let cy = map_src(y, src.height, new_h);
            let px = unpremultiply(filter_1d(&col, cy, radius, kernel));
            let o = (y as usize * new_w as usize + x as usize) * 4;
            out[o] = px[0];
            out[o + 1] = px[1];
            out[o + 2] = px[2];
            out[o + 3] = px[3];
        }
    }
    out
}

fn resample_lanczos_nonsep(src: &PixelLayer, new_w: u32, new_h: u32) -> Vec<u8> {
    let mut out = vec![0u8; new_w as usize * new_h as usize * 4];
    let radius = 3i64;
    for y in 0..new_h {
        for x in 0..new_w {
            let cx = map_src(x, src.width, new_w);
            let cy = map_src(y, src.height, new_h);
            let mut acc = [0.0; 4];
            let mut wsum = 0.0;
            let x0 = (cx.floor() as i64 - radius + 1).max(0);
            let x1 = (cx.floor() as i64 + radius).min(src.width as i64 - 1);
            let y0 = (cy.floor() as i64 - radius + 1).max(0);
            let y1 = (cy.floor() as i64 + radius).min(src.height as i64 - 1);
            let mut yy = y0;
            while yy <= y1 {
                let wy = kernel_lanczos3(cy - yy as f64);
                let mut xx = x0;
                while xx <= x1 {
                    let w = wy * kernel_lanczos3(cx - xx as f64);
                    if w != 0.0 {
                        let s = premultiply(sample_clamp(src, xx, yy));
                        acc[0] += s[0] * w;
                        acc[1] += s[1] * w;
                        acc[2] += s[2] * w;
                        acc[3] += s[3] * w;
                        wsum += w;
                    }
                    xx += 1;
                }
                yy += 1;
            }
            let px = if wsum.abs() < 1e-12 {
                [0, 0, 0, 0]
            } else {
                unpremultiply([acc[0] / wsum, acc[1] / wsum, acc[2] / wsum, acc[3] / wsum])
            };
            let o = (y as usize * new_w as usize + x as usize) * 4;
            out[o] = px[0];
            out[o + 1] = px[1];
            out[o + 2] = px[2];
            out[o + 3] = px[3];
        }
    }
    out
}

fn snapshot(doc: &Document, guides: &crate::guides::Guides) -> Result<serde_json::Value, CommandError> {
    serde_json::to_value(json!({
        "pages": doc.pages,
        "dpi": doc.dpi,
        "units": doc.units,
        "objects": doc.objects,
        "pixel_layers": doc.pixel_layers,
        "layer_masks": doc.layer_masks,
        "pixel_selection": doc.pixel_selection,
        "slices": doc.slices,
        "margins": doc.setup.margins,
        "bleed": doc.setup.bleed,
        "setup": doc.setup,
        "guides": guides,
    }))
    .map_err(|_| CommandError::invalid_params())
}

fn restore_snapshot(
    doc: &mut Document,
    guides: &mut crate::guides::Guides,
    value: serde_json::Value,
) -> Result<(), CommandError> {
    let pages = value
        .get("pages")
        .cloned()
        .ok_or_else(CommandError::invalid_params)?;
    doc.pages = serde_json::from_value(pages).map_err(|_| CommandError::invalid_params())?;
    let dpi = value
        .get("dpi")
        .and_then(|v| v.as_f64())
        .ok_or_else(CommandError::invalid_params)?;
    if !dpi.is_finite() || dpi <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    doc.dpi = dpi as f32;
    if let Some(units) = value.get("units").cloned() {
        doc.units = serde_json::from_value(units).map_err(|_| CommandError::invalid_params())?;
    }
    let objects = value
        .get("objects")
        .cloned()
        .ok_or_else(CommandError::invalid_params)?;
    doc.objects = serde_json::from_value(objects).map_err(|_| CommandError::invalid_params())?;
    let layers = value
        .get("pixel_layers")
        .cloned()
        .ok_or_else(CommandError::invalid_params)?;
    doc.pixel_layers = serde_json::from_value(layers).map_err(|_| CommandError::invalid_params())?;
    if let Some(masks) = value.get("layer_masks") {
        doc.layer_masks = serde_json::from_value(masks.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    let selection = value.get("pixel_selection").cloned().unwrap_or(serde_json::Value::Null);
    doc.pixel_selection = serde_json::from_value(selection).map_err(|_| CommandError::invalid_params())?;
    if let Some(slices) = value.get("slices").cloned() {
        doc.slices = serde_json::from_value(slices).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(margins) = value.get("margins").cloned() {
        doc.setup.margins = serde_json::from_value(margins).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(bleed) = value.get("bleed").cloned() {
        doc.setup.bleed = serde_json::from_value(bleed).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(setup) = value.get("setup").cloned() {
        doc.setup = serde_json::from_value(setup).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(g) = value.get("guides").cloned() {
        guides.replace_with(serde_json::from_value(g).map_err(|_| CommandError::invalid_params())?);
    }
    Ok(())
}

pub fn parse_resize_params(params: &serde_json::Value) -> Result<(u32, u32, f32, ResampleMethod), CommandError> {
    let w = params
        .get("w")
        .or_else(|| params.get("width"))
        .and_then(|v| v.as_u64())
        .ok_or_else(CommandError::invalid_params)?;
    let h = params
        .get("h")
        .or_else(|| params.get("height"))
        .and_then(|v| v.as_u64())
        .ok_or_else(CommandError::invalid_params)?;
    let dpi = params
        .get("dpi")
        .and_then(|v| v.as_f64())
        .ok_or_else(CommandError::invalid_params)?;
    if w == 0 || w > u32::MAX as u64 || h == 0 || h > u32::MAX as u64 || !dpi.is_finite() || dpi <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    let method = if let Some(value) = params.get("method").or_else(|| params.get("m")) {
        serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?
    } else {
        ResampleMethod::Bilinear
    };
    Ok((w as u32, h as u32, dpi as f32, method))
}

pub fn apply_resize(
    session: &mut Session,
    w: u32,
    h: u32,
    dpi: f32,
    m: ResampleMethod,
) -> Result<CommandResult, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let (old_w, old_h) = document_pixel_size(&session.document)?;
    let old_dpi = session.document.dpi;
    if old_w == w && old_h == h && (old_dpi - dpi).abs() < f32::EPSILON {
        return Ok(Ok(json!({ "w": w, "h": h, "dpi": dpi, "changed": false })));
    }
    let before = snapshot(&session.document, &session.guides)?;
    let old_page = session
        .document
        .pages
        .first()
        .map(|page| page.size)
        .ok_or_else(CommandError::invalid_params)?;
    resize_document(&mut session.document, w, h, dpi, m)?;
    let new_page = session
        .document
        .pages
        .first()
        .map(|page| page.size)
        .unwrap_or(old_page);
    if old_page[0] > 0.0 && old_page[1] > 0.0 {
        scale_guides(
            session,
            new_page[0] / old_page[0],
            new_page[1] / old_page[1],
        );
    }
    let after = snapshot(&session.document, &session.guides)?;
    let redo = Command {
        name: DOC_RESIZE_RESTORE.to_string(),
        params: json!({ "snapshot": after }),
    };
    let undo = Command {
        name: DOC_RESIZE_RESTORE.to_string(),
        params: json!({ "snapshot": before }),
    };
    session.undo.push(Change {
        name: DOC_RESIZE.to_string(),
        redo,
        undo,
    });
    session.generation += 1;
    session.document.version += 1;
    Ok(Ok(json!({ "w": w, "h": h, "dpi": dpi, "changed": true })))
}

fn apply_pixel_art_resize(session: &mut Session, cmd: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let (factor, filter) = crate::pixel_art::options(&cmd.params)?;
    let before = snapshot(&session.document, &session.guides)?;
    let mut document = session.document.clone();
    let [width, height] = crate::pixel_art::resize(&mut document, factor, filter)?;
    let mut guides = session.guides.clone();
    scale_guide_positions(&mut guides, factor as f32, factor as f32);
    let after = snapshot(&document, &guides)?;
    let restore = |snapshot: serde_json::Value| Command::with_params(DOC_RESIZE_PIXEL_ART_RESTORE, json!({"snapshot": snapshot}));
    document.version += 1;
    session.document = document;
    session.guides = guides;
    session.undo.push(Change {
        name: DOC_RESIZE_PIXEL_ART.to_string(),
        redo: restore(after),
        undo: restore(before),
    });
    session.generation += 1;
    Ok(json!({
        "w": width, "h": height, "scale": factor,
        "filter": cmd.params["filter"], "changes": [{"name": DOC_RESIZE_PIXEL_ART}],
    }))
}

fn restore_with_view(session: &mut Session, cmd: &Command) -> Result<(), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let value = cmd.params.get("snapshot").cloned().ok_or_else(CommandError::invalid_params)?;
    let offset = cmd.params.get("content_offset").map(|value| {
        let offset: [f32; 2] = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        if offset.iter().all(|coord| coord.is_finite()) { Ok(offset) } else { Err(CommandError::invalid_params()) }
    }).transpose()?;
    restore_snapshot(&mut session.document, &mut session.guides, value)?;
    if let Some(offset) = offset {
        offset_canvas_view(&mut session.view, offset);
    }
    Ok(())
}

fn offset_canvas_view(view: &mut crate::ViewState, offset: [f32; 2]) {
    let origin = crate::view::doc_to_view(view, [0.0; 2]);
    let moved = crate::view::doc_to_view(view, offset);
    for axis in 0..2 {
        view.scroll[axis] += origin[axis] - moved[axis];
    }
}

fn apply_restore(session: &mut Session, cmd: &Command) -> Result<CommandResult, CommandError> {
    restore_with_view(session, cmd)?;
    session.generation += 1;
    session.document.version += 1;
    Ok(Ok(json!({})))
}

pub fn resize_canvas(doc: &mut Document, w: u32, h: u32, anchor: u8) -> Result<(), CommandError> {
    resize_canvas_with_offset(doc, w, h, anchor).map(|_| ())
}

fn resize_canvas_with_offset(doc: &mut Document, w: u32, h: u32, anchor: u8) -> Result<[f32; 2], CommandError> {
    if w == 0 || h == 0 {
        return Err(CommandError::invalid_params());
    }
    let (old_w, old_h) = document_pixel_size(doc)?;
    if old_w == 0 || old_h == 0 {
        return Err(CommandError::invalid_params());
    }
    let anchor = anchor.min(8);
    let col = anchor % 3;
    let row = anchor / 3;

    let dx_px = w as i64 - old_w as i64;
    let dy_px = h as i64 - old_h as i64;

    let offset_x_px = match col {
        0 => 0,
        1 => dx_px / 2,
        _ => dx_px,
    };
    let offset_y_px = match row {
        0 => 0,
        1 => dy_px / 2,
        _ => dy_px,
    };

    let dpi = doc.dpi;
    if !dpi.is_finite() || dpi <= 0.0 {
        return Err(CommandError::invalid_params());
    }

    let offset_x_unit = px_to_unit(offset_x_px as f32, doc.units, dpi);
    let offset_y_unit = px_to_unit(offset_y_px as f32, doc.units, dpi);

    if !offset_x_unit.is_finite() || !offset_y_unit.is_finite() {
        return Err(CommandError::invalid_params());
    }

    let offset = [offset_x_unit, offset_y_unit];
    let translate = |point: [f32; 2]| [point[0] + offset[0], point[1] + offset[1]];
    let new_page = [
        px_to_unit(w as f32, doc.units, dpi),
        px_to_unit(h as f32, doc.units, dpi),
    ];
    if new_page.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || doc.objects.iter().any(|object| {
            translate(object.pos).iter().any(|v| !v.is_finite())
        })
    {
        return Err(CommandError::invalid_params());
    }

    let selection = doc.pixel_selection.as_ref()
        .map(|selection| selection.for_resized_canvas(w, h, [offset_x_px, offset_y_px]))
        .transpose()?;
    doc.pixel_selection = selection;
    for object in &mut doc.objects {
        object.pos = translate(object.pos);
        // The transform origin is stored as a fraction of the object size.
    }

    for slice in &mut doc.slices {
        slice.bounds[0] += offset_x_unit;
        slice.bounds[1] += offset_y_unit;
        slice.bounds[2] += offset_x_unit;
        slice.bounds[3] += offset_y_unit;
    }

    for page in &mut doc.pages {
        page.size = new_page;
    }

    Ok(offset)
}

pub fn parse_resize_canvas_params(params: &serde_json::Value) -> Result<(u32, u32, u8), CommandError> {
    let w = params
        .get("w")
        .or_else(|| params.get("width"))
        .and_then(|v| v.as_u64())
        .ok_or_else(CommandError::invalid_params)?;
    let h = params
        .get("h")
        .or_else(|| params.get("height"))
        .and_then(|v| v.as_u64())
        .ok_or_else(CommandError::invalid_params)?;
    let anchor = params
        .get("anchor")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if w == 0 || w > u32::MAX as u64 || h == 0 || h > u32::MAX as u64 {
        return Err(CommandError::invalid_params());
    }
    if anchor > 8 {
        return Err(CommandError::invalid_params());
    }

    Ok((w as u32, h as u32, anchor as u8))
}

pub fn apply_resize_canvas(
    session: &mut Session,
    w: u32,
    h: u32,
    anchor: u8,
) -> Result<CommandResult, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let (old_w, old_h) = document_pixel_size(&session.document)?;
    if old_w == w && old_h == h {
        return Ok(Ok(json!({ "w": w, "h": h, "anchor": anchor, "changed": false })));
    }

    let before = snapshot(&session.document, &session.guides)?;
    
    let offset = resize_canvas_with_offset(&mut session.document, w, h, anchor)?;
    for guide in session.guides.items.iter_mut().filter(|guide| guide.surface().is_none()) {
        guide.position += offset[guide.kind.axis()];
    }

    let after = snapshot(&session.document, &session.guides)?;
    let redo = Command {
        name: DOC_RESIZE_CANVAS_RESTORE.to_string(),
        params: json!({ "snapshot": after, "content_offset": offset }),
    };
    let undo = Command {
        name: DOC_RESIZE_CANVAS_RESTORE.to_string(),
        params: json!({ "snapshot": before, "content_offset": offset.map(|value| -value) }),
    };

    offset_canvas_view(&mut session.view, offset);
    session.undo.push(Change {
        name: DOC_RESIZE_CANVAS.to_string(),
        redo,
        undo,
    });

    session.generation += 1;
    session.document.version += 1;

    Ok(Ok(json!({ "w": w, "h": h, "anchor": anchor, "changed": true })))
}

pub fn turn_canvas(doc: &mut Document, t: CanvasTurn) -> Result<(), CommandError> {
    let (old_w, old_h) = document_pixel_size(doc)?;
    if old_w == 0 || old_h == 0 {
        return Err(CommandError::invalid_params());
    }

    let masks = doc.layer_masks.iter().map(|mask| {
        let coverage = &mask.coverage;
        let expected = (coverage.width as usize).checked_mul(coverage.height as usize)
            .ok_or_else(CommandError::invalid_params)?;
        if expected == 0 || expected != coverage.coverage.len() { return Err(CommandError::invalid_params()); }
        let rgba: Vec<u8> = coverage.coverage.iter().flat_map(|v| [*v, *v, *v, 255]).collect();
        let layer = PixelLayer::from_rgba(coverage.width, coverage.height, &rgba);
        let layer = match t {
            CanvasTurn::RotateLeft => rotate_layer_left(&layer),
            CanvasTurn::RotateRight => rotate_layer_right(&layer),
            CanvasTurn::FlipH => flip_layer_horizontal(&layer),
            CanvasTurn::FlipV => flip_layer_vertical(&layer),
        };
        let coverage = layer.to_rgba().chunks_exact(4).map(|p| p[0]).collect();
        let transform = kurbo::Affine::new(match t {
            CanvasTurn::RotateLeft => [0.0, 1.0, -1.0, 0.0, 1.0, 0.0],
            CanvasTurn::RotateRight => [0.0, -1.0, 1.0, 0.0, 0.0, 1.0],
            CanvasTurn::FlipH => [-1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            CanvasTurn::FlipV => [1.0, 0.0, 0.0, -1.0, 0.0, 1.0],
        });
        Ok(crate::raster_source::LayerMask {
            object: mask.object,
            coverage: PixelSelection::from_coverage(layer.width, layer.height, coverage),
            vector_outline: mask.vector_outline.as_ref()
                .map(|outline| crate::curve::Curve::from_kurbo(&(transform * outline.to_kurbo()))),
        })
    }).collect::<Result<Vec<_>, CommandError>>()?;

    match t {
        CanvasTurn::RotateLeft => {
            rotate_canvas_left(doc, old_w, old_h)?;
        }
        CanvasTurn::RotateRight => {
            rotate_canvas_right(doc, old_w, old_h)?;
        }
        CanvasTurn::FlipH => {
            flip_canvas_horizontal(doc, old_w);
        }
        CanvasTurn::FlipV => {
            flip_canvas_vertical(doc, old_h);
        }
    }

    doc.layer_masks = masks;
    Ok(())
}

fn rotate_canvas_left(doc: &mut Document, old_w: u32, old_h: u32) -> Result<(), CommandError> {
    let dpi = doc.dpi;
    if !dpi.is_finite() || dpi <= 0.0 {
        return Err(CommandError::invalid_params());
    }

    for object in &mut doc.objects {
        let x = object.pos[0];
        let y = object.pos[1];
        let w = object.size[0];
        let h = object.size[1];

        object.pos[0] = old_h as f32 - (y + h);
        object.pos[1] = x;
        object.size[0] = h;
        object.size[1] = w;

        if !object.pos[0].is_finite() || !object.pos[1].is_finite() {
            return Err(CommandError::invalid_params());
        }

        if let ObjectKind::Curve(curve) = &mut object.kind {
            rotate_curve_left(curve, old_h as f64);
        }
    }

    for slice in &mut doc.slices {
        let x0 = slice.bounds[0];
        let y0 = slice.bounds[1];
        let x1 = slice.bounds[2];
        let y1 = slice.bounds[3];

        slice.bounds[0] = old_h as f32 - y1;
        slice.bounds[1] = x0;
        slice.bounds[2] = old_h as f32 - y0;
        slice.bounds[3] = x1;
    }

    doc.setup.margins.left *= 1.0; 
    let tmp_m_l = doc.setup.margins.left;
    let tmp_m_r = doc.setup.margins.right;
    let tmp_m_t = doc.setup.margins.top;
    let tmp_m_b = doc.setup.margins.bottom;

    doc.setup.margins.left = tmp_m_t;
    doc.setup.margins.top = tmp_m_r;
    doc.setup.margins.right = tmp_m_b;
    doc.setup.margins.bottom = tmp_m_l;

    let tmp_b_l = doc.setup.bleed.left;
    let tmp_b_r = doc.setup.bleed.right;
    let tmp_b_t = doc.setup.bleed.top;
    let tmp_b_b = doc.setup.bleed.bottom;

    doc.setup.bleed.left = tmp_b_t;
    doc.setup.bleed.top = tmp_b_r;
    doc.setup.bleed.right = tmp_b_b;
    doc.setup.bleed.bottom = tmp_b_l;

    for page in &mut doc.pages {
        let old_w_unit = page.size[0];
        let old_h_unit = page.size[1];
        page.size[0] = old_h_unit;
        page.size[1] = old_w_unit;
    }

    let ids = doc.pixel_layers.ids();
    for id in ids {
        if let Some(layer) = doc.pixel_layers.get(id) {
            let rotated = rotate_layer_left(layer);
            doc.pixel_layers.insert(id, rotated);
        }
    }

    for object in &mut doc.objects {
        if let (Some(data), Some(size)) = (object.raster_data.as_ref(), object.raster_size) {
            if size.0 > 0 && size.1 > 0 {
                let expected = size.0 as usize * size.1 as usize * 4;
                if data.len() >= expected {
                    let src = PixelLayer::from_rgba(size.0, size.1, &data[..expected]);
                    let rotated = rotate_layer_left(&src);
                    object.raster_data = Some(rotated.to_rgba());
                    object.raster_size = Some((rotated.width, rotated.height));
                }
            }
        }
    }

    if let Some(selection) = doc.pixel_selection.take() {
        if selection.width > 0 && selection.height > 0 {
            let mut rgba = vec![0u8; selection.width as usize * selection.height as usize * 4];
            let n = (selection.width as usize * selection.height as usize).min(selection.coverage.len());
            for i in 0..n {
                let v = selection.coverage[i];
                let o = i * 4;
                rgba[o] = v;
                rgba[o + 1] = v;
                rgba[o + 2] = v;
                rgba[o + 3] = 255;
            }
            let src = PixelLayer::from_rgba(selection.width, selection.height, &rgba);
            let rotated = rotate_layer_left(&src);
            let out = rotated.to_rgba();
            let mut coverage = vec![0u8; rotated.width as usize * rotated.height as usize];
            for (i, slot) in coverage.iter_mut().enumerate() {
                let o = i * 4;
                if o < out.len() {
                    *slot = out[o];
                }
            }
            doc.pixel_selection = Some(PixelSelection::from_coverage(rotated.width, rotated.height, coverage));
        }
    }


    Ok(())
}

fn rotate_canvas_right(doc: &mut Document, old_w: u32, old_h: u32) -> Result<(), CommandError> {
    let dpi = doc.dpi;
    if !dpi.is_finite() || dpi <= 0.0 {
        return Err(CommandError::invalid_params());
    }

    for object in &mut doc.objects {
        let x = object.pos[0];
        let y = object.pos[1];
        let w = object.size[0];
        let h = object.size[1];

        object.pos[0] = y;
        object.pos[1] = old_w as f32 - (x + w);
        object.size[0] = h;
        object.size[1] = w;

        if !object.pos[0].is_finite() || !object.pos[1].is_finite() {
            return Err(CommandError::invalid_params());
        }

        if let ObjectKind::Curve(curve) = &mut object.kind {
            rotate_curve_right(curve, old_w as f64);
        }
    }

    for slice in &mut doc.slices {
        let x0 = slice.bounds[0];
        let y0 = slice.bounds[1];
        let x1 = slice.bounds[2];
        let y1 = slice.bounds[3];

        slice.bounds[0] = y0;
        slice.bounds[1] = old_w as f32 - x1;
        slice.bounds[2] = y1;
        slice.bounds[3] = old_w as f32 - x0;
    }

    let tmp_m_l = doc.setup.margins.left;
    let tmp_m_r = doc.setup.margins.right;
    let tmp_m_t = doc.setup.margins.top;
    let tmp_m_b = doc.setup.margins.bottom;

    doc.setup.margins.left = tmp_m_b;
    doc.setup.margins.top = tmp_m_l;
    doc.setup.margins.right = tmp_m_t;
    doc.setup.margins.bottom = tmp_m_r;

    let tmp_b_l = doc.setup.bleed.left;
    let tmp_b_r = doc.setup.bleed.right;
    let tmp_b_t = doc.setup.bleed.top;
    let tmp_b_b = doc.setup.bleed.bottom;

    doc.setup.bleed.left = tmp_b_b;
    doc.setup.bleed.top = tmp_b_l;
    doc.setup.bleed.right = tmp_b_t;
    doc.setup.bleed.bottom = tmp_b_r;

    for page in &mut doc.pages {
        let old_w_unit = page.size[0];
        let old_h_unit = page.size[1];
        page.size[0] = old_h_unit;
        page.size[1] = old_w_unit;
    }

    let ids = doc.pixel_layers.ids();
    for id in ids {
        if let Some(layer) = doc.pixel_layers.get(id) {
            let rotated = rotate_layer_right(layer);
            doc.pixel_layers.insert(id, rotated);
        }
    }

    for object in &mut doc.objects {
        if let (Some(data), Some(size)) = (object.raster_data.as_ref(), object.raster_size) {
            if size.0 > 0 && size.1 > 0 {
                let expected = size.0 as usize * size.1 as usize * 4;
                if data.len() >= expected {
                    let src = PixelLayer::from_rgba(size.0, size.1, &data[..expected]);
                    let rotated = rotate_layer_right(&src);
                    object.raster_data = Some(rotated.to_rgba());
                    object.raster_size = Some((rotated.width, rotated.height));
                }
            }
        }
    }

    if let Some(selection) = doc.pixel_selection.take() {
        if selection.width > 0 && selection.height > 0 {
            let mut rgba = vec![0u8; selection.width as usize * selection.height as usize * 4];
            let n = (selection.width as usize * selection.height as usize).min(selection.coverage.len());
            for i in 0..n {
                let v = selection.coverage[i];
                let o = i * 4;
                rgba[o] = v;
                rgba[o + 1] = v;
                rgba[o + 2] = v;
                rgba[o + 3] = 255;
            }
            let src = PixelLayer::from_rgba(selection.width, selection.height, &rgba);
            let rotated = rotate_layer_right(&src);
            let out = rotated.to_rgba();
            let mut coverage = vec![0u8; rotated.width as usize * rotated.height as usize];
            for (i, slot) in coverage.iter_mut().enumerate() {
                let o = i * 4;
                if o < out.len() {
                    *slot = out[o];
                }
            }
            doc.pixel_selection = Some(PixelSelection::from_coverage(rotated.width, rotated.height, coverage));
        }
    }


    Ok(())
}

fn flip_canvas_horizontal(doc: &mut Document, old_w: u32) {
    for object in &mut doc.objects {
        let x = object.pos[0];
        let w = object.size[0];
        object.pos[0] = old_w as f32 - (x + w);

        if let ObjectKind::Curve(curve) = &mut object.kind {
            flip_curve_horizontal(curve, old_w as f64);
        }
    }

    for slice in &mut doc.slices {
        let x0 = slice.bounds[0];
        let x1 = slice.bounds[2];
        slice.bounds[0] = old_w as f32 - x1;
        slice.bounds[2] = old_w as f32 - x0;
    }

    let tmp_l = doc.setup.margins.left;
    doc.setup.margins.left = doc.setup.margins.right;
    doc.setup.margins.right = tmp_l;

    let tmp_bl = doc.setup.bleed.left;
    doc.setup.bleed.left = doc.setup.bleed.right;
    doc.setup.bleed.right = tmp_bl;

    let ids = doc.pixel_layers.ids();
    for id in ids {
        if let Some(layer) = doc.pixel_layers.get(id) {
            let flipped = flip_layer_horizontal(layer);
            doc.pixel_layers.insert(id, flipped);
        }
    }

    for object in &mut doc.objects {
        if let (Some(data), Some(size)) = (object.raster_data.as_ref(), object.raster_size) {
            if size.0 > 0 && size.1 > 0 {
                let expected = size.0 as usize * size.1 as usize * 4;
                if data.len() >= expected {
                    let src = PixelLayer::from_rgba(size.0, size.1, &data[..expected]);
                    let flipped = flip_layer_horizontal(&src);
                    object.raster_data = Some(flipped.to_rgba());
                    object.raster_size = Some((flipped.width, flipped.height));
                }
            }
        }
    }

    if let Some(selection) = doc.pixel_selection.take() {
        if selection.width > 0 && selection.height > 0 {
            let mut rgba = vec![0u8; selection.width as usize * selection.height as usize * 4];
            let n = (selection.width as usize * selection.height as usize).min(selection.coverage.len());
            for i in 0..n {
                let v = selection.coverage[i];
                let o = i * 4;
                rgba[o] = v;
                rgba[o + 1] = v;
                rgba[o + 2] = v;
                rgba[o + 3] = 255;
            }
            let src = PixelLayer::from_rgba(selection.width, selection.height, &rgba);
            let flipped = flip_layer_horizontal(&src);
            let out = flipped.to_rgba();
            let mut coverage = vec![0u8; flipped.width as usize * flipped.height as usize];
            for (i, slot) in coverage.iter_mut().enumerate() {
                let o = i * 4;
                if o < out.len() {
                    *slot = out[o];
                }
            }
            doc.pixel_selection = Some(PixelSelection::from_coverage(flipped.width, flipped.height, coverage));
        }
    }

}

fn flip_canvas_vertical(doc: &mut Document, old_h: u32) {
    for object in &mut doc.objects {
        let y = object.pos[1];
        let h = object.size[1];
        object.pos[1] = old_h as f32 - (y + h);

        if let ObjectKind::Curve(curve) = &mut object.kind {
            flip_curve_vertical(curve, old_h as f64);
        }
    }

    for slice in &mut doc.slices {
        let y0 = slice.bounds[1];
        let y1 = slice.bounds[3];
        slice.bounds[1] = old_h as f32 - y1;
        slice.bounds[3] = old_h as f32 - y0;
    }

    let tmp_t = doc.setup.margins.top;
    doc.setup.margins.top = doc.setup.margins.bottom;
    doc.setup.margins.bottom = tmp_t;

    let tmp_bt = doc.setup.bleed.top;
    doc.setup.bleed.top = doc.setup.bleed.bottom;
    doc.setup.bleed.bottom = tmp_bt;

    let ids = doc.pixel_layers.ids();
    for id in ids {
        if let Some(layer) = doc.pixel_layers.get(id) {
            let flipped = flip_layer_vertical(layer);
            doc.pixel_layers.insert(id, flipped);
        }
    }

    for object in &mut doc.objects {
        if let (Some(data), Some(size)) = (object.raster_data.as_ref(), object.raster_size) {
            if size.0 > 0 && size.1 > 0 {
                let expected = size.0 as usize * size.1 as usize * 4;
                if data.len() >= expected {
                    let src = PixelLayer::from_rgba(size.0, size.1, &data[..expected]);
                    let flipped = flip_layer_vertical(&src);
                    object.raster_data = Some(flipped.to_rgba());
                    object.raster_size = Some((flipped.width, flipped.height));
                }
            }
        }
    }

    if let Some(selection) = doc.pixel_selection.take() {
        if selection.width > 0 && selection.height > 0 {
            let mut rgba = vec![0u8; selection.width as usize * selection.height as usize * 4];
            let n = (selection.width as usize * selection.height as usize).min(selection.coverage.len());
            for i in 0..n {
                let v = selection.coverage[i];
                let o = i * 4;
                rgba[o] = v;
                rgba[o + 1] = v;
                rgba[o + 2] = v;
                rgba[o + 3] = 255;
            }
            let src = PixelLayer::from_rgba(selection.width, selection.height, &rgba);
            let flipped = flip_layer_vertical(&src);
            let out = flipped.to_rgba();
            let mut coverage = vec![0u8; flipped.width as usize * flipped.height as usize];
            for (i, slot) in coverage.iter_mut().enumerate() {
                let o = i * 4;
                if o < out.len() {
                    *slot = out[o];
                }
            }
            doc.pixel_selection = Some(PixelSelection::from_coverage(flipped.width, flipped.height, coverage));
        }
    }

}

fn turn_guides(guides: &mut crate::guides::Guides, t: CanvasTurn, old_w: u32, old_h: u32) {
    use crate::guides::GuideKind;
    for guide in &mut guides.items {
        match t {
            CanvasTurn::RotateLeft => match guide.kind {
                GuideKind::Vertical => {
                    guide.position = old_h as f32 - guide.position;
                    guide.kind = GuideKind::Horizontal;
                }
                GuideKind::Horizontal => guide.kind = GuideKind::Vertical,
                _ => {}
            },
            CanvasTurn::RotateRight => match guide.kind {
                GuideKind::Vertical => guide.kind = GuideKind::Horizontal,
                GuideKind::Horizontal => {
                    guide.position = old_w as f32 - guide.position;
                    guide.kind = GuideKind::Vertical;
                }
                _ => {}
            },
            CanvasTurn::FlipH => {
                if guide.kind == GuideKind::Vertical {
                    guide.position = old_w as f32 - guide.position;
                }
            }
            CanvasTurn::FlipV => {
                if guide.kind == GuideKind::Horizontal {
                    guide.position = old_h as f32 - guide.position;
                }
            }
        }
    }
}

fn rotate_layer_left(src: &PixelLayer) -> PixelLayer {
    let w = src.width;
    let h = src.height;
    let mut out = vec![0u8; (h as usize * w as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let px = src.get(x, y);
            let nx = h - 1 - y;
            let ny = x;
            let o = (ny as usize * h as usize + nx as usize) * 4;
            out[o] = px[0];
            out[o + 1] = px[1];
            out[o + 2] = px[2];
            out[o + 3] = px[3];
        }
    }
    PixelLayer::from_rgba(h, w, &out)
}

fn rotate_layer_right(src: &PixelLayer) -> PixelLayer {
    let w = src.width;
    let h = src.height;
    let mut out = vec![0u8; (h as usize * w as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let px = src.get(x, y);
            let nx = y;
            let ny = w - 1 - x;
            let o = (ny as usize * h as usize + nx as usize) * 4;
            out[o] = px[0];
            out[o + 1] = px[1];
            out[o + 2] = px[2];
            out[o + 3] = px[3];
        }
    }
    PixelLayer::from_rgba(h, w, &out)
}

fn flip_layer_horizontal(src: &PixelLayer) -> PixelLayer {
    let w = src.width;
    let h = src.height;
    let mut out = vec![0u8; (w as usize * h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let px = src.get(x, y);
            let nx = w - 1 - x;
            let o = (y as usize * w as usize + nx as usize) * 4;
            out[o] = px[0];
            out[o + 1] = px[1];
            out[o + 2] = px[2];
            out[o + 3] = px[3];
        }
    }
    PixelLayer::from_rgba(w, h, &out)
}

fn flip_layer_vertical(src: &PixelLayer) -> PixelLayer {
    let w = src.width;
    let h = src.height;
    let mut out = vec![0u8; (w as usize * h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let px = src.get(x, y);
            let ny = h - 1 - y;
            let o = (ny as usize * w as usize + x as usize) * 4;
            out[o] = px[0];
            out[o + 1] = px[1];
            out[o + 2] = px[2];
            out[o + 3] = px[3];
        }
    }
    PixelLayer::from_rgba(w, h, &out)
}

fn rotate_contour_left(contour: &mut crate::curve::Contour, height: f64) {
    for point in &mut contour.points {
        let x = point.pos[0];
        let y = point.pos[1];
        point.pos[0] = height - y;
        point.pos[1] = x;

        if let Some(handle) = point.handle_in.as_mut() {
            let hx = handle[0];
            let hy = handle[1];
            handle[0] = height - hy;
            handle[1] = hx;
        }
        if let Some(handle) = point.handle_out.as_mut() {
            let hx = handle[0];
            let hy = handle[1];
            handle[0] = height - hy;
            handle[1] = hx;
        }
    }
}

fn rotate_contour_right(contour: &mut crate::curve::Contour, width: f64) {
    for point in &mut contour.points {
        let x = point.pos[0];
        let y = point.pos[1];
        point.pos[0] = y;
        point.pos[1] = width - x;

        if let Some(handle) = point.handle_in.as_mut() {
            let hx = handle[0];
            let hy = handle[1];
            handle[0] = hy;
            handle[1] = width - hx;
        }
        if let Some(handle) = point.handle_out.as_mut() {
            let hx = handle[0];
            let hy = handle[1];
            handle[0] = hy;
            handle[1] = width - hx;
        }
    }
}

fn flip_contour_horizontal(contour: &mut crate::curve::Contour, width: f64) {
    for point in &mut contour.points {
        let x = point.pos[0];
        point.pos[0] = width - x;

        if let Some(handle) = point.handle_in.as_mut() {
            handle[0] = width - handle[0];
        }
        if let Some(handle) = point.handle_out.as_mut() {
            handle[0] = width - handle[0];
        }
    }
}

fn flip_contour_vertical(contour: &mut crate::curve::Contour, height: f64) {
    for point in &mut contour.points {
        let y = point.pos[1];
        point.pos[1] = height - y;

        if let Some(handle) = point.handle_in.as_mut() {
            handle[1] = height - handle[1];
        }
        if let Some(handle) = point.handle_out.as_mut() {
            handle[1] = height - handle[1];
        }
    }
}

fn rotate_curve_left(curve: &mut crate::curve::Curve, height: f64) {
    rotate_contour_left(&mut curve.contour, height);
    for hole in &mut curve.holes {
        rotate_contour_left(hole, height);
    }
}

fn rotate_curve_right(curve: &mut crate::curve::Curve, width: f64) {
    rotate_contour_right(&mut curve.contour, width);
    for hole in &mut curve.holes {
        rotate_contour_right(hole, width);
    }
}

fn flip_curve_horizontal(curve: &mut crate::curve::Curve, width: f64) {
    flip_contour_horizontal(&mut curve.contour, width);
    for hole in &mut curve.holes {
        flip_contour_horizontal(hole, width);
    }
}

fn flip_curve_vertical(curve: &mut crate::curve::Curve, height: f64) {
    flip_contour_vertical(&mut curve.contour, height);
    for hole in &mut curve.holes {
        flip_contour_vertical(hole, height);
    }
}

pub fn parse_turn_canvas_params(params: &serde_json::Value) -> Result<CanvasTurn, CommandError> {
    let turn = params
        .get("turn")
        .or_else(|| params.get("t"))
        .and_then(|v| v.as_str())
        .ok_or_else(CommandError::invalid_params)?;

    match turn {
        "rotate_left" | "RotateLeft" => Ok(CanvasTurn::RotateLeft),
        "rotate_right" | "RotateRight" => Ok(CanvasTurn::RotateRight),
        "flip_h" | "FlipH" => Ok(CanvasTurn::FlipH),
        "flip_v" | "FlipV" => Ok(CanvasTurn::FlipV),
        _ => Err(CommandError::invalid_params()),
    }
}

pub fn apply_turn_canvas(
    session: &mut Session,
    t: CanvasTurn,
) -> Result<CommandResult, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }

    let before = snapshot(&session.document, &session.guides)?;

    let (old_w, old_h) = document_pixel_size(&session.document)?;
    turn_canvas(&mut session.document, t)?;
    turn_guides(&mut session.guides, t, old_w, old_h);

    let after = snapshot(&session.document, &session.guides)?;
    let redo = Command {
        name: DOC_TURN_CANVAS_RESTORE.to_string(),
        params: json!({ "snapshot": after }),
    };
    let undo = Command {
        name: DOC_TURN_CANVAS_RESTORE.to_string(),
        params: json!({ "snapshot": before }),
    };

    session.undo.push(Change {
        name: DOC_TURN_CANVAS.to_string(),
        redo,
        undo,
    });

    session.generation += 1;
    session.document.version += 1;

    Ok(Ok(json!({ "turn": format_turn(t), "changed": true })))
}

fn format_turn(t: CanvasTurn) -> String {
    match t {
        CanvasTurn::RotateLeft => "rotate_left".to_string(),
        CanvasTurn::RotateRight => "rotate_right".to_string(),
        CanvasTurn::FlipH => "flip_h".to_string(),
        CanvasTurn::FlipV => "flip_v".to_string(),
    }
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    match cmd.name.as_str() {
        DOC_RESIZE => {
            if !session.has_document {
                return Some(false);
            }
            Some(parse_resize_params(&cmd.params).is_ok())
        }
        DOC_RESIZE_PIXEL_ART => Some(session.has_document && crate::pixel_art::options(&cmd.params).is_ok()),
        DOC_RESIZE_RESTORE | DOC_RESIZE_PIXEL_ART_RESTORE => Some(session.has_document && cmd.params.get("snapshot").is_some()),
        DOC_RESIZE_CANVAS => {
            if !session.has_document {
                return Some(false);
            }
            Some(parse_resize_canvas_params(&cmd.params).is_ok())
        }
        DOC_RESIZE_CANVAS_RESTORE => Some(session.has_document && cmd.params.get("snapshot").is_some()),
        DOC_TURN_CANVAS => {
            if !session.has_document {
                return Some(false);
            }
            Some(parse_turn_canvas_params(&cmd.params).is_ok())
        }
        DOC_TURN_CANVAS_RESTORE => Some(session.has_document && cmd.params.get("snapshot").is_some()),
        _ => None,
    }
}

fn flatten(result: Result<CommandResult, CommandError>) -> CommandResult {
    match result {
        Ok(inner) => inner,
        Err(err) => Err(err),
    }
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    match cmd.name.as_str() {
        DOC_RESIZE => {
            let parsed = match parse_resize_params(&cmd.params) {
                Ok(v) => v,
                Err(err) => return Some(Err(err)),
            };
            Some(flatten(apply_resize(session, parsed.0, parsed.1, parsed.2, parsed.3)))
        }
        DOC_RESIZE_PIXEL_ART => Some(apply_pixel_art_resize(session, cmd)),
        DOC_RESIZE_RESTORE | DOC_RESIZE_PIXEL_ART_RESTORE => Some(flatten(apply_restore(session, cmd))),
        DOC_RESIZE_CANVAS => {
            let parsed = match parse_resize_canvas_params(&cmd.params) {
                Ok(v) => v,
                Err(err) => return Some(Err(err)),
            };
            Some(flatten(apply_resize_canvas(session, parsed.0, parsed.1, parsed.2)))
        }
        DOC_RESIZE_CANVAS_RESTORE => Some(flatten(apply_restore(session, cmd))),
        DOC_TURN_CANVAS => {
            let parsed = match parse_turn_canvas_params(&cmd.params) {
                Ok(v) => v,
                Err(err) => return Some(Err(err)),
            };
            Some(flatten(apply_turn_canvas(session, parsed)))
        }
        DOC_TURN_CANVAS_RESTORE => Some(flatten(apply_restore(session, cmd))),
        _ => None,
    }
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    match cmd.name.as_str() {
        DOC_RESIZE_RESTORE | DOC_RESIZE_CANVAS_RESTORE | DOC_TURN_CANVAS_RESTORE | DOC_RESIZE_PIXEL_ART_RESTORE => {
            Some(restore_with_view(session, cmd).map(|_| json!({})))
        }
        _ => run(session, cmd),
    }
}
