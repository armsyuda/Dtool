use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::selection::PixelSurface;
use crate::{Command, CommandError, CommandResult, ObjectId, Session};
use serde::Deserialize;
use serde_json::json;

pub const TOOL: &str = "tool.red_eye";
const COMMAND: &str = "pixel.red_eye";

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
    let mut changed = 0u64;
    let mut bounds = [x1, y1, x0, y0];
    // Work a row at a time so a large drag never allocates a full-frame scratch buffer.
    for y in y0..y1 {
        let strip = [x0, y, width, 1];
        let mut rgba = pixels.read_region(strip);
        let mut row_changed = false;
        for (offset, value) in rgba.chunks_exact_mut(4).enumerate() {
            let [r, g, b, alpha] = [value[0], value[1], value[2], value[3]];
            if alpha == 0 || !is_red_eye(r, g, b) { continue; }
            let x = x0 + offset as u32;
            let local = [x as f32 + 0.5, y as f32 + 0.5];
            let world = to_world(local);
            let point = if args.document_space { world } else { local };
            let dx = (point[0] - centre[0]) / radius[0];
            let dy = (point[1] - centre[1]) / radius[1];
            let coverage = (1.0 - dx * dx - dy * dy).max(0.0);
            if coverage == 0.0 { continue; }
            let selected = selection.as_ref().map_or(255, |(mask, surface)| {
                surface.as_ref().map_or(0, |surface| surface.coverage_at(mask, world))
            });
            let blend = coverage * f32::from(selected) / 255.0;
            let neutral = (0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)) * 0.2;
            let rgb = [r, g, b].map(|channel| (f32::from(channel) * (1.0 - blend) + neutral * blend).round() as u8);
            if rgb == [r, g, b] { continue; }
            value[..3].copy_from_slice(&rgb);
            changed += 1;
            row_changed = true;
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
        "engine_version": "red-eye-2",
    });
    if record {
        let selection = session.selection.ids.clone();
        super::document_change::commit(session, COMMAND, document, selection, result)
    } else {
        session.document = document;
        Ok(result)
    }
}

fn is_red_eye(red: u8, green: u8, blue: u8) -> bool {
    let [r, g, b] = [red, green, blue].map(i16::from);
    r >= 50 && g <= 180 && b <= 180 && r - g >= 30 && r - b >= 20 && !(r > 200 && g > 150)
}
