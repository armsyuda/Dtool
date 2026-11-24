use crate::commands::{Command, CommandError, CommandResult};
use crate::command::Session;
use crate::object::ObjectId;

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name == "pixel.red_eye" {
        Some(session.has_document)
    } else {
        None
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if can_execute(session, command).is_none() {
        return None;
    }
    Some(run_inner(session, command))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    match run(session, command) {
        Some(Ok(_)) => Some(Ok(())),
        Some(Err(err)) => Some(Err(err)),
        None => None,
    }
}

fn run_inner(session: &mut Session, cmd: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let doc = &mut session.document;
    let rect = parse_rect(&cmd.params)?;
    let target_id = get_target_node(&cmd.params)?;
    let obj = doc.find_object_mut(target_id.0).ok_or_else(CommandError::not_found)?;
    if obj.raster_data.is_none() {
        return Err(CommandError::invalid_params());
    }
    let (w, h) = obj.raster_size.ok_or_else(CommandError::no_document)?;

    let x0 = rect[0].max(0.0).min(w as f32);
    let y0 = rect[1].max(0.0).min(h as f32);
    let x1 = rect[2].max(0.0).min(w as f32);
    let y1 = rect[3].max(0.0).min(h as f32);

    if x0 >= x1 || y0 >= y1 {
        return Err(CommandError::invalid_params());
    }

    let ix0 = x0.floor() as u32;
    let iy0 = y0.floor() as u32;
    let ix1 = x1.ceil() as u32;
    let iy1 = y1.ceil() as u32;

    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    let radius_x = (x1 - x0) / 2.0;
    let radius_y = (y1 - y0) / 2.0;

    let data = obj.raster_data.as_mut().ok_or_else(CommandError::no_document)?;

    let changed_bounds = [ix0, iy0, ix1, iy1];
    let mut changed_count = 0;

    for y in iy0..iy1 {
        for x in ix0..ix1 {
            let dx = (x as f32 + 0.5 - cx) / radius_x;
            let dy = (y as f32 + 0.5 - cy) / radius_y;
            let dist_sq = dx * dx + dy * dy;

            if dist_sq > 1.0 {
                continue;
            }

            let idx = ((y * w + x) * 4) as usize;

            if idx + 3 >= data.len() {
                continue;
            }

            let r = data[idx] as f32;
            let g = data[idx + 1] as f32;
            let b = data[idx + 2] as f32;

            if is_red_eye(r, g, b) {
                let luminance = 0.299 * r + 0.587 * g + 0.114 * b;
                let soft_edge_factor = (1.0 - dist_sq).min(1.0);
                let blend_alpha = soft_edge_factor.min(1.0);
                let dark_neutral = luminance * 0.2;

                data[idx] = (r * (1.0 - blend_alpha) + dark_neutral * blend_alpha) as u8;
                data[idx + 1] = (g * (1.0 - blend_alpha) + dark_neutral * blend_alpha) as u8;
                data[idx + 2] = (b * (1.0 - blend_alpha) + dark_neutral * blend_alpha) as u8;

                changed_count += 1;
            }
        }
    }

    if changed_count == 0 {
        return Err(CommandError::validation("NO_RED_EYE"));
    }

    Ok(serde_json::json!({
        "node_id": target_id.0,
        "changed_bounds": changed_bounds,
        "changed_tile_count": changed_count,
        "engine_version": "red-eye-1"
    }))
}

fn is_red_eye(r: f32, g: f32, b: f32) -> bool {
    if r < 50.0 || g > 180.0 || b > 180.0 {
        return false;
    }
    let rg_diff = r - g;
    let rb_diff = r - b;
    if rg_diff < 30.0 || rb_diff < 20.0 {
        return false;
    }
    if r > 200.0 && g > 150.0 {
        return false;
    }
    true
}

fn parse_rect(params: &serde_json::Value) -> Result<[f32; 4], CommandError> {
    let rect_val = params.get("rect").ok_or_else(CommandError::invalid_params)?;
    let arr = rect_val.as_array().ok_or_else(CommandError::invalid_params)?;
    if arr.len() != 4 {
        return Err(CommandError::invalid_params());
    }
    let mut rect = [0.0f32; 4];
    for i in 0..4 {
        rect[i] = arr[i].as_f64().ok_or_else(CommandError::invalid_params)? as f32;
    }
    Ok(rect)
}

fn get_target_node(params: &serde_json::Value) -> Result<ObjectId, CommandError> {
    let target_str = params.get("target").and_then(|v| v.as_str()).ok_or_else(CommandError::invalid_params)?;
    if !target_str.starts_with("node:") {
        return Err(CommandError::invalid_params());
    }
    let id_str = &target_str[5..];
    match id_str.parse::<u64>() {
        Ok(id) => Ok(ObjectId(id)),
        Err(_) => Err(CommandError::invalid_params()),
    }
}
