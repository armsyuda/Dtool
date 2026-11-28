use serde_json::Value;

use crate::ai::AiImage;
use crate::geometry::{apply_matrix, invert_matrix, object_matrix, object_world_bounds};
use crate::selection::{PixelSelection, PixelSurface};
use crate::{CommandError, ObjectKind, Session};

use super::{Job, source_image};

pub(super) fn mask(
    session: &Session,
    params: &Value,
    job: &Job,
    matte: &AiImage,
) -> Result<PixelSelection, CommandError> {
    if params["replace"].as_bool() == Some(true) {
        return Err(CommandError::invalid_params());
    }
    let invalid = || CommandError::new("AI_IMAGE_SUBJECT_SOURCE");
    let empty = || CommandError::new("AI_IMAGE_SUBJECT_EMPTY");
    let id = job.source.ok_or_else(invalid)?;
    let doc = &session.document;
    let object = doc.get_object(id).filter(|object| object.kind == ObjectKind::Image)
        .ok_or_else(invalid)?;
    let original = job.original.as_deref().ok_or_else(invalid)?;
    let current = source_image(session, id).map_err(|_| invalid())?;
    if current.width != original.width || current.height != original.height || current.rgba != original.rgba {
        return Err(invalid());
    }

    let page = doc.pages.get(crate::data_merge::layout::object_page_index(doc, id)).ok_or_else(invalid)?;
    let surface = PixelSurface::for_page(doc, Some(page.id)).ok_or_else(invalid)?;
    let [width, height] = surface.size;
    if u64::from(width) * u64::from(height) > 32 * 1024 * 1024 {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    let size = object.size.map(f32::abs);
    if size.iter().any(|&side| !side.is_finite() || side <= f32::EPSILON) {
        return Err(invalid());
    }
    let inverse = invert_matrix(&object_matrix(object)).ok_or_else(invalid)?;
    let [x, y, w, h] = object_world_bounds(object);
    let [start_x, start_y] = surface.doc_to_pixel([x, y]).map(|value| value.floor().max(0.0) as u32);
    let [end_x, end_y] = surface.doc_to_pixel([x + w, y + h]).map(|value| value.ceil().max(0.0) as u32);
    let mut mask = PixelSelection::empty(width, height);
    mask.page_id = Some(page.id);
    let mut any_coverage = false;

    for y in start_y..end_y.min(height) {
        for x in start_x..end_x.min(width) {
            let point = surface.pixel_to_doc([x as f32 + 0.5, y as f32 + 0.5]);
            let local = apply_matrix(&inverse, point);
            let [u, v] = std::array::from_fn(|axis| (local[axis] - object.pos[axis]) / size[axis]);
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let alpha = alpha_at(matte, u, v);
            mask.coverage[(y * width + x) as usize] = alpha;
            any_coverage |= alpha != 0;
        }
    }
    if !any_coverage {
        return Err(empty());
    }
    Ok(mask)
}

fn alpha_at(matte: &AiImage, u: f32, v: f32) -> u8 {
    let x = (u * matte.width as f32 - 0.5).clamp(0.0, (matte.width - 1) as f32);
    let y = (v * matte.height as f32 - 0.5).clamp(0.0, (matte.height - 1) as f32);
    let (left, top) = (x as u32, y as u32);
    let (right, bottom) = ((left + 1).min(matte.width - 1), (top + 1).min(matte.height - 1));
    let dx = x - left as f32;
    let dy = y - top as f32;
    let sample = |x, y| f32::from(matte.rgba[((y * matte.width + x) * 4 + 3) as usize]);
    let row = |y| sample(left, y) * (1.0 - dx) + sample(right, y) * dx;
    (row(top) * (1.0 - dy) + row(bottom) * dy).round() as u8
}
