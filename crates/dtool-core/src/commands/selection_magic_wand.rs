use dtool_protocol::{Command, CommandError, CommandResult};

use crate::command::Session;
use crate::object::ObjectId;
use crate::raster_view::RasterView;
use crate::selection::{brush_mask, combine_page_mask, flood_mask, PixelSurface, SelectionOp};

const WAND: &str = "selection.magicWand";
const BRUSH: &str = "selection.brush";
pub const COLOUR_RANGE: &str = "selection.colorRange";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), WAND | BRUSH | COLOUR_RANGE).then_some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command)?;
    let before = session.document.pixel_selection.clone();
    Some(run_inner(session, command).map(|mut value| {
        if before != session.document.pixel_selection {
            let summary = super::pixel_selection::record_mask(session, command, before);
            value["doc_version"] = summary["doc_version"].clone();
            value["changes"] = summary["changes"].clone();
        }
        value
    }))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    can_execute(session, command)?;
    Some(run_inner(session, command).map(|_| ()))
}

fn run_inner(session: &mut Session, cmd: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let surface = PixelSurface::for_command(&session.document, &cmd.params)?;
    let [w, h] = surface.size;
    if u64::from(w) * u64::from(h) > 32 * 1024 * 1024 {
        return Err(CommandError::validation("PIXEL_SELECTION_TOO_LARGE"));
    }
    let params = &cmd.params;
    let op = match params["op"].as_str() {
        Some("add") => SelectionOp::Add,
        Some("subtract") => SelectionOp::Subtract,
        Some("intersect") => SelectionOp::Intersect,
        Some("new") => SelectionOp::New,
        None if cmd.name != BRUSH => SelectionOp::New,
        None => SelectionOp::Add,
        _ => return Err(CommandError::invalid_params()),
    };
    let mut mask = if cmd.name == WAND {
        let at = &params["at"];
        let coordinate = |key, limit| {
            at[key].as_f64().filter(|v| *v >= 0.0 && *v < f64::from(limit))
                .map(|v| v as u32).ok_or_else(CommandError::invalid_params)
        };
        let seed = [coordinate("x", w)?, coordinate("y", h)?];
        let scope = params.get("sampling_scope").filter(|scope| !scope.is_null());
        let raster = sample_pixels(session, scope, surface)?;
        let contiguous = params["contiguous"].as_bool().unwrap_or(true);
        let antialias = params["antialias"].as_bool().unwrap_or(false);
        let tolerance = params["tolerance"].as_f64().unwrap_or(32.0) as f32;
        flood_mask(&RasterView::new(w, h, &raster), seed, tolerance, contiguous, antialias)
    } else if cmd.name == COLOUR_RANGE {
        let colour: [u8; 4] = serde_json::from_value(params["color"].clone()).map_err(|_| CommandError::invalid_params())?;
        let tolerance = match params.get("tolerance") {
            Some(value) => value.as_f64().filter(|value| (0.0..=100.0).contains(value))
                .ok_or_else(CommandError::invalid_params)? as f32,
            None => 10.0,
        };
        let scope = params.get("sampling_scope").filter(|scope| !scope.is_null());
        let raster = merged_or_layer(session, scope, surface)?;
        crate::selection::colour_mask(&RasterView::new(w, h, &raster), colour, tolerance)
    } else {
        let invalid = CommandError::invalid_params;
        let mut points: Vec<[f32; 3]> = serde_json::from_value(params["points"].clone()).map_err(|_| invalid())?;
        if points.is_empty() || points.len() > super::pixel_brush::MAX_POINTS
            || points.iter().flatten().any(|value| !value.is_finite()) {
            return Err(invalid());
        }
        let size = params["size_px"].as_f64().filter(|size| (1.0..=2048.0).contains(size)).ok_or_else(invalid)? as f32;
        let hardness = params["hardness"].as_f64().unwrap_or(1.0) as f32;
        let margin = size * 0.5 + 1.0;
        for point in &mut points {
            point[0] = point[0].clamp(-margin, w as f32 + margin);
            point[1] = point[1].clamp(-margin, h as f32 + margin);
        }
        let mut mask = brush_mask(w, h, &points, size, hardness);
        if params["snap_to_edges"].as_bool() == Some(true) {
            let [x, y, _] = points[0];
            if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
                return Err(invalid());
            }
            let scope = serde_json::json!(if params["all_layers"].as_bool() == Some(true) { "all_layers" } else { "current_layer" });
            let raster = sample_pixels(session, Some(&scope), surface)?;
            let matching = flood_mask(&RasterView::new(w, h, &raster), [x as u32, y as u32], 15.0, true, false);
            mask.combine(&matching, SelectionOp::Intersect);
        }
        mask
    };
    if mask.is_empty() && cmd.name == WAND {
        return Err(CommandError::validation("NOTHING_SELECTED"));
    }
    let (area, bounds) = (mask.area(), mask.bounds());
    mask.page_id = params["page_id"].as_u64().map(crate::document::PageId);
    if op == SelectionOp::New || op == SelectionOp::Intersect || !mask.is_empty() {
        if op != SelectionOp::Subtract || session.document.pixel_selection.is_some() {
            combine_page_mask(&mut session.document, mask, op)?;
        }
    }
    if cmd.name != COLOUR_RANGE {
        return Ok(serde_json::json!({"selected": true, "area_px": area, "bounds": bounds}));
    }
    let mut result = crate::command::pixel_selection_summary(&session.document);
    result["selected"] = serde_json::json!(!result["empty"].as_bool().unwrap_or(true));
    Ok(result)
}

pub fn colour_source(session: &Session, scope: Option<&serde_json::Value>) -> Result<([u32; 2], Vec<u8>), CommandError> {
    colour_source_for_page(session, scope, None)
}

pub fn colour_source_for_page(session: &Session, scope: Option<&serde_json::Value>, page_id: Option<crate::document::PageId>) -> Result<([u32; 2], Vec<u8>), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let surface = PixelSurface::for_page(&session.document, page_id).ok_or_else(CommandError::no_document)?;
    let [w, h] = surface.size;
    if u64::from(w) * u64::from(h) > 32 * 1024 * 1024 {
        return Err(CommandError::validation("PIXEL_SELECTION_TOO_LARGE"));
    }
    let rgba = merged_or_layer(session, scope, surface)?;
    Ok(([w, h], rgba))
}

pub(super) fn merged_or_layer(session: &Session, scope: Option<&serde_json::Value>, surface: PixelSurface) -> Result<Vec<u8>, CommandError> {
    let merged = serde_json::Value::String("all_layers".into());
    sample_pixels(session, scope.or(Some(&merged)), surface)
}

fn sample_pixels(session: &Session, scope: Option<&serde_json::Value>, surface: PixelSurface) -> Result<Vec<u8>, CommandError> {
    let doc = &session.document;
    let [w, h] = surface.size;
    let layer = match scope.and_then(serde_json::Value::as_str) {
        Some("all_layers") => None,
        None if scope.is_none() => current_layer(session),
        Some("current_layer") => current_layer(session),
        _ => {
            let id = scope.and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()));
            Some(ObjectId(id.ok_or_else(CommandError::invalid_params)?))
        }
    };
    let expected = w as usize * h as usize * 4;
    if let Some(id) = layer {
        let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
        if matches!(object.kind, crate::object::ObjectKind::Shape)
            && object.pickable && object.opacity == 1.0
            && object.pos == [surface.bounds[0], surface.bounds[1]] && object.raster_size == Some((w, h))
            && let Some(raster) = object.raster_data.as_ref().filter(|raster| raster.len() == expected)
        {
            return Ok(raster.clone());
        }
    } else if scope.and_then(serde_json::Value::as_str) != Some("all_layers") {
        return Ok(vec![0; expected]);
    }
    let isolated = layer.and_then(|id| super::colour_pick::layer_document(doc, id));
    let pixels = crate::layer_style::raster::render_region_scaled(
        isolated.as_ref().unwrap_or(doc), &crate::CanvasFlags::default(), &[],
        surface.bounds, w, h, w as f32 / surface.bounds[2],
    );
    if pixels.len() != expected {
        return Err(CommandError::not_found());
    }
    Ok(pixels)
}

fn current_layer(session: &Session) -> Option<ObjectId> {
    session.selection.key().or_else(|| session.selection.ids.last().copied()).or_else(|| {
        session.document.objects.iter().rev()
            .find(|object| object.pickable && !object.locked && (object.raster_data.is_some() || session.document.pixel_layers.contains(object.id)))
            .map(|object| object.id)
    })
}
