use std::sync::atomic::{AtomicBool, Ordering};

use crate::{CommandError, Document, Object, ObjectId, ObjectKind, ai::AiImage};

use super::{Candidate, MAX_SIDE, Request, comfy};

/// Border pixels in left, top, right, bottom order.
pub type Borders = [u32; 4];

pub fn output_size([w, h]: [u32; 2], [left, top, right, bottom]: Borders) -> Option<[u32; 2]> {
    let size = [w.checked_add(left)?.checked_add(right)?, h.checked_add(top)?.checked_add(bottom)?];
    (size != [w, h] && size.iter().all(|side| (64..=MAX_SIDE).contains(side))).then_some(size)
}

pub fn image_size(document: &Document, id: ObjectId) -> Option<[u32; 2]> {
    let object = document.get_object(id).filter(|object| object.kind == ObjectKind::Image)?;
    if object.size.iter().any(|side| !side.is_finite() || side.abs() <= f32::EPSILON) {
        return None;
    }
    let [width, height] = document.pixel_layers.get(id).map(|layer| [layer.width, layer.height])
        .or_else(|| object.raster_size.map(|(width, height)| [width, height]))?;
    [width, height].iter().all(|side| (8..=MAX_SIDE).contains(side)).then_some([width, height])
}

pub fn canvas_borders(document: &Document, id: ObjectId) -> Option<Borders> {
    let image = document.get_object(id)?;
    let [width, height] = image_size(document, id)?;
    if image.rotation_deg.rem_euclid(360.0) != 0.0 || image.shear_deg != 0.0 {
        return None;
    }
    let [x, y, w, h] = if let Some(board) = crate::artboard::owner(document, id) {
        crate::artboard::artboard_bounds(document, board.id).ok()?
    } else {
        let page = crate::data_merge::layout::object_page_index(document, id);
        crate::data_merge::layout::page_rect(document, page)
    };
    let [iw, ih] = image.size.map(f32::abs);
    let [ix, iy] = image.pos;
    if ix >= x + w || iy >= y + h || ix + iw <= x || iy + ih <= y {
        return None;
    }
    // Allow for floating-point document units when an edge already fits.
    let pixels = |gap: f32, size: f32, count: u32| ((gap / size * count as f32 - 0.0001).ceil().max(0.0)) as u32;
    let mut borders = [
        pixels(ix - x, iw, width), pixels(iy - y, ih, height),
        pixels(x + w - ix - iw, iw, width), pixels(y + h - iy - ih, ih, height),
    ];
    if image.size[0] < 0.0 { borders.swap(0, 2); }
    if image.size[1] < 0.0 { borders.swap(1, 3); }
    Some(borders)
}

pub(super) fn place(source: &Object, result: &mut Object, size: [u32; 2], [left, top, right, bottom]: Borders) {
    let extent = source.size.map(f32::abs);
    let step = [extent[0] / size[0] as f32, extent[1] / size[1] as f32];
    let center = crate::geometry::box_centre(source);
    let displaced = crate::geometry::apply_matrix(&crate::geometry::object_matrix(source), [
        center[0] + (right as f32 - left as f32) * step[0] * 0.5,
        center[1] + (bottom as f32 - top as f32) * step[1] * 0.5,
    ]);
    result.size = [
        source.size[0] + (left + right) as f32 * step[0] * source.size[0].signum(),
        source.size[1] + (top + bottom) as f32 * step[1] * source.size[1].signum(),
    ];
    result.pos = [displaced[0] - result.size[0].abs() * 0.5, displaced[1] - result.size[1].abs() * 0.5];
}

pub(super) fn run(
    request: &Request, source: &AiImage, borders: Borders, cancelled: &AtomicBool,
) -> Result<Vec<Candidate>, CommandError> {
    let [left, top, _, _] = borders;
    let [width, height] = output_size([source.width, source.height], borders)
        .ok_or_else(|| CommandError::new("AI_IMAGE_OUTPAINT_SIZE"))?;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    let mut mask = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        if cancelled.load(Ordering::Acquire) {
            return Err(CommandError::new("CANCELLED"));
        }
        for x in 0..width {
            let u = x.saturating_sub(left).min(source.width - 1);
            let v = y.saturating_sub(top).min(source.height - 1);
            let pixel = (v as usize * source.width as usize + u as usize) * 4;
            rgba.extend_from_slice(&source.rgba[pixel..pixel + 4]);
            // Feather on the generated side of the seam. The source rectangle's mask stays zero.
            let dx = left.saturating_sub(x).max(x.saturating_sub(left + source.width - 1));
            let dy = top.saturating_sub(y).max(y.saturating_sub(top + source.height - 1));
            mask.push((dx.max(dy).min(8) * 255 / 8) as u8);
        }
    }
    let expanded = AiImage { width, height, rgba };
    comfy::run(request, Some(&expanded), Some(&mask), cancelled)
}
