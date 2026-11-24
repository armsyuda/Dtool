use crate::command::Session;
use crate::document::Document;
use crate::object::ObjectId;
use crate::paint::{Colour, Paint};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const COLOR_PICK: &str = "color.pick";
pub const COLOR_CONTRAST: &str = "color.contrast";
pub const COLOR_RECENT_LIST: &str = "color.recent.list";
pub const UI_COLOR_TARGET_SET: &str = "ui.color.target.set";
pub const UI_COLOR_TARGET_QUERY: &str = "ui.color.target.query";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode {
    Point,
    Average3,
    Average5,
    Merged,
}

impl PickMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "point" => Some(Self::Point),
            "average3" => Some(Self::Average3),
            "average5" => Some(Self::Average5),
            "merged" => Some(Self::Merged),
            _ => None,
        }
    }

    pub fn size(self) -> u32 {
        match self {
            Self::Point | Self::Merged => 1,
            Self::Average3 => 3,
            Self::Average5 => 5,
        }
    }
}

pub use crate::view_state::{ColourTarget, ColourTargetState as TargetState};

pub fn target_state(session: &Session) -> TargetState {
    session.view.colour_target
}

pub fn set_target_state(session: &mut Session, state: TargetState) {
    session.view.colour_target = state;
}

fn target_json(state: &TargetState) -> Value {
    json!({
        "target": state.target.as_str(),
        "story": state.story,
        "range": state.range.map(|(start, end)| json!({ "start": start, "end": end })),
    })
}

pub(super) fn layer_document(doc: &Document, layer: ObjectId) -> Option<Document> {
    doc.get_object(layer)?;
    let mut keep: HashSet<ObjectId> = HashSet::from([layer]);
    loop {
        let before = keep.len();
        for object in &doc.objects {
            if object.parent_id.is_some_and(|parent| keep.contains(&parent)) {
                keep.insert(object.id);
            }
        }
        if keep.len() == before {
            break;
        }
    }
    let mut filtered = doc.clone();
    filtered.objects.retain(|object| keep.contains(&object.id));
    Some(filtered)
}

pub fn average(pixels: &[u8]) -> Option<Colour> {
    let count = pixels.len() / 4;
    if count == 0 {
        return None;
    }
    let mut weight = 0.0f64;
    let mut sums = [0.0f64; 3];
    for pixel in pixels.chunks_exact(4) {
        let alpha = f64::from(pixel[3]);
        weight += alpha;
        for channel in 0..3 {
            sums[channel] += f64::from(pixel[channel]) * alpha;
        }
    }
    if weight <= 0.0 {
        return Some(Colour::new(0, 0, 0, 0));
    }
    let channel = |index: usize| (sums[index] / weight).round().clamp(0.0, 255.0) as u8;
    let alpha = (weight / count as f64).round().clamp(0.0, 255.0) as u8;
    Some(Colour::new(channel(0), channel(1), channel(2), alpha))
}

pub fn sample(doc: &Document, x: f32, y: f32, mode: PickMode, layer: Option<ObjectId>, zoom: f32) -> Option<Colour> {
    sample_live(doc, x, y, mode, layer, zoom, &[])
}

pub fn sample_live(doc: &Document, x: f32, y: f32, mode: PickMode, layer: Option<ObjectId>, zoom: f32, staged: &[crate::commands::layer_style::StagedEffect]) -> Option<Colour> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let filtered = match (mode, layer) {
        (PickMode::Merged, _) | (_, None) => None,
        (_, Some(id)) => Some(layer_document(doc, id)?),
    };
    let source = filtered.as_ref().unwrap_or(doc);
    let size = mode.size();
    let scale = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    let extent = size as f32 / scale;
    let half = extent / 2.0;
    let pixels = crate::layer_style::raster::render_region(source, &crate::view_state::CanvasFlags::default(), staged, [x - half, y - half, extent, extent], size, size);
    average(&pixels)
}

fn hex(colour: &Colour) -> String {
    if colour.a < 255 {
        format!("#{:02x}{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b, colour.a)
    } else {
        format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
    }
}

pub fn colour_json(colour: &Colour) -> Value {
    json!({ "hex": hex(colour), "rgba": [colour.r, colour.g, colour.b, colour.a] })
}

fn number(params: &Value, key: &str) -> Result<f32, CommandError> {
    params.get(key).and_then(Value::as_f64).map(|value| value as f32).filter(|value| value.is_finite()).ok_or_else(CommandError::invalid_params)
}

fn range_param(params: &Value) -> Result<Option<(usize, usize)>, CommandError> {
    match params.get("range") {
        None | Some(Value::Null) => Ok(None),
        Some(range) => {
            let start = range.get("start").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)? as usize;
            let end = range.get("end").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)? as usize;
            if start >= end {
                return Err(CommandError::invalid_params());
            }
            Ok(Some((start, end)))
        }
    }
}

fn state_from(params: &Value, base: TargetState) -> Result<TargetState, CommandError> {
    let target = match params.get("target") {
        None | Some(Value::Null) => base.target,
        Some(value) => value.as_str().and_then(ColourTarget::parse).ok_or_else(CommandError::invalid_params)?,
    };
    let story = match params.get("story") {
        None => base.story,
        Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or_else(CommandError::invalid_params)?),
    };
    let range = if params.get("range").is_some() { range_param(params)? } else { base.range };
    Ok(TargetState { target, story, range })
}

fn outline_width(session: &Session, story: u64, start: usize, end: usize) -> f32 {
    session.document.stories.iter().find(|item| item.id.0 == story).and_then(|item| {
        item.ranges.iter().find_map(|range| {
            let from = range.get("start").and_then(Value::as_u64)? as usize;
            let to = range.get("end").and_then(Value::as_u64)? as usize;
            if from >= end || to <= start {
                return None;
            }
            range.get("outline").and_then(Value::as_array).and_then(|pair| pair.get(1)).and_then(Value::as_f64).map(|width| width as f32)
        })
    }).unwrap_or(1.0)
}

fn write(session: &mut Session, state: &TargetState, colour: Colour) -> Result<bool, CommandError> {
    let paint = Paint::Solid { colour };
    let paint_value = crate::paint::paint_to_json_value(&paint);
    match state.target {
        ColourTarget::Fill | ColourTarget::Stroke => {
            let ids: Vec<ObjectId> = session.selection.ids.iter().copied().collect();
            if ids.is_empty() {
                return Ok(false);
            }
            let mut updates = Vec::new();
            for id in ids {
                let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
                let params = if state.target == ColourTarget::Fill {
                    json!({ "id": id.0, "fill": paint_value })
                } else {
                    let mut stroke = serde_json::to_value(&object.stroke).map_err(|_| CommandError::new("INTERNAL_ERROR"))?;
                    stroke["paint"] = paint_value.clone();
                    json!({ "id": id.0, "stroke": stroke })
                };
                updates.push(Command::with_params("object.update", params));
            }
            let command = if updates.len() == 1 {
                updates.remove(0)
            } else {
                Command::with_params("edit.batch", json!({ "commands": updates }))
            };
            session.run(&command)?;
            Ok(true)
        }
        ColourTarget::TextFill | ColourTarget::TextStroke => {
            let story = state.story.ok_or_else(CommandError::invalid_params)?;
            let (start, end) = state.range.ok_or_else(CommandError::invalid_params)?;
            let patch = if state.target == ColourTarget::TextFill {
                json!({ "fill": paint_value })
            } else {
                json!({ "outline": [hex(&colour), outline_width(session, story, start, end)] })
            };
            session.run(&Command::with_params("text.style", json!({ "story": story, "range": { "start": start, "end": end }, "patch": patch })))?;
            Ok(true)
        }
    }
}

fn pick(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let x = number(params, "x")?;
    let y = number(params, "y")?;
    let mode = match params.get("mode") {
        None | Some(Value::Null) => PickMode::Point,
        Some(value) => value.as_str().and_then(PickMode::parse).ok_or_else(CommandError::invalid_params)?,
    };
    let layer = match params.get("layer") {
        None | Some(Value::Null) => None,
        Some(value) => Some(ObjectId(value.as_u64().ok_or_else(CommandError::invalid_params)?)),
    };
    let zoom = match params.get("zoom") {
        None | Some(Value::Null) => session.view.zoom,
        Some(value) => value.as_f64().map(|value| value as f32).filter(|value| value.is_finite() && *value > 0.0).ok_or_else(CommandError::invalid_params)?,
    };
    let state = state_from(params, target_state(session))?;
    let staged: Vec<_> = session.document.objects.iter().flat_map(|object| crate::commands::layer_style::staged_effects(session, object.id)).collect();
    let colour = sample_live(&session.document, x, y, mode, layer, zoom, &staged).ok_or_else(CommandError::not_found)?;
    let wants_apply = params.get("apply").and_then(Value::as_bool).unwrap_or(true);
    let applied = wants_apply && write(session, &state, colour)?;
    if applied {
        crate::palette::push_recent(colour)?;
    }
    Ok(json!({ "colour": colour_json(&colour), "model": "rgb", "target": target_json(&state), "applied": applied, "doc_version": session.generation }))
}

fn contrast(params: &Value) -> CommandResult {
    let a = crate::commands::swatches::parse_colour(params.get("a").ok_or_else(CommandError::invalid_params)?)?;
    let b = crate::commands::swatches::parse_colour(params.get("b").ok_or_else(CommandError::invalid_params)?)?;
    let services = crate::colour::services().read().map_err(|_| CommandError::new("INTERNAL_ERROR"))?;
    let ratio = crate::colour::convert::contrast_ratio(&a.managed(), &b.managed(), &services.profiles);
    Ok(json!({ "ratio": ratio, "aa_normal": ratio >= 4.5, "aa_large": ratio >= 3.0 }))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        COLOR_PICK => Some(session.has_document),
        COLOR_CONTRAST | COLOR_RECENT_LIST | UI_COLOR_TARGET_SET | UI_COLOR_TARGET_QUERY => Some(true),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        COLOR_PICK => Some(pick(session, &command.params)),
        COLOR_CONTRAST => Some(contrast(&command.params)),
        COLOR_RECENT_LIST => Some(Ok(json!({ "recent": crate::palette::recent_colours().iter().map(colour_json).collect::<Vec<_>>() }))),
        UI_COLOR_TARGET_SET => Some((|| {
            let target = command.params.get("target").and_then(Value::as_str).and_then(ColourTarget::parse).ok_or_else(CommandError::invalid_params)?;
            let state = state_from(&command.params, TargetState { target, story: None, range: None })?;
            if state.target.is_text() && (state.story.is_none() || state.range.is_none()) {
                return Err(CommandError::invalid_params());
            }
            set_target_state(session, state);
            Ok(target_json(&state))
        })()),
        UI_COLOR_TARGET_QUERY => Some(Ok(target_json(&target_state(session)))),
        _ => None,
    }
}
