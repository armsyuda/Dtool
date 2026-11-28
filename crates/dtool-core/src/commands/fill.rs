use crate::brush::BrushOp;
use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::raster_view::RasterView;
use crate::selection::{PixelSelection, flood_mask};
use crate::{Command, CommandError, CommandResult, ObjectId, Session};
use serde::Deserialize;
use serde_json::{Value, json};

pub const TOOL: &str = "tool.fill";
pub const AREA: &str = "fill.area";
pub const FLOOD: &str = "fill.flood";
pub const ERASE: &str = "fill.flood_erase";
pub const FILL_RESTORE: &str = "fill.restore";
const MAX_PIXELS: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Source {
    #[default]
    CurrentLayer,
    AllLayers,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Blend {
    #[default]
    Replace,
    Normal,
}

#[derive(Deserialize)]
struct Args {
    target: Option<String>,
    point: Option<[f32; 2]>,
    at: Option<At>,
    color: Option<[u8; 4]>,
    tolerance: Option<f32>,
    contiguous: Option<bool>,
    antialias: Option<bool>,
    #[serde(default)]
    pigment: bool,
    #[serde(default)]
    source: Source,
    #[serde(default)]
    blend: Blend,
}

#[derive(Deserialize)]
struct At { x: u32, y: u32 }

pub fn canvas_command(session: &Session, erase: bool, point: [f32; 2]) -> Option<Command> {
    let tools = &session.tool_options;
    let tool = &session.active_tool;
    let colour = super::pixel_brush::settings(session, BrushOp::Paint).colour;
    if tool == TOOL {
        if !session.quick_mask.active && let Some(selection) = &session.document.pixel_selection {
            let surface = crate::selection::PixelSurface::for_page(&session.document, selection.page_id)?;
            if surface.coverage_at(selection, point) == 0 {
                return None;
            }
        }
        if !session.quick_mask.active && tools.flag(TOOL, "fill.context_fill") == Some(true) {
            return Some(Command::new(super::inpaint::FILL_SELECTION));
        }
        return Some(Command::with_params(AREA, json!({
            "point": point,
            "color": if tools.choice(TOOL, "fill.fill_type").as_deref() == Some("transparent") { [0; 4] } else { colour },
            "pigment": tools.flag(TOOL, "fill.pigment") == Some(true),
        })));
    }
    Some(Command::with_params(if erase { ERASE } else { FLOOD }, json!({
        "point": point,
        "color": colour,
        "tolerance": tools.number(tool, "fill.tolerance").unwrap_or(32.0),
        "contiguous": tools.flag(tool, "fill.contiguous").unwrap_or(true),
        "antialias": tools.flag(tool, "fill.antialias").unwrap_or(true),
        "source": tools.choice(tool, "fill.source").as_deref().unwrap_or("current_layer"),
        "blend": tools.choice(tool, "fill.blend").as_deref().unwrap_or("replace"),
    })))
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    matches!(cmd.name.as_str(), AREA | FLOOD | ERASE).then_some(session.has_document)
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    can_execute(session, cmd)?;
    Some(execute(session, cmd, true))
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<Result<(), CommandError>> {
    if cmd.name == FILL_RESTORE {
        return Some(restore_legacy(session, &cmd.params));
    }
    can_execute(session, cmd)?;
    Some(execute(session, cmd, false).map(|_| ()))
}

fn execute(session: &mut Session, cmd: &Command, record: bool) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let args: Args = serde_json::from_value(cmd.params.clone()).map_err(|_| CommandError::invalid_params())?;
    if args.point.is_some_and(|point| point.iter().any(|value| !value.is_finite()))
        || args.point.is_some() == args.at.is_some()
        || args.tolerance.is_some_and(|value| !(0.0..=100.0).contains(&value))
    {
        return Err(CommandError::invalid_params());
    }
    let erase = cmd.name == ERASE;
    let area = cmd.name == AREA;
    if session.quick_mask.active && args.target.is_none() {
        return fill_quick_mask(session, cmd, &args, erase, record);
    }

    let target = args.target.as_deref().map(|id| {
        id.strip_prefix("node:").and_then(|id| id.parse().ok()).map(ObjectId)
            .ok_or_else(CommandError::invalid_params)
    }).transpose()?;
    let Some((object, mut pixels, add_layer)) = super::pixel_brush::edit_layer(session, target, args.point, !erase)? else {
        return Ok(json!({"changes": []}));
    };
    let [w, h] = [pixels.width, pixels.height];
    ensure_size(w, h)?;
    let [width, height] = object.size.map(f32::abs);
    if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
        return Err(CommandError::invalid_params());
    }
    let matrix = object_matrix(&object);
    let seed = if let Some(point) = args.point {
        let inverse = invert_matrix(&matrix).ok_or_else(CommandError::invalid_params)?;
        let [x, y] = apply_matrix(&inverse, point);
        let local = [(x - object.pos[0]) * w as f32 / width, (y - object.pos[1]) * h as f32 / height];
        let Some(seed) = in_bounds(local, [w, h]) else { return Ok(json!({"changes": []})); };
        seed
    } else {
        let at = args.at.as_ref().ok_or_else(CommandError::invalid_params)?;
        if at.x >= w || at.y >= h { return Err(CommandError::invalid_params()); }
        [at.x, at.y]
    };

    let colour = if erase { [0; 4] } else { args.color.ok_or_else(CommandError::invalid_params)? };
    let doc = &session.document;
    let scale = crate::units::to_px(1.0, doc.units, doc.dpi).ok_or_else(CommandError::invalid_params)?;
    let to_world = |x: u32, y: u32| apply_matrix(&matrix, [
        object.pos[0] + (x as f32 + 0.5) * width / w as f32,
        object.pos[1] + (y as f32 + 0.5) * height / h as f32,
    ]);
    let selection_surface = doc.pixel_selection.as_ref().and_then(|selection| {
        crate::selection::PixelSurface::for_page(doc, selection.page_id)
    });
    if area && let Some(selection) = &doc.pixel_selection {
        let point = args.point.unwrap_or_else(|| to_world(seed[0], seed[1]));
        if selection_surface.is_none_or(|surface| surface.coverage_at(selection, point) == 0) {
            return Ok(json!({"changes": []}));
        }
    }
    let mut rgba = pixels.to_rgba();
    let mut mask = match (area, args.source) {
        (true, _) => PixelSelection::all(w, h),
        (false, Source::CurrentLayer) => matches(&rgba, [w, h], seed, &args),
        (false, Source::AllLayers) => {
            let [x, y, world_w, world_h] = crate::geometry::object_world_bounds(&object);
            let [source_w, source_h] = [world_w, world_h].map(|v| (v * scale).ceil().max(1.0) as u32);
            ensure_size(source_w, source_h)?;
            let render = crate::layer_style::raster::render_region_scaled(doc, &crate::CanvasFlags::default(), &[],
                [x, y, world_w, world_h], source_w, source_h, scale);
            let source = RasterView::new(source_w, source_h, &render);
            let mut sampled = Vec::with_capacity(rgba.len());
            for j in 0..h {
                for i in 0..w {
                    let point = to_world(i, j);
                    let pos = [(point[0] - x) * source_w as f32 / world_w, (point[1] - y) * source_h as f32 / world_h];
                    let sample = in_bounds(pos, [source_w, source_h])
                        .and_then(|[u, v]| source.pixel(u, v)).unwrap_or([0; 4]);
                    sampled.extend_from_slice(&sample);
                }
            }
            matches(&sampled, [w, h], seed, &args)
        }
    };
    for (index, (pixel, strength)) in rgba.chunks_exact_mut(4).zip(&mut mask.coverage).enumerate() {
        let mut coverage = *strength;
        if coverage != 0 && let Some(selection) = &doc.pixel_selection {
            let world = to_world(index as u32 % w, index as u32 / w);
            let alpha = selection_surface.map_or(0, |surface| surface.coverage_at(selection, world));
            coverage = ((u16::from(coverage) * u16::from(alpha) + 127) / 255) as u8;
        }
        let old = [pixel[0], pixel[1], pixel[2], pixel[3]];
        let pigment = area && args.pigment && colour[3] != 0;
        let over = pigment || !erase && matches!(args.blend, Blend::Normal);
        let next = composite(old, colour, coverage, over, pigment);
        if old == next { *strength = 0; } else { pixel.copy_from_slice(&next); }
    }
    let Some(bounds) = mask.bounds() else { return Ok(json!({"changes": []})); };
    pixels.set_region([0, 0, w, h], &rgba);
    pixels.compact();
    let mut document = doc.clone();
    let id = super::pixel_brush::install_pixels(&mut document, &session.selection.ids, object, pixels, add_layer);
    let result = json!({"node_id": id.0, "changed_bounds": bounds});
    if record {
        super::document_change::commit(session, &cmd.name, document, vec![id], result)
    } else {
        session.document = document;
        session.selection.ids = vec![id];
        Ok(result)
    }
}

fn matches(rgba: &[u8], [w, h]: [u32; 2], seed: [u32; 2], args: &Args) -> PixelSelection {
    flood_mask(&RasterView::new(w, h, rgba), seed, args.tolerance.unwrap_or(0.0),
        args.contiguous.unwrap_or(true), args.antialias.unwrap_or(false))
}

fn in_bounds([x, y]: [f32; 2], [w, h]: [u32; 2]) -> Option<[u32; 2]> {
    (x >= 0.0 && y >= 0.0 && x < w as f32 && y < h as f32).then_some([x as u32, y as u32])
}

fn ensure_size(w: u32, h: u32) -> Result<(), CommandError> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(CommandError::validation("PIXEL_SELECTION_TOO_LARGE"));
    }
    Ok(())
}

fn composite(old: [u8; 4], fill: [u8; 4], coverage: u8, over: bool, pigment: bool) -> [u8; 4] {
    if coverage == 0 { return old; }
    let t = f32::from(coverage) / 255.0;
    let added = f32::from(fill[3]) * t;
    let remaining = f32::from(old[3]) * (1.0 - if over { added / 255.0 } else { t });
    let alpha = (added + remaining).round() as u8;
    if alpha == 0 { return [0; 4]; }
    if pigment && added > 0.0 && remaining > 0.0 {
        let [r, g, b, _] = crate::brush::mix_colour(old, fill, added / (remaining + added), true);
        return [r, g, b, alpha];
    }
    let channel = |i: usize| ((f32::from(old[i]) * remaining + f32::from(fill[i]) * added) / (remaining + added)).round() as u8;
    [channel(0), channel(1), channel(2), alpha]
}

fn fill_quick_mask(session: &mut Session, cmd: &Command, args: &Args, erase: bool, record: bool) -> CommandResult {
    let Some(surface) = super::quickmask::surface(session, args.point) else { return Ok(json!({"changes": []})); };
    let [w, h] = surface.size;
    ensure_size(w, h)?;
    let seed = if let Some(point) = args.point {
        in_bounds(surface.doc_to_pixel(point), [w, h])
    } else {
        args.at.as_ref().and_then(|at| (at.x < w && at.y < h).then_some([at.x, at.y]))
    };
    let Some(seed) = seed else { return Ok(json!({"changes": []})); };
    let before = session.document.pixel_selection.as_ref();
    let mut edited = match before {
        Some(mask) if mask.width == w && mask.height == h => mask.clone(),
        Some(_) => return Err(CommandError::invalid_params()),
        None => {
            let mut mask = PixelSelection::empty(w, h);
            mask.page_id = Some(surface.page_id);
            mask
        }
    };
    let mask = match (cmd.name == AREA, args.source) {
        (true, _) => PixelSelection::all(w, h),
        (false, Source::CurrentLayer) => {
            let rgba: Vec<_> = edited.coverage.iter().flat_map(|&v| [v, v, v, 255]).collect();
            matches(&rgba, [w, h], seed, args)
        }
        (false, Source::AllLayers) => {
            let rgba = super::selection_magic_wand::merged_or_layer(session, None, surface)?;
            matches(&rgba, [w, h], seed, args)
        }
    };
    let ink = if erase { 0 } else { u16::from(session.quick_mask.ink) };
    for (old, weight) in edited.coverage.iter_mut().zip(mask.coverage) {
        let weight = u16::from(weight);
        *old = ((u16::from(*old) * (255 - weight) + ink * weight + 127) / 255) as u8;
    }
    if before.map_or_else(|| edited.is_empty(), |before| *before == edited) {
        return Ok(json!({"changes": []}));
    }
    let before = session.document.pixel_selection.replace(edited);
    if record { Ok(super::pixel_selection::record_mask(session, cmd, before)) }
    else { Ok(json!({"changes": []})) }
}

fn restore_legacy(session: &mut Session, params: &Value) -> Result<(), CommandError> {
    let id = params["node_id"].as_u64().ok_or_else(CommandError::invalid_params)?;
    let data: Vec<u8> = serde_json::from_value(params["data"].clone()).map_err(|_| CommandError::invalid_params())?;
    let object = session.document.find_object_mut(id).ok_or_else(CommandError::not_found)?;
    let (w, h) = object.raster_size.ok_or_else(CommandError::invalid_params)?;
    if u64::from(w) * u64::from(h) * 4 != data.len() as u64 { return Err(CommandError::invalid_params()); }
    object.raster_data = Some(data.clone());
    if let Some(layer) = session.document.pixel_layers.get_mut(ObjectId(id)) {
        layer.set_region([0, 0, w, h], &data);
    }
    session.document.version += 1;
    Ok(())
}
