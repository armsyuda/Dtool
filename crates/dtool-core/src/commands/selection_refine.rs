use crate::pixel::PixelLayer;
use crate::raster_source::LayerMask;
use crate::raster_view::RasterView;
use crate::selection::refinement::{refine, RefineStroke};
use crate::selection::{parse_length, PixelSelection, PixelSurface, RefineOutput, RefineSettings};
use crate::{CanvasFlags, Command, CommandError, CommandResult, Document, ObjectId, Session};
use serde_json::{json, Value};

pub const REFINE: &str = "selection.refine";
const MAX_PIXELS: u64 = 32 * 1024 * 1024;

pub fn capture(document: &Document, dimensions: [u32; 2]) -> Result<Vec<u8>, CommandError> {
    let [w, h] = dimensions;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    let page = document.pixel_selection.as_ref().and_then(|mask| mask.page_id);
    let surface = PixelSurface::for_page(document, page).ok_or_else(CommandError::no_document)?;
    let pixels = crate::layer_style::raster::render_region_scaled(
        document, &CanvasFlags::default(), &[], surface.bounds,
        w, h, w as f32 / surface.bounds[2],
    );
    if pixels.len() != w as usize * h as usize * 4 { return Err(CommandError::new("NOT_AVAILABLE")); }
    Ok(pixels)
}

fn parse(command: &Command, dpi: f32) -> Result<(RefineSettings, Vec<RefineStroke>, f32), CommandError> {
    let params = &command.params;
    let length = |key| {
        let value = params.get(key).unwrap_or(&Value::Null);
        let value = if value.is_null() { 0.0 } else { parse_length(value, dpi)? };
        if !(0.0..=1000.0).contains(&value) { return Err(CommandError::invalid_params()); }
        Ok(value)
    };
    let ramp = params["ramp"].as_f64().unwrap_or(1.0) as f32;
    let edge_shift = params["edge_shift"].as_f64().unwrap_or(0.0) as f32;
    if !(0.01..=100.0).contains(&ramp) || !(-1.0..=1.0).contains(&edge_shift) {
        return Err(CommandError::invalid_params());
    }
    let output = match params["output"].as_str() {
        None | Some("selection" | "Selection") => RefineOutput::Selection,
        Some("mask" | "Mask") => RefineOutput::Mask,
        Some("new_layer" | "NewLayer") => RefineOutput::NewLayer,
        Some("new_layer_with_mask" | "NewLayerWithMask") => RefineOutput::NewLayerWithMask,
        _ => return Err(CommandError::invalid_params()),
    };
    let strokes: Vec<RefineStroke> = match params.get("strokes") {
        None => Vec::new(),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?,
    };
    if strokes.len() > 128 || strokes.iter().any(|stroke| {
        !(0.1..=2048.0).contains(&stroke.width) || stroke.points.is_empty()
            || stroke.points.len() > super::pixel_brush::MAX_POINTS
            || stroke.points.iter().flatten().any(|value| !value.is_finite())
    }) { return Err(CommandError::invalid_params()); }
    Ok((RefineSettings {
        border_width: length("border_width")?,
        smooth: length("smooth")?,
        feather: length("feather")?,
        ramp,
        decontaminate: params["decontaminate"].as_bool().unwrap_or(false),
        output,
        ..RefineSettings::default()
    }, strokes, edge_shift))
}

pub(super) fn run(session: &mut Session, command: &Command, record: bool) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let before = session.document.pixel_selection.as_ref().filter(|mask| !mask.is_empty())
        .ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let (settings, strokes, edge_shift) = parse(command, session.document.dpi)?;
    let dimensions = [before.width, before.height];
    if u64::from(dimensions[0]) * u64::from(dimensions[1]) > MAX_PIXELS {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    let new_layer = matches!(settings.output, RefineOutput::NewLayer | RefineOutput::NewLayerWithMask);
    let needs_source = settings.border_width > 0.0 || new_layer
        || strokes.iter().any(|stroke| stroke.brush == crate::selection::refinement::RefineBrush::Matte);
    let rgba = if needs_source { capture(&session.document, dimensions)? } else { Vec::new() };
    let source = RasterView::new(dimensions[0], dimensions[1], &rgba);
    let mut mask = refine(before, &source, &settings, &strokes, edge_shift);
    mask.page_id = before.page_id;
    let mut info = json!({
        "border_width": settings.border_width,
        "smooth": settings.smooth,
        "feather": settings.feather,
        "ramp": settings.ramp,
        "decontaminate": settings.decontaminate,
        "changed": mask != *before,
        "output": settings.output,
    });
    if settings.output == RefineOutput::Selection {
        if mask == *before {
            return Ok(json!({"doc_version": session.generation, "changes": [], "result": info}));
        }
        let before = session.document.pixel_selection.replace(mask);
        let mut result = if record { super::pixel_selection::record_mask(session, command, before) }
            else { crate::command::pixel_selection_summary(&session.document) };
        result["result"] = info;
        return Ok(result);
    }
    if mask.is_empty() { return Err(CommandError::new("NOTHING_SELECTED")); }
    let mut document = session.document.clone();
    let id = if new_layer {
        create_layer(&mut document, mask, rgba, settings)
    } else {
        let selected = command.params["id"].as_u64().map(ObjectId).or_else(|| session.selection.key())
            .or_else(|| session.selection.ids.last().copied()).ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
        apply_mask(&mut document, selected, &mask)?;
        selected
    };
    info["id"] = json!(id.0);
    let selection = if new_layer { vec![id] } else { session.selection.ids.clone() };
    if record {
        super::document_change::commit(session, REFINE, document, selection, json!({"id": id.0, "result": info}))
    } else {
        session.document = document;
        session.selection.ids = selection;
        Ok(json!({"id": id.0, "result": info}))
    }
}

fn apply_mask(document: &mut Document, id: ObjectId, selection: &PixelSelection) -> Result<(), CommandError> {
    let object = document.get_object(id).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let dimensions = super::layer_mask::mask_dimensions(document, object).ok_or_else(CommandError::invalid_params)?;
    let mut coverage = super::layer_mask::resample_selection(document, object, dimensions, selection)
        .ok_or_else(CommandError::invalid_params)?;
    if let Some(existing) = document.layer_masks.iter_mut().find(|mask| mask.object == id) {
        for y in 0..coverage.height {
            for x in 0..coverage.width {
                let index = (y * coverage.width + x) as usize;
                coverage.coverage[index] = multiply(coverage.coverage[index], existing.coverage.get(x, y));
            }
        }
        existing.coverage = coverage;
    } else {
        document.layer_masks.push(LayerMask { object: id, coverage });
    }
    Ok(())
}

fn create_layer(document: &mut Document, mask: PixelSelection, mut rgba: Vec<u8>, settings: RefineSettings) -> ObjectId {
    let dims = [mask.width, mask.height];
    if settings.decontaminate { decontaminate(&mut rgba, &mask); }
    if settings.output == RefineOutput::NewLayer {
        for (pixel, &coverage) in rgba.chunks_exact_mut(4).zip(&mask.coverage) {
            pixel[3] = multiply(pixel[3], coverage);
        }
    }
    let page = mask.page_id.and_then(|id| document.pages.iter().position(|page| page.id == id)).unwrap_or(0);
    let mut object = super::layer_kinds::pixel_object(document, page, dims);
    object.name = "Refined Selection".to_string();
    let id = document.create_object(object);
    document.pixel_layers.insert(id, PixelLayer::from_rgba(dims[0], dims[1], &rgba));
    if settings.output == RefineOutput::NewLayerWithMask {
        let mut coverage = mask;
        coverage.page_id = None;
        document.layer_masks.push(LayerMask { object: id, coverage });
    }
    id
}

fn multiply(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}

fn decontaminate(rgba: &mut [u8], mask: &PixelSelection) {
    let width = mask.width as usize;
    let height = mask.height as usize;
    for (i, &coverage) in mask.coverage.iter().enumerate() {
        if coverage == 0 || coverage >= 250 || rgba[i * 4 + 3] == 0 { continue; }
        let (x, y) = (i % width, i / width);
        let sample = [1, 3, 6, 12].into_iter().find_map(|distance| {
            let left = x.saturating_sub(distance);
            let right = (x + distance).min(width - 1);
            let top = y.saturating_sub(distance);
            let bottom = (y + distance).min(height - 1);
            [(left, y), (right, y), (x, top), (x, bottom), (left, top), (left, bottom), (right, top), (right, bottom)]
                .into_iter().map(|(sx, sy)| sy * width + sx)
                .find(|&index| mask.coverage[index] >= 250 && rgba[index * 4 + 3] >= 250)
        });
        if let Some(sample) = sample {
            for channel in 0..3 {
                let at = i * 4 + channel;
                let value = rgba[sample * 4 + channel];
                rgba[at] = ((u16::from(rgba[at]) * u16::from(coverage) + u16::from(value) * u16::from(255 - coverage) + 127) / 255) as u8;
            }
        }
    }
}
