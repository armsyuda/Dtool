use crate::raster_source::{dimensions, LayerMask};
use crate::selection::PixelSelection;
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, ObjectKind, Session};
use serde_json::json;

pub const SET: &str = "layer.mask.set";
pub const QUERY: &str = "layer.mask.query";
pub const INVERT: &str = "layer.mask.invert";
pub const FROM_SELECTION: &str = "layer.mask.from_selection";
pub const RELEASE: &str = "layer.mask.release";

fn ours(name: &str) -> bool {
    matches!(name, SET | QUERY | INVERT | FROM_SELECTION | RELEASE)
}

pub(crate) fn mask_dimensions(document: &Document, object: &Object) -> Option<[usize; 2]> {
    if let Some(size) = dimensions(document, object) {
        return Some(size);
    }
    if !matches!(object.kind, ObjectKind::Group) && crate::outline::object_outline(object).is_none() {
        return None;
    }
    let [w, h] = object.size.map(f32::abs);
    let width = crate::units::to_px(w, document.units, document.dpi)?;
    let height = crate::units::to_px(h, document.units, document.dpi)?;
    let largest = width.max(height);
    if !largest.is_finite() || !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    // Use a bounded raster grid even for large design documents.
    let scale = (2000.0 / largest).min(1.0);
    Some([width, height].map(|length| (length * scale).ceil().clamp(1.0, 2000.0) as usize))
}

pub(super) fn resample_selection(document: &Document, object: &Object, size: [usize; 2], selection: &PixelSelection) -> Option<PixelSelection> {
    resample_selection_in_frame(document, object, size, selection, [0.0, 0.0, 1.0, 1.0])
}

pub(super) fn resample_selection_in_frame(
    document: &Document, object: &Object, size: [usize; 2],
    selection: &PixelSelection, [left, top, width, height]: [f32; 4],
) -> Option<PixelSelection> {
    let surface = crate::selection::PixelSurface::for_page(document, selection.page_id)?;
    let scale = [selection.width as f32 / surface.bounds[2], selection.height as f32 / surface.bounds[3]];
    let quad = crate::geometry::object_world_quad(object);
    let across = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
    let down = [quad[3][0] - quad[0][0], quad[3][1] - quad[0][1]];
    let mut mask = PixelSelection::empty(size[0] as u32, size[1] as u32);
    for y in 0..size[1] {
        let v = top + height * (y as f32 + 0.5) / size[1] as f32;
        for x in 0..size[0] {
            let u = left + width * (x as f32 + 0.5) / size[0] as f32;
            let point = [quad[0][0] + across[0] * u + down[0] * v, quad[0][1] + across[1] * u + down[1] * v];
            let [px, py] = std::array::from_fn(|axis| (point[axis] - surface.bounds[axis]) * scale[axis]);
            if px >= 0.0 && py >= 0.0 {
                mask.coverage[y * size[0] + x] = selection.get(px as u32, py as u32);
            }
        }
    }
    Some(mask)
}

fn next_coverage(session: &Session, command: &Command, object: &Object, size: [usize; 2], current: &Option<PixelSelection>) -> Result<Option<PixelSelection>, CommandError> {
    match command.name.as_str() {
        INVERT => {
            let mut mask = current.clone().ok_or_else(CommandError::not_found)?;
            mask.coverage.iter_mut().for_each(|value| *value = 255 - *value);
            Ok(Some(mask))
        }
        RELEASE => {
            current.as_ref().ok_or_else(CommandError::not_found)?;
            Ok(None)
        }
        FROM_SELECTION => session.document.pixel_selection.as_ref().filter(|selection| !selection.is_empty())
            .and_then(|selection| resample_selection(&session.document, object, size, selection))
            .map(Some).ok_or_else(|| CommandError::new("NOTHING_SELECTED")),
        _ => {
            let value = command.params.get("mask").ok_or_else(CommandError::invalid_params)?;
            let coverage: Option<PixelSelection> = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
            if let Some(mask) = &coverage {
                if [mask.width as usize, mask.height as usize] != size || size[0].checked_mul(size[1]) != Some(mask.coverage.len()) { return Err(CommandError::invalid_params()); }
            }
            Ok(coverage)
        }
    }
}

fn run_inner(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let id = ObjectId(command.params.get("id").and_then(serde_json::Value::as_u64).ok_or_else(CommandError::invalid_params)?);
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    let existing = session.document.layer_masks.iter().find(|mask| mask.object == id);
    let current = existing.map(|mask| mask.coverage.clone());
    let frame = existing.and_then(|mask| mask.frame);
    if command.name == QUERY { return Ok(json!({"id":id.0,"mask":current,"frame":frame,"doc_version":session.generation})); }
    if session.document.is_object_locked(object) { return Err(CommandError::new("LOCKED")); }
    let size = if matches!(command.name.as_str(), SET | FROM_SELECTION) {
        mask_dimensions(&session.document, object).ok_or_else(CommandError::invalid_params)?
    } else {
        [0, 0]
    };
    let coverage = next_coverage(session, command, object, size, &current)?;
    let next_frame = if command.name == INVERT { frame } else { None };
    if current == coverage && frame == next_frame { return Ok(json!({"id":id.0,"doc_version":session.generation,"changes":[]})); }
    let mut document = session.document.clone();
    document.layer_masks.retain(|mask| mask.object != id);
    if let Some(coverage) = coverage {
        let mut mask = LayerMask::new(id, coverage);
        mask.frame = next_frame;
        document.layer_masks.push(mask);
    }
    let name = command.name.clone();
    super::document_change::commit(session, &name, document, session.selection.ids.clone(), json!({"id":id.0}))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    ours(command.name.as_str()).then(|| {
        session.has_document && command.params["id"].as_u64().map(ObjectId)
            .and_then(|id| session.document.get_object(id))
            .is_some_and(|object| command.name == QUERY || !session.document.is_object_locked(object))
    })
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    ours(command.name.as_str()).then(|| run_inner(session, command))
}
