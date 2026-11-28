use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::selection::PixelSurface;
use crate::{Command, CommandError, CommandResult, ObjectId, Session};
use serde::Deserialize;
use serde_json::json;
use std::collections::VecDeque;

pub const TOOL: &str = "tool.red_eye";
const COMMAND: &str = "pixel.red_eye";
const MAX_REGION_PIXELS: u64 = 32 * 1024 * 1024;

#[derive(Deserialize)]
struct Args {
    target: Option<String>,
    rect: [f32; 4],
    #[serde(default)]
    document_space: bool,
}

pub fn canvas_target(session: &Session) -> Option<ObjectId> {
    super::pixel_brush::clone_layer(session)
}

pub fn canvas_command(target: ObjectId, start: [f32; 2], end: [f32; 2]) -> Option<Command> {
    (start[0] != end[0] && start[1] != end[1]).then(|| Command::with_params(COMMAND, json!({
        "target": format!("node:{}", target.0),
        "rect": [start[0].min(end[0]), start[1].min(end[1]), start[0].max(end[0]), start[1].max(end[1])],
        "document_space": true,
    })))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == COMMAND).then_some(session.has_document && !session.quick_mask.active)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    (command.name == COMMAND).then(|| execute(session, command, true))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    (command.name == COMMAND).then(|| execute(session, command, false).map(|_| ()))
}

fn execute(session: &mut Session, command: &Command, record: bool) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    if session.quick_mask.active { return Err(CommandError::invalid_params()); }
    let args: Args = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
    let [left, top, right, bottom] = args.rect;
    if !args.rect.iter().all(|v| v.is_finite()) || left >= right || top >= bottom {
        return Err(CommandError::invalid_params());
    }
    let target = args.target.as_deref()
        .and_then(|target| target.strip_prefix("node:"))
        .and_then(|value| value.parse().ok())
        .map(ObjectId)
        .ok_or_else(CommandError::invalid_params)?;
    let Some((object, mut pixels, _)) = super::pixel_brush::edit_layer(session, Some(target), None, false)? else {
        return Err(CommandError::invalid_params());
    };
    let matrix = object_matrix(&object);
    let inverse = invert_matrix(&matrix).ok_or_else(CommandError::invalid_params)?;
    let size = object.size.map(f32::abs);
    if size.iter().any(|v| !v.is_finite() || *v <= 0.0) || pixels.width == 0 || pixels.height == 0 {
        return Err(CommandError::invalid_params());
    }
    let scale = [pixels.width as f32 / size[0], pixels.height as f32 / size[1]];
    let to_local = |point| {
        let [x, y] = apply_matrix(&inverse, point);
        [(x - object.pos[0]) * scale[0], (y - object.pos[1]) * scale[1]]
    };
    let to_world = |[x, y]: [f32; 2]| apply_matrix(&matrix, [object.pos[0] + x / scale[0], object.pos[1] + y / scale[1]]);
    let corners = [[left, top], [left, bottom], [right, top], [right, bottom]];
    let corners = corners.map(|point| if args.document_space { to_local(point) } else { point });
    if corners.iter().flatten().any(|v| !v.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    let limits = [pixels.width, pixels.height];
    let ranges = [0, 1].map(|axis| {
        let low = corners.iter().map(|p| p[axis]).fold(f32::INFINITY, f32::min).floor() as u32;
        let high = corners.iter().map(|p| p[axis]).fold(f32::NEG_INFINITY, f32::max).ceil() as u32;
        [low.min(limits[axis]), high.min(limits[axis])]
    });
    let [[x0, x1], [y0, y1]] = ranges;
    let [width, height] = [x1 - x0, y1 - y0];
    if width == 0 || height == 0 { return Err(CommandError::validation("NO_RED_EYE")); }

    let centre = [left * 0.5 + right * 0.5, top * 0.5 + bottom * 0.5];
    let radius = [right * 0.5 - left * 0.5, bottom * 0.5 - top * 0.5];
    let selection = session.document.pixel_selection.as_ref().map(|mask| {
        (mask, PixelSurface::for_page(&session.document, mask.page_id))
    });
    let area = u64::from(width) * u64::from(height);
    if area > MAX_REGION_PIXELS {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }

    let mut weights = vec![0; area as usize];
    let mut seed = None;
    let mut best_score = 0.0;
    for y in y0..y1 {
        let rgba = pixels.read_region([x0, y, width, 1]);
        let row_start = (y - y0) as usize * width as usize;
        for (offset, value) in rgba.chunks_exact(4).enumerate() {
            if value[3] == 0 { continue; }
            let red_weight = red_eye_weight(value[0], value[1], value[2]);
            if red_weight == 0.0 { continue; }
            let x = x0 + offset as u32;
            let local = [x as f32 + 0.5, y as f32 + 0.5];
            let world = to_world(local);
            let point = if args.document_space { world } else { local };
            let dx = (point[0] - centre[0]) / radius[0];
            let dy = (point[1] - centre[1]) / radius[1];
            let inside = (1.0 - dx * dx - dy * dy).max(0.0);
            if inside == 0.0 { continue; }
            let coverage = (inside / 0.36).min(1.0);
            let selected = selection.as_ref().map_or(255, |(mask, surface)| {
                surface.as_ref().map_or(0, |surface| surface.coverage_at(mask, world))
            });
            let index = row_start + offset;
            let weight = (red_weight * coverage * f32::from(selected)).round() as u8;
            weights[index] = weight;
            let score = f32::from(weight) * inside;
            if score > best_score {
                best_score = score;
                seed = Some(index);
            }
        }
    }
    let seed = seed.ok_or_else(|| CommandError::validation("NO_RED_EYE"))?;
    let pupil = connected_pupil(&mut weights, seed, width as usize);
    drop(weights);

    let mut changed = 0u64;
    let mut bounds = [x1, y1, x0, y0];
    for (row, weights) in pupil.chunks_exact(width as usize).enumerate() {
        if weights.iter().all(|&weight| weight == 0) { continue; }
        let y = y0 + row as u32;
        let strip = [x0, y, width, 1];
        let mut rgba = pixels.read_region(strip);
        let mut row_changed = false;
        for (offset, (value, &weight)) in rgba.chunks_exact_mut(4).zip(weights).enumerate() {
            if weight == 0 { continue; }
            let rgb = [value[0], value[1], value[2]];
            let [r, g, b] = rgb.map(f32::from);
            let blend = f32::from(weight) / 255.0;
            let neutral = (0.299 * r + 0.587 * g + 0.114 * b) * 0.2;
            let corrected = [r, g, b].map(|channel| (channel * (1.0 - blend) + neutral * blend).round() as u8);
            if corrected == rgb { continue; }
            value[..3].copy_from_slice(&corrected);
            changed += 1;
            row_changed = true;
            let x = x0 + offset as u32;
            bounds = [bounds[0].min(x), bounds[1].min(y), bounds[2].max(x + 1), bounds[3].max(y + 1)];
        }
        if row_changed { pixels.set_region(strip, &rgba); }
    }
    if changed == 0 { return Err(CommandError::validation("NO_RED_EYE")); }

    let mut document = session.document.clone();
    super::pixel_brush::install_pixels(&mut document, &session.selection.ids, object, pixels, false);
    let result = json!({
        "node_id": target.0,
        "changed_bounds": bounds,
        "changed_tile_count": changed,
        "engine_version": "red-eye-3",
    });
    if record {
        let selection = session.selection.ids.clone();
        super::document_change::commit(session, COMMAND, document, selection, result)
    } else {
        session.document = document;
        Ok(result)
    }
}

fn connected_pupil(weights: &mut [u8], seed: usize, width: usize) -> Vec<u8> {
    let mut pupil = vec![0; weights.len()];
    pupil[seed] = std::mem::take(&mut weights[seed]);
    let mut pending = VecDeque::from([seed]);
    while let Some(index) = pending.pop_front() {
        let column = index % width;
        let neighbors = [
            (column > 0).then(|| index - 1),
            (column + 1 < width).then_some(index + 1),
            index.checked_sub(width),
            (index + width < weights.len()).then_some(index + width),
        ];
        for neighbor in neighbors.into_iter().flatten() {
            let weight = std::mem::take(&mut weights[neighbor]);
            if weight > 0 {
                pupil[neighbor] = weight;
                pending.push_back(neighbor);
            }
        }
    }
    pupil
}

fn red_eye_weight(red: u8, green: u8, blue: u8) -> f32 {
    let [red, green, blue] = [red, green, blue].map(f32::from);
    let warm_bias = (green - blue).max(0.0);
    let red_excess = red - green - warm_bias * 0.5;
    let colour_weight = ((red_excess - (red * 0.22).max(50.0)) / 20.0).clamp(0.0, 1.0);
    let reflection_weight = ((red - blue - 15.0) / 18.0).clamp(0.0, 1.0);
    colour_weight * reflection_weight
}
