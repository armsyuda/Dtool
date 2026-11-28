use super::{Area, Options, Prepared, can_patch};
use crate::geometry::apply_matrix;
use crate::selection::PixelSelection;
use crate::{Command, CommandError, CommandResult, Session};
use serde::Deserialize;
use serde_json::json;

pub const TOOL: &str = "tool.blemish_removal";
pub const APPLY: &str = "pixel.blemish";
pub const WIDTH: &str = "blemish.width";
pub const DEFAULT_WIDTH: f64 = 50.0;
pub const MAX_WIDTH: f64 = 512.0;

#[derive(Deserialize)]
struct Args {
    at: [f32; 2],
    width: Option<f64>,
}

pub fn width(session: &Session) -> f64 {
    session.tool_options.number(TOOL, WIDTH).unwrap_or(DEFAULT_WIDTH)
}

pub fn document_width(session: &Session, width_px: f64) -> Option<f32> {
    crate::units::from_px(width_px as f32, session.document.units, session.document.dpi)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == APPLY).then(|| can_patch(session))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    (command.name == APPLY).then(|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        let args: Args = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
        let width_px = args.width.unwrap_or_else(|| width(session));
        if !(1.0..=MAX_WIDTH).contains(&width_px) || args.at.iter().any(|coordinate| !coordinate.is_finite()) {
            return Err(CommandError::invalid_params());
        }
        let radius = document_width(session, width_px).ok_or_else(CommandError::invalid_params)? * 0.5;
        let [x, y] = args.at;
        let area = Area::Rect([x - radius, y - radius, radius * 2.0, radius * 2.0]);
        let mut patch = Prepared::new(session, Some(&area))?;
        let [left, top, w, h] = patch.bounds;
        let mut coverage = vec![0; w as usize * h as usize];
        for dy in 0..h {
            for dx in 0..w {
                let local = [
                    patch.object.pos[0] + (left as f32 + dx as f32 + 0.5) / patch.scale[0],
                    patch.object.pos[1] + (top as f32 + dy as f32 + 0.5) / patch.scale[1],
                ];
                let [px, py] = apply_matrix(&patch.matrix, local);
                if (px - x).hypot(py - y) <= radius && patch.pixels.get(left + dx, top + dy)[3] != 0 {
                    coverage[(dy * w + dx) as usize] = 255;
                }
            }
        }
        patch.mask = PixelSelection::from_coverage(w, h, coverage);
        let Some(offset) = (!patch.mask.is_empty()).then(|| source_offset(&patch, args.at, radius)).flatten() else {
            return Ok(json!({"changes": []}));
        };
        let options = Options { selection_is_source: false, texture_only: false, transparent: true };
        let Some(document) = patch.document(session, offset, options, false) else { return Ok(json!({"changes": []})); };
        let selection = session.selection.ids.clone();
        super::super::document_change::commit(session, APPLY, document, selection, json!({"id": patch.layer().0}))
    })
}

fn source_offset(patch: &Prepared, center: [f32; 2], radius: f32) -> Option<[i64; 2]> {
    let local = apply_matrix(&patch.inverse, center);
    let raster_center = std::array::from_fn::<_, 2, _>(|axis| {
        ((local[axis] - patch.object.pos[axis]) * patch.scale[axis]).floor() as i64
    });
    let sample = |[x, y]: [i64; 2]| {
        if (0..i64::from(patch.pixels.width)).contains(&x) && (0..i64::from(patch.pixels.height)).contains(&y) {
            patch.pixels.get(x as u32, y as u32)
        } else {
            [0; 4]
        }
    };
    let directions: Vec<_> = (0..24).map(|index| {
        let (sin, cos) = (index as f32 * std::f32::consts::TAU / 24.0).sin_cos();
        [cos, sin]
    }).collect();
    let mut perimeter = Vec::new();
    for direction in &directions {
        let delta = direction.map(|axis| axis * radius * 1.22);
        let offset = patch.offset(delta).ok()?;
        if offset == [0; 2] || perimeter.iter().any(|(point, _)| *point == offset) { continue; }
        let rgba = sample([raster_center[0] + offset[0], raster_center[1] + offset[1]]);
        perimeter.push((offset, rgba));
    }
    if perimeter.is_empty() { return None; }
    let [left, top, w, h] = patch.bounds.map(i64::from);
    let mut best = None;
    for distance in [2.4, 3.2, 4.5] {
        for direction in &directions {
            let [dx, dy] = patch.offset(direction.map(|axis| axis * radius * distance)).ok()?;
            if [dx, dy] == [0; 2] || left + dx < 0 || top + dy < 0
                || left + dx + w > i64::from(patch.pixels.width)
                || top + dy + h > i64::from(patch.pixels.height) { continue; }
            let origin = [raster_center[0] + dx, raster_center[1] + dy];
            let core = sample(origin);
            if core[3] == 0 { continue; }
            let error: f32 = perimeter.iter().map(|([x, y], target)| {
                let candidate = sample([origin[0] + x, origin[1] + y]);
                target.iter().zip(candidate).map(|(&a, b)| (f32::from(a) - f32::from(b)).powi(2)).sum::<f32>()
            }).sum::<f32>() / perimeter.len() as f32;
            let core_error = perimeter.iter().map(|(_, target)| {
                target.iter().zip(core).map(|(&a, b)| (f32::from(a) - f32::from(b)).powi(2)).sum::<f32>()
            }).sum::<f32>() / perimeter.len() as f32;
            let score = error + core_error * 0.08 + distance;
            if best.is_none_or(|(value, _)| score < value) { best = Some((score, [dx, dy])); }
        }
    }
    best.map(|(_, offset)| offset)
}
