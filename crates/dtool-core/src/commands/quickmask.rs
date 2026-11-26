use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::command::Session;
use crate::paint::{gradient_parameter, GradientGeom, GradientKind};
use crate::selection::{PixelSelection, PixelSurface};

pub const TOGGLE: &str = "selection.quickmask.toggle";
pub const GRADIENT: &str = "selection.quickmask.gradient";

pub(super) fn surface(session: &Session, point: Option<[f32; 2]>) -> Option<PixelSurface> {
    let doc = &session.document;
    let pointed = point.and_then(|point| PixelSurface::at_point(doc, point));
    if let Some(mask) = &doc.pixel_selection {
        let active = PixelSurface::for_page(doc, mask.page_id)?;
        pointed.is_none_or(|page| page.page_id == active.page_id).then_some(active)
    } else if point.is_some() {
        pointed
    } else {
        PixelSurface::for_page(doc, None)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    #[default]
    Overlay,
    Black,
    White,
    Transparent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub active: bool,
    pub display: Display,
    pub ink: u8,
}

impl Default for State {
    fn default() -> Self {
        Self { active: false, display: Display::Overlay, ink: 255 }
    }
}

#[derive(Default, Deserialize)]
struct Args {
    on: Option<bool>,
    display: Option<Display>,
    ink: Option<u8>,
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    match cmd.name.as_str() {
        TOGGLE => Some(session.has_document),
        GRADIENT => Some(session.has_document && session.quick_mask.active),
        _ => None,
    }
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    if cmd.name == GRADIENT {
        return Some(apply_gradient(session, cmd, true));
    }
    (cmd.name == TOGGLE).then(|| {
        update(session, cmd)?;
        let state = session.quick_mask;
        Ok(json!({
            "on": state.active,
            "display": state.display,
            "ink": state.ink,
            "doc_version": session.generation,
            "changes": [],
        }))
    })
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<Result<(), CommandError>> {
    match cmd.name.as_str() {
        TOGGLE => Some(update(session, cmd)),
        GRADIENT => Some(apply_gradient(session, cmd, false).map(|_| ())),
        _ => None,
    }
}

#[derive(Deserialize)]
struct GradientArgs {
    kind: Option<GradientKind>,
    start: [f32; 2],
    end: [f32; 2],
    ratio: Option<f32>,
    from: Option<u8>,
    to: Option<u8>,
}

fn apply_gradient(session: &mut Session, cmd: &Command, record: bool) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    if !session.quick_mask.active {
        return Err(CommandError::invalid_params());
    }
    let args: GradientArgs = serde_json::from_value(cmd.params.clone()).map_err(|_| CommandError::invalid_params())?;
    let ratio = args.ratio.unwrap_or(1.0);
    if args.start == args.end || !args.start.into_iter().chain(args.end).all(f32::is_finite)
        || !ratio.is_finite() || ratio <= 0.0
    {
        return Err(CommandError::invalid_params());
    }
    let Some(surface) = surface(session, Some(args.start)) else { return Ok(json!({"changes": []})); };
    let [width, height] = surface.size;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 32 * 1024 * 1024 {
        return Err(CommandError::validation("PIXEL_SELECTION_TOO_LARGE"));
    }
    if session.document.pixel_selection.as_ref().is_some_and(|mask| mask.width != width || mask.height != height) {
        return Err(CommandError::invalid_params());
    }
    let in_pixels = |point: [f32; 2]| surface.doc_to_pixel(point).map(f64::from);
    let geom = GradientGeom { start: in_pixels(args.start), end: in_pixels(args.end), ratio: f64::from(ratio), ..GradientGeom::default() };
    let kind = args.kind.unwrap_or(GradientKind::Linear);
    let from = f32::from(args.from.unwrap_or(session.quick_mask.ink));
    let to = f32::from(args.to.unwrap_or(0));
    let mut mask = PixelSelection::empty(width, height);
    mask.page_id = session.document.pixel_selection.as_ref().map_or(Some(surface.page_id), |mask| mask.page_id);
    for (y, row) in mask.coverage.chunks_exact_mut(width as usize).enumerate() {
        for (x, value) in row.iter_mut().enumerate() {
            let t = gradient_parameter(kind, x as f64 + 0.5, y as f64 + 0.5, &geom).clamp(0.0, 1.0);
            *value = (from + (to - from) * t).round() as u8;
        }
    }
    let result = (!mask.is_empty()).then_some(mask);
    if session.document.pixel_selection == result {
        return Ok(json!({"changes": []}));
    }
    let before = std::mem::replace(&mut session.document.pixel_selection, result);
    if record {
        Ok(super::pixel_selection::record_mask(session, cmd, before))
    } else {
        Ok(json!({"changes": []}))
    }
}

fn update(session: &mut Session, cmd: &Command) -> Result<(), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let args: Args = if cmd.params.is_null() { Args::default() } else {
        serde_json::from_value(cmd.params.clone()).map_err(|_| CommandError::invalid_params())?
    };
    let state = &mut session.quick_mask;
    if let Some(on) = args.on {
        state.active = on;
    } else if args.display.is_none() && args.ink.is_none() {
        state.active = !state.active;
    }
    if let Some(display) = args.display {
        state.display = display;
    }
    if let Some(ink) = args.ink {
        state.ink = ink;
    }
    Ok(())
}
