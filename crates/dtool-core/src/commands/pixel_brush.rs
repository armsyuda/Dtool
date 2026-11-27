use crate::brush::{self, BrushOp, BrushSettings, MixerLoad, OpParams, SharpenMode, SpongeMode, StrokePoint, TipTransform, TonalRange};
use crate::context_toolbar::{MIXER_TOOL, TONE_TOOL};
use crate::tone::{Mode as ToneMode, Range as ToneRange, ToneLayer};
use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::object::Object;
use crate::paint::Paint;
use crate::pixel::PixelLayer;
use crate::raster_view::RasterViewMut;
use crate::{Command, CommandError, CommandResult, Document, Session};
use serde::Deserialize;
use serde_json::json;
use std::borrow::Cow;

pub const STROKE: &str = "pixel.brush.stroke";
pub const MAX_POINTS: usize = 16_384;
const MIXER_LOAD: &str = "pixel.brush.mixer.load";
const MIXER_CLEAN: &str = "pixel.brush.mixer.clean";
const TONE_ADD: &str = "pixel.brush.tone.add";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrokeArgs {
    points: Vec<[f32; 2]>,
    pressure: Option<Vec<f32>>,
    tilt: Option<Vec<[f32; 2]>>,
    velocity: Option<Vec<f32>>,
    #[serde(default)]
    invert: bool,
    clone_source: Option<[f32; 2]>,
}

impl StrokeArgs {
    fn valid(&self) -> bool {
        let count = self.points.len();
        let unit = |v: &f32| (0.0..=1.0).contains(v);
        count != 0 && count <= MAX_POINTS && self.points.iter().flatten().all(|v| v.is_finite())
            && self.clone_source.is_none_or(|point| point.iter().all(|v| v.is_finite()))
            && self.pressure.as_ref().is_none_or(|values| values.len() == count && values.iter().all(unit))
            && self.velocity.as_ref().is_none_or(|values| values.len() == count && values.iter().all(unit))
            && self.tilt.as_ref().is_none_or(|values| values.len() == count
                && values.iter().flatten().all(|v| (-1.0..=1.0).contains(v)))
    }

    fn samples(&self, to_raster: impl Fn([f32; 2]) -> [f32; 2]) -> Vec<StrokePoint> {
        self.points.iter().enumerate().map(|(index, point)| {
            let [x, y] = to_raster(*point);
            StrokePoint {
                x, y,
                pressure: self.pressure.as_ref().map_or(1.0, |v| v[index]),
                has_pressure: self.pressure.is_some(),
                tilt: self.tilt.as_ref().map_or([0.0; 2], |v| v[index]),
                velocity: self.velocity.as_ref().map_or(0.0, |v| v[index]),
            }
        }).collect()
    }
}

pub fn operation(tool: &str) -> Option<BrushOp> {
    match tool {
        "tool.paint.brush" | "tool.paint_brush" | crate::context_toolbar::CALLIGRAPHY_TOOL => Some(BrushOp::Paint),
        "tool.paint.pencil" | "tool.paint.pixel" => Some(BrushOp::Pixel),
        "tool.eraser.erase" | "tool.eraser" => Some(BrushOp::Erase),
        "tool.eraser.background" => Some(BrushOp::BackgroundErase),
        "tool.paint.colour_replace" => Some(BrushOp::ColourReplace),
        crate::context_toolbar::CLONE_TOOL => Some(BrushOp::Clone),
        MIXER_TOOL => Some(BrushOp::Mixer),
        TONE_TOOL => Some(BrushOp::Tone),
        "tool.paint.adjustment" | "tool.dodge" => Some(BrushOp::Dodge),
        "tool.burn" => Some(BrushOp::Burn),
        "tool.paint.sponge" => Some(BrushOp::Sponge),
        "tool.paint.filter" | "tool.paint.blur" => Some(BrushOp::Blur),
        "tool.sharpen" => Some(BrushOp::Sharpen),
        "tool.paint.median" => Some(BrushOp::Median),
        "tool.paint.smudge" | "tool.eraser.smudge" => Some(BrushOp::Smudge),
        _ => None,
    }
}

pub fn settings(session: &Session, op: BrushOp) -> BrushSettings {
    settings_for(session, &session.active_tool, op)
}

pub(super) fn settings_for(session: &Session, tool: &str, op: BrushOp) -> BrushSettings {
    let number = |key, fallback| session.tool_options.number(tool, key).unwrap_or(fallback) as f32;
    let colour_option = match op {
        BrushOp::Mixer => "brush.mixer_colour",
        BrushOp::Tone => "brush.tone_colour",
        _ => "brush.replacement_colour",
    };
    let fill = session.tool_options.choice(tool, colour_option)
        .and_then(|hex| match crate::paint::paint_from_legacy_str(&hex) { Some(Paint::Solid { colour }) => Some(colour), _ => None })
        .or_else(|| session.selection.ids.first()
        .and_then(|id| session.document.get_object(*id))
        .and_then(|object| match object.fill { Paint::Solid { colour } => Some(colour), _ => None }))
        .or_else(|| match super::defaults::current().fill { Paint::Solid { colour } => Some(colour), _ => None });
    use crate::context_toolbar::{CALLIGRAPHY_ANGLE, CALLIGRAPHY_DEFAULT_ANGLE, CALLIGRAPHY_DEFAULT_ROUNDNESS,
        CALLIGRAPHY_ROUNDNESS, CALLIGRAPHY_TOOL};
    let calligraphy = tool == CALLIGRAPHY_TOOL;
    let tip = (!calligraphy).then(|| session.tool_options.raster_tip(tool)).flatten();
    BrushSettings {
        tip: tip.map(|tip| tip.id.clone()),
        tip_kind: tip.map_or(crate::brush::TipKind::Round, |tip| tip.kind),
        stamp: tip.and_then(|tip| tip.image.clone()),
        additional_stamps: tip.map_or_else(Vec::new, |tip| tip.additional_images.clone()),
        texture: tip.and_then(|tip| tip.texture.clone()),
        sub_brushes: tip.map_or_else(Vec::new, |tip| tip.sub_brushes.clone()),
        stamp_colour: op == BrushOp::Paint,
        spacing: tip.map_or(0.1, |tip| tip.spacing),
        rotation: if calligraphy { number(CALLIGRAPHY_ANGLE, f64::from(CALLIGRAPHY_DEFAULT_ANGLE)) }
            else { tip.map_or(0.0, |tip| tip.rotation) },
        dynamics: tip.map(|tip| tip.dynamics.clone()).unwrap_or_default(),
        size_px: if op == BrushOp::Pixel { 1.0 } else { number("brush.width", tip.map_or(64.0, |tip| tip.size_px) as f64) },
        opacity: number("brush.opacity", tip.map_or(100.0, |tip| tip.opacity * 100.0) as f64) / 100.0,
        accumulation: tip.filter(|_| matches!(op, BrushOp::Paint | BrushOp::Erase)).and_then(|tip| tip.accumulation),
        flow: number("brush.flow", tip.map_or(100.0, |tip| tip.flow * 100.0) as f64) / 100.0,
        hardness: number("brush.hardness", tip.map_or(80.0, |tip| tip.hardness * 100.0) as f64) / 100.0,
        roundness: if calligraphy { number(CALLIGRAPHY_ROUNDNESS, f64::from(CALLIGRAPHY_DEFAULT_ROUNDNESS)) / 100.0 }
            else { tip.map_or(1.0, |tip| tip.roundness) },
        pressure_size: session.tool_options.flag(tool, "brush.pressure_size").unwrap_or(false),
        protect_alpha: session.tool_options.flag(tool, "brush.protect_alpha").unwrap_or(false),
        symmetry: number("brush.symmetry", 1.0).round().clamp(1.0, 8.0) as u32,
        mirror: session.tool_options.flag(tool, "brush.mirror").unwrap_or(false),
        symmetry_lock: session.tool_options.flag(tool, "brush.symmetry_lock").unwrap_or(false),
        symmetry_origin: [number("brush.symmetry_x", 50.0) / 100.0, number("brush.symmetry_y", 50.0) / 100.0],
        stabilizer: session.tool_options.flag(tool, "brush.stabilizer").unwrap_or(false),
        wet_edges: session.tool_options.flag(tool, "brush.wet_edges").unwrap_or(false),
        wet_edge_curve: tip.and_then(|tip| tip.wet_edges.curve.clone()),
        wet_edge_scope: tip.map(|tip| tip.wet_edges.scope).unwrap_or_default(),
        colour: fill.map_or([0, 0, 0, 255], |c| [c.r, c.g, c.b, c.a]),
        ..Default::default()
    }
}

pub(super) fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        STROKE => Some(session.has_document && operation(&session.active_tool)
            .is_some_and(|op| {
                if op == BrushOp::Clone {
                    clone_layer(session).is_some()
                } else {
                    !session.quick_mask.active || supports_quick_mask(op)
                }
            })),
        TONE_ADD => Some(session.has_document && session.active_tool == TONE_TOOL && !session.quick_mask.active
            && tone_source(session).is_ok()),
        MIXER_LOAD | MIXER_CLEAN => Some(session.has_document && session.active_tool == MIXER_TOOL && !session.quick_mask.active),
        _ => None,
    }
}

fn supports_quick_mask(op: BrushOp) -> bool {
    matches!(op, BrushOp::Paint | BrushOp::Pixel | BrushOp::Erase)
}

pub(super) fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        STROKE => Some(apply_stroke(session, command)),
        TONE_ADD => Some(tone_brush(session, None)),
        MIXER_LOAD | MIXER_CLEAN => Some(set_mixer_load(session, command)),
        _ => None,
    }
}

fn set_mixer_load(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    if session.active_tool != MIXER_TOOL || session.quick_mask.active {
        return Err(CommandError::invalid_params());
    }
    let colour = settings(session, BrushOp::Mixer).colour;
    session.mixer_load = if command.name == MIXER_LOAD && colour[3] != 0 {
        MixerLoad::Colour(colour)
    } else {
        MixerLoad::Clean
    };
    Ok(json!({"colour": session.mixer_load.colour(), "changes": []}))
}

fn editable(document: &Document, object: &Object) -> bool {
    let mut current = Some(object);
    for _ in 0..=document.objects.len() {
        let Some(layer) = current else { return true };
        if layer.locked || !layer.pickable { return false; }
        current = layer.parent_id.and_then(|parent| document.get_object(parent));
    }
    false
}

fn apply_stroke(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let op = operation(&session.active_tool).ok_or_else(CommandError::invalid_params)?;
    let args: StrokeArgs = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
    if !args.valid() {
        return Err(CommandError::invalid_params());
    }
    if session.quick_mask.active {
        if !supports_quick_mask(op) { return Ok(json!({"changes": []})); }
        return paint_quick_mask(session, command, &args, op);
    }
    if op == BrushOp::Tone { return tone_brush(session, Some(&args)); }
    let (draft, mixer_load) = raster_stroke(session, &args, op)?;
    if let Some(load) = mixer_load { session.mixer_load = load; }
    let Some((document, id)) = draft else { return Ok(json!({"changes": []})); };
    super::document_change::commit(session, STROKE, document, vec![id], json!({"id": id.0}))
}

type RasterDraft = Option<(Document, crate::ObjectId)>;

fn raster_stroke(session: &Session, args: &StrokeArgs, op: BrushOp) -> Result<(RasterDraft, Option<MixerLoad>), CommandError> {
    if op == BrushOp::Clone && args.clone_source.is_none() {
        return Err(CommandError::invalid_params());
    }
    let settings = settings(session, op);
    let options = &session.tool_options;
    let tool = &session.active_tool;
    let params = OpParams {
        tonal_range: match options.choice(tool, "brush.tonal_range").as_deref() {
            Some("shadows") => TonalRange::Shadows,
            Some("highlights") => TonalRange::Highlights,
            _ => TonalRange::Midtones,
        },
        protect_hue: options.flag(tool, "brush.protect_hue").unwrap_or(false),
        sponge_mode: match options.choice(tool, "brush.sponge_mode").as_deref() {
            Some("desaturate") => SpongeMode::Desaturate,
            Some("vibrance") => SpongeMode::Vibrance,
            _ => SpongeMode::Saturate,
        },
        sharpen_mode: match options.choice(tool, "brush.sharpen_mode").as_deref() {
            Some("unsharp") => SharpenMode::UnsharpMask,
            Some("harsh") => SharpenMode::Harsh,
            _ => SharpenMode::Clarity,
        },
        pigment: options.choice(tool, "brush.model").as_deref() == Some("ryb"),
        strength: (options.number(tool, "brush.strength").unwrap_or(90.0) / 100.0) as f32,
        tolerance: (options.number(tool, "fill.tolerance").unwrap_or(10.0) / 100.0) as f32,
        contiguous: options.flag(tool, "fill.contiguous").unwrap_or(true),
        sample_continuously: options.flag(tool, "brush.sample_continuously").unwrap_or(false),
        ..Default::default()
    };
    let mut mixer = (op == BrushOp::Mixer).then(|| {
        let auto_load = options.flag(tool, "brush.auto_load").unwrap_or(false);
        let auto_clean = options.flag(tool, "brush.auto_clean").unwrap_or(false);
        let colour = if auto_load || (!auto_clean && session.mixer_load == MixerLoad::Fresh) {
            Some(settings.colour).filter(|colour| colour[3] != 0)
        } else if auto_clean { None } else { session.mixer_load.colour() };
        let pigment = options.choice(tool, "brush.model").as_deref() == Some("ryb");
        (colour, pigment)
    });
    let add = (matches!(op, BrushOp::Paint | BrushOp::Pixel) && !settings.protect_alpha)
        || mixer.is_some_and(|(colour, _)| colour.is_some() && settings.flow > 0.0 && params.strength > 0.0);
    let Some((object, mut pixels, add_layer)) = edit_layer(session, None, Some(args.points[0]), add)? else {
        return Ok((None, None));
    };
    let mut document = session.document.clone();
    let changed = paint(&document, &object, &mut pixels, args, op, &settings, &params, mixer.as_mut())?;
    let mixer_load = mixer.map(|(colour, _)| colour.map_or(MixerLoad::Clean, MixerLoad::Colour));
    if !changed { return Ok((None, mixer_load)); }
    let id = install_pixels(&mut document, &session.selection.ids, object, pixels, add_layer);
    Ok((Some((document, id)), mixer_load))
}

pub fn preview(session: &Session, command: &Command) -> Option<Document> {
    if !session.has_document || command.name != STROKE { return None; }
    let op = operation(&session.active_tool)?;
    let args: StrokeArgs = serde_json::from_value(command.params.clone()).ok()?;
    if !args.valid() { return None; }
    if session.quick_mask.active {
        if !supports_quick_mask(op) { return None; }
        let mut document = session.document.clone();
        document.pixel_selection = Some(quick_mask_draft(session, &args, op).ok()??);
        return Some(document);
    }
    if op == BrushOp::Tone {
        return tone_draft(session, Some(&args)).ok()?.map(|(document, _, _)| document);
    }
    raster_stroke(session, &args, op).ok()?.0.map(|(document, _)| document)
}

fn tone_source(session: &Session) -> Result<&Object, CommandError> {
    let document = &session.document;
    let selected = session.selection.ids.last().and_then(|id| document.get_object(*id))
        .ok_or_else(CommandError::invalid_params)?;
    let source = if selected.tone_layer.is_some() {
        selected.parent_id.and_then(|id| document.get_object(id)).ok_or_else(CommandError::invalid_params)?
    } else {
        selected
    };
    if !editable(document, selected) || !editable(document, source) {
        return Err(CommandError::new("LOCKED"));
    }
    if source.tone_layer.is_some() || crate::raster_source::dimensions(document, source).is_none() {
        return Err(CommandError::invalid_params());
    }
    Ok(source)
}

fn tone_brush(session: &mut Session, stroke: Option<&StrokeArgs>) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    if session.active_tool != TONE_TOOL || session.quick_mask.active { return Err(CommandError::invalid_params()); }
    let Some((document, id, source)) = tone_draft(session, stroke)? else {
        return Ok(json!({"changes": []}));
    };
    let command = if stroke.is_some() { STROKE } else { TONE_ADD };
    super::document_change::commit(session, command, document, vec![id], json!({"id": id.0, "source": source.0}))
}

fn tone_draft(session: &Session, stroke: Option<&StrokeArgs>) -> Result<Option<(Document, crate::ObjectId, crate::ObjectId)>, CommandError> {
    use crate::blend::BlendMode;
    let source = tone_source(session)?;
    let [w, h] = crate::raster_source::dimensions(&session.document, source).ok_or_else(CommandError::invalid_params)?;
    let options = &session.tool_options;
    let mode = match options.choice(TONE_TOOL, "brush.tone_mode").as_deref() {
        Some("blend") => ToneMode::Blend,
        Some("inverse_blend") => ToneMode::InverseBlend,
        _ => ToneMode::DodgeBurn,
    };
    let range = match options.choice(TONE_TOOL, "brush.tone_range").as_deref() {
        Some("shadows") => ToneRange::Shadows,
        Some("midtones") => ToneRange::Midtones,
        Some("highlights") => ToneRange::Highlights,
        _ => ToneRange::Full,
    };
    let blend_mode = match options.choice(TONE_TOOL, "brush.tone_blend").as_deref() {
        Some("soft_light") => BlendMode::SoftLight,
        Some("hard_light") => BlendMode::HardLight,
        Some("multiply") => BlendMode::Multiply,
        Some("screen") => BlendMode::Screen,
        _ => BlendMode::Overlay,
    };
    let spec = ToneLayer { mode, range };
    let mut document = session.document.clone();
    let compatible = |object: &&Object| object.tone_layer == Some(spec) && object.blend_mode == blend_mode
        && object.parent_id == Some(source.id) && editable(&document, object)
        && document.pixel_layers.get(object.id).is_some();
    let layer = stroke.and_then(|_| {
        let active = session.selection.ids.last().and_then(|id| document.get_object(*id));
        if active.is_some_and(|object| object.tone_layer.is_some()) {
            active.filter(compatible)
        } else {
            document.objects.iter().rev().find(|object| object.parent_id == Some(source.id) && object.tone_layer.is_some())
                .filter(compatible)
        }
    });
    let created = layer.is_none();
    let (object, mut pixels) = if let Some(layer) = layer {
        (layer.clone(), document.pixel_layers.get(layer.id).unwrap().clone())
    } else {
        let name = match mode {
            ToneMode::DodgeBurn => "tone.dodge_burn_layer",
            ToneMode::Blend => "tone.blend_layer",
            ToneMode::InverseBlend => "tone.inverse_blend_layer",
        };
        (Object {
            name: dtool_strings::text(name).to_owned(), kind: crate::ObjectKind::Image,
            pos: source.pos, size: source.size, parent_id: Some(source.id), page_id: source.page_id,
            rotation_deg: source.rotation_deg, shear_deg: source.shear_deg, transform_origin: source.transform_origin,
            tone_layer: Some(spec), blend_mode, raster_size: Some((w as u32, h as u32)), ..Object::default()
        }, PixelLayer::new(w as u32, h as u32))
    };
    if let Some(stroke) = stroke {
        let mut settings = settings(session, BrushOp::Tone);
        settings.colour = match mode {
            ToneMode::DodgeBurn if stroke.invert => [0, 0, 0, 255],
            ToneMode::DodgeBurn => [255; 4],
            ToneMode::Blend | ToneMode::InverseBlend => {
                let [mut r, mut g, mut b, a] = settings.colour;
                if stroke.invert ^ (mode == ToneMode::InverseBlend) {
                    [r, g, b] = [255 - r, 255 - g, 255 - b];
                }
                [r, g, b, a]
            }
        };
        if !paint(&document, &object, &mut pixels, stroke, BrushOp::Paint, &settings, &OpParams::default(), None)? {
            return Ok(None);
        }
    }
    let id = if created { document.create_object(object) } else { object.id };
    document.pixel_layers.insert(id, pixels);
    Ok(Some((document, id, source.id)))
}

pub(super) fn edit_layer(
    session: &Session, target: Option<crate::ObjectId>, point: Option<[f32; 2]>, add: bool,
) -> Result<Option<(Object, PixelLayer, bool)>, CommandError> {
    use crate::artboard::edit;
    let document = &session.document;
    let target = if let Some(id) = target {
        Some(document.get_object(id).filter(|object| crate::raster_source::dimensions(document, object).is_some())
            .ok_or_else(CommandError::invalid_params)?)
    } else {
        selected_raster(session)
    };
    if let Some(object) = target {
        if !editable(document, object) { return Err(CommandError::new("LOCKED")); }
        let pixels = document.pixel_layers.get(object.id).cloned().or_else(|| {
            let ([w, h], rgba) = crate::scene::raster_rgba(object)?;
            Some(PixelLayer::from_rgba(w as u32, h as u32, &rgba))
        }).ok_or_else(CommandError::invalid_params)?;
        return Ok(Some((object.clone(), pixels, false)));
    }
    let Some(point) = point.filter(|_| add) else { return Ok(None); };
    let board = edit::at_point(document, point);
    if board.is_some_and(|board| !editable(document, board)) { return Err(CommandError::new("LOCKED")); }
    let page = board.map(|board| crate::data_merge::layout::object_page_index(document, board.id))
        .or_else(|| edit::page_at_point(document, point));
    let Some(page) = page else { return Ok(None); };
    let rect = board.map(crate::geometry::object_world_bounds)
        .or_else(|| super::layer_kinds::page_rect(document, page)).ok_or_else(CommandError::invalid_params)?;
    let size = super::layer_kinds::pixel_dimensions(document, [rect[2], rect[3]])?;
    let mut object = super::layer_kinds::pixel_object(document, page, size);
    object.pos = [rect[0], rect[1]];
    object.size = [rect[2], rect[3]];
    edit::assign_new_object(document, &mut object, point);
    Ok(Some((object, PixelLayer::new(size[0], size[1]), true)))
}

fn selected_raster(session: &Session) -> Option<&Object> {
    let document = &session.document;
    session.selection.ids.iter().rev().find_map(|id| {
        if document.placed_images.iter().any(|image| image.object_id == *id) { return None; }
        document.get_object(*id).filter(|object| crate::raster_source::dimensions(document, object).is_some())
    })
}

/// The pixel layer on which a clone source can be placed or painted.
pub fn clone_layer(session: &Session) -> Option<crate::ObjectId> {
    (!session.quick_mask.active).then(|| selected_raster(session)).flatten()
        .filter(|object| editable(&session.document, object)).map(|object| object.id)
}

/// Three corners of the active symmetry surface in document coordinates:
/// its origin and the ends of its local X and Y axes.
pub fn symmetry_frame(session: &Session, viewport: [f32; 2]) -> Option<[[f32; 2]; 3]> {
    use crate::view::ViewTarget;
    let document = &session.document;
    if !session.has_document { return None; }
    let target = ViewTarget::at_view_center(document, &session.view, viewport);
    let [x, y, w, h] = if session.quick_mask.active {
        let surface = super::quickmask::surface(session, None)?;
        let [w, h] = surface.size.map(|side| side as f32 / surface.scale);
        [surface.bounds[0], surface.bounds[1], w, h]
    } else if let Some(object) = selected_raster(session) {
        let [x, y] = object.pos;
        let [w, h] = object.size.map(f32::abs);
        let matrix = object_matrix(object);
        return Some([[x, y], [x + w, y], [x, y + h]].map(|point| apply_matrix(&matrix, point)));
    } else if let Some(ViewTarget::Artboard(id)) = target {
        crate::geometry::object_world_bounds(document.get_object(id)?)
    } else {
        let page = match target { Some(ViewTarget::Page(id)) => Some(id), _ => None };
        crate::selection::PixelSurface::for_page(document, page)?.bounds
    };
    Some([[x, y], [x + w, y], [x, y + h]])
}

pub(super) fn install_pixels(
    document: &mut Document, selected: &[crate::ObjectId], object: Object, pixels: PixelLayer, add_layer: bool,
) -> crate::ObjectId {
    let id = if add_layer {
        super::layer_kinds::insert_above_selection(document, selected, object)
    } else {
        let id = object.id;
        if let Some(layer) = document.find_object_mut(id.0) {
            layer.raster_size = Some((pixels.width, pixels.height));
            layer.raster_data = None;
        }
        id
    };
    document.pixel_layers.insert(id, pixels);
    id
}

fn paint(
    document: &Document, object: &Object, pixels: &mut PixelLayer, stroke: &StrokeArgs,
    op: BrushOp, settings: &BrushSettings, params: &OpParams, mixer: Option<&mut (Option<[u8; 4]>, bool)>,
) -> Result<bool, CommandError> {
    let matrix = object_matrix(object);
    let inverse = invert_matrix(&matrix).ok_or_else(CommandError::invalid_params)?;
    let size = object.size.map(f32::abs);
    if size.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(CommandError::invalid_params());
    }
    let scale = [pixels.width as f32 / size[0], pixels.height as f32 / size[1]];
    let to_raster = |point| {
        let [x, y] = apply_matrix(&inverse, point);
        [(x - object.pos[0]) * scale[0], (y - object.pos[1]) * scale[1]]
    };
    let points = stroke.samples(to_raster);
    let clone_offset = if op == BrushOp::Clone {
        let source = to_raster(stroke.clone_source.ok_or_else(CommandError::invalid_params)?);
        let offset = [source[0].floor() - points[0].x.floor(), source[1].floor() - points[0].y.floor()];
        if !offset.iter().all(|v| v.is_finite()) { return Err(CommandError::invalid_params()); }
        Some(offset.map(|v| v.round() as i64))
    } else {
        None
    };
    let size = [pixels.width, pixels.height];
    let filtering = brush::is_filter(op);
    let mut paths = symmetric_paths(points, size, settings);
    let padding = if filtering { brush::filter_padding(op, settings, params) } else { 0.0 };
    let Some(rect) = stroke_region(&mut paths, settings, size, padding)? else {
        return Ok(false);
    };
    let [x0, y0, w, h] = rect;
    let before = pixels.read_region(rect);
    let mut after = before.clone();
    let mut raster = RasterViewMut::new(w, h, &mut after);
    if let Some(offset) = clone_offset {
        let dabs = brush::dabs(&paths[0].points, settings);
        brush::clone_dabs(&mut raster, pixels, &dabs, settings, [x0, y0], offset);
    } else if let Some((colour, pigment)) = mixer {
        brush::mixer_stroke(&mut raster, &paths[0].points, settings, params.strength, colour, *pigment, [x0, y0]);
    } else if filtering {
        let dabs = paths.iter().map(|path| brush::dabs(&path.points, &path.settings(settings)));
        brush::filter_dabs(&mut raster, dabs, settings, params, op);
    } else {
        for path in &paths {
            brush::stroke_at(&mut raster, &path.points, &path.settings(settings), op, params, [x0, y0]);
        }
    }

    if let Some(selection) = &document.pixel_selection {
        let surface = crate::selection::PixelSurface::for_page(document, selection.page_id);
        for (i, (old, new)) in before.chunks_exact(4).zip(after.chunks_exact_mut(4)).enumerate() {
            if old == new { continue; }
            let position = apply_matrix(&matrix, [
                object.pos[0] + (x0 as f32 + (i % w as usize) as f32 + 0.5) / scale[0],
                object.pos[1] + (y0 as f32 + (i / w as usize) as f32 + 0.5) / scale[1],
            ]);
            let alpha = surface.map_or(0, |surface| surface.coverage_at(selection, position));
            if alpha == 0 {
                new.copy_from_slice(old);
            } else if alpha < 255 {
                let ratio = f32::from(alpha) / 255.0;
                if op == BrushOp::ColourReplace {
                    let [hue, saturation, _] = crate::colour::rgb8_to_hsl(new);
                    new[..3].copy_from_slice(&crate::colour::mix_chroma(old, hue, saturation, ratio));
                    continue;
                }
                let a = f32::from(old[3]) * (1.0 - ratio);
                let b = f32::from(new[3]) * ratio;
                let opacity = a + b;
                if opacity > 0.0 {
                    for (old, new) in old[..3].iter().zip(&mut new[..3]) {
                        *new = ((f32::from(*old) * a + f32::from(*new) * b) / opacity).round() as u8;
                    }
                }
                new[3] = opacity.round() as u8;
            }
        }
    }
    if before == after { return Ok(false); }
    pixels.set_region(rect, &after);
    pixels.compact();
    Ok(true)
}

struct StrokePath {
    points: Vec<StrokePoint>,
    tip_transform: TipTransform,
}

impl StrokePath {
    fn settings<'a>(&self, base: &'a BrushSettings) -> Cow<'a, BrushSettings> {
        if self.tip_transform == base.tip_transform {
            Cow::Borrowed(base)
        } else {
            Cow::Owned(BrushSettings { tip_transform: self.tip_transform, ..base.clone() })
        }
    }
}

fn symmetric_paths(points: Vec<StrokePoint>, size: [u32; 2], settings: &BrushSettings) -> Vec<StrokePath> {
    let [cx, cy] = std::array::from_fn(|axis| size[axis] as f32 * settings.symmetry_origin[axis]);
    let mut paths = vec![StrokePath { points, tip_transform: TipTransform::default() }];
    for copy in 0..settings.symmetry {
        let angle = std::f32::consts::TAU * copy as f32 / settings.symmetry as f32;
        let (sin, cos) = if (4 * copy).is_multiple_of(settings.symmetry) {
            [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)][(4 * copy / settings.symmetry) as usize]
        } else {
            angle.sin_cos()
        };
        for reflect in [false, true].into_iter().take(1 + usize::from(settings.mirror)) {
            if copy == 0 && !reflect { continue; }
            let tip_transform = TipTransform { radians: angle, reflect };
            let sign = if reflect { -1.0 } else { 1.0 };
            let points = paths[0].points.iter().map(|point| {
                let x = (point.x - cx) * sign;
                let y = point.y - cy;
                let [tx, ty] = point.tilt;
                StrokePoint { x: cx + x * cos - y * sin, y: cy + x * sin + y * cos,
                    tilt: [tx * sign * cos - ty * sin, tx * sign * sin + ty * cos], ..*point }
            }).collect::<Vec<_>>();
            if !paths.iter().any(|other| {
                settings.equivalent_tip_transform(other.tip_transform, tip_transform)
                    && other.points.iter().zip(&points)
                        .all(|(a, b)| (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001)
            }) {
                paths.push(StrokePath { points, tip_transform });
            }
        }
    }
    paths
}

fn stroke_region(
    paths: &mut [StrokePath], settings: &BrushSettings, [width, height]: [u32; 2], padding: f32,
) -> Result<Option<[u32; 4]>, CommandError> {
    let radius = paths.iter().map(|path| path.settings(settings).max_radius())
        .fold(0.0, f32::max) + 2.0 + padding;
    let bounds = |axis: fn(&StrokePoint) -> f32, limit: u32| {
        let (lo, hi) = paths.iter().flat_map(|path| &path.points).map(axis).fold((f32::INFINITY, f32::NEG_INFINITY),
            |(lo, hi), value| (lo.min(value), hi.max(value)));
        ((lo - radius).floor().clamp(0.0, limit as f32) as u32,
         (hi + radius).ceil().clamp(0.0, limit as f32) as u32)
    };
    let (x0, x1) = bounds(|p| p.x, width);
    let (y0, y1) = bounds(|p| p.y, height);
    let (w, h) = (x1 - x0, y1 - y0);
    if w == 0 || h == 0 { return Ok(None); }
    if u64::from(w) * u64::from(h) > 32_000_000 { return Err(CommandError::invalid_params()); }
    for point in paths.iter_mut().flat_map(|path| &mut path.points) {
        point.x = (point.x - x0 as f32).clamp(-radius, w as f32 + radius);
        point.y = (point.y - y0 as f32).clamp(-radius, h as f32 + radius);
    }
    Ok(Some([x0, y0, w, h]))
}

fn paint_quick_mask(
    session: &mut Session, command: &Command, stroke: &StrokeArgs, op: BrushOp,
) -> CommandResult {
    let Some(mask) = quick_mask_draft(session, stroke, op)? else { return Ok(json!({"changes": []})); };
    let before = session.document.pixel_selection.replace(mask);
    Ok(super::pixel_selection::record_mask(session, command, before))
}

fn quick_mask_draft(
    session: &Session, stroke: &StrokeArgs, op: BrushOp,
) -> Result<Option<crate::selection::PixelSelection>, CommandError> {
    use crate::selection::PixelSelection;
    let Some(surface) = super::quickmask::surface(session, stroke.points.first().copied()) else {
        return Ok(None);
    };
    let [width, height] = surface.size;
    if u64::from(width) * u64::from(height) > 100_000_000 {
        return Err(CommandError::invalid_params());
    }
    let points = stroke.samples(|point| surface.doc_to_pixel(point));
    let mut settings = settings(session, op);
    settings.colour = [255; 4];
    settings.protect_alpha = false;
    settings.stamp_colour = false;
    let mut paths = symmetric_paths(points, [width, height], &settings);
    let Some([x0, y0, w, h]) = stroke_region(&mut paths, &settings, [width, height], 0.0)? else {
        return Ok(None);
    };
    let mut stencil = vec![0; w as usize * h as usize * 4];
    let mut raster = RasterViewMut::new(w, h, &mut stencil);
    let stamp = if op == BrushOp::Erase { BrushOp::Paint } else { op };
    for path in &paths {
        brush::stroke_at(&mut raster, &path.points, &path.settings(&settings), stamp, &OpParams::default(), [x0, y0]);
    }

    let before = &session.document.pixel_selection;
    if before.as_ref().is_some_and(|mask| {
        mask.width != width || mask.height != height || mask.coverage.len() != width as usize * height as usize
    }) {
        return Err(CommandError::invalid_params());
    }
    let ink = u32::from(if op == BrushOp::Erase { 0 } else { session.quick_mask.ink });
    let mut edited = None;
    for (i, pixel) in stencil.chunks_exact(4).enumerate().filter(|(_, pixel)| pixel[3] != 0) {
        let x = x0 + i as u32 % w;
        let y = y0 + i as u32 / w;
        let old = u32::from(before.as_ref().map_or(0, |mask| mask.get(x, y)));
        let strength = u32::from(pixel[3]);
        let value = ((old * (255 - strength) + ink * strength + 127) / 255) as u8;
        if u32::from(value) != old {
            let mask = edited.get_or_insert_with(|| before.clone().unwrap_or_else(|| {
                let mut mask = PixelSelection::empty(width, height);
                mask.page_id = Some(surface.page_id);
                mask
            }));
            mask.coverage[(y * width + x) as usize] = value;
        }
    }
    Ok(edited)
}
