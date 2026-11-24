use dtool_core::commands::preview::{PREVIEW_MAX_BYTES, PREVIEW_MAX_PX, UI_SCREENSHOT_KIND, UI_STATE_KIND};
use dtool_core::workspace::{ContextId, HostReply, HostRequest};
use dtool_protocol::CommandError;
use serde_json::{json, Value};

pub const UI_STATE_QUERY: &str = "ui.state.query";
pub const UI_SCREENSHOT_CAPTURE: &str = "ui.screenshot.capture";
pub const ROOT_VIEWPORT: &str = "root";
pub const DOCK_PREFIXES: [&str; 3] = ["ui.layout.", "ui.panel.", "ui.preset."];

pub fn is_ui_thread_command(name: &str) -> bool {
    name == UI_STATE_QUERY || name == UI_SCREENSHOT_CAPTURE
}

pub fn is_dock_command(name: &str) -> bool {
    DOCK_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiFrame {
    pub frame: u64,
    pub context_id: Option<ContextId>,
    pub generation: u64,
    pub viewport: [u32; 2],
}

pub fn envelope(kind: &'static str, frame: &UiFrame) -> Value {
    json!({
        "kind": kind,
        "frame": frame.frame,
        "context_id": frame.context_id.map(|id| id.0),
        "generation": frame.generation,
        "viewport": { "id": ROOT_VIEWPORT, "w": frame.viewport[0], "h": frame.viewport[1] },
    })
}

pub fn state_reply(frame: &UiFrame, state: Value) -> Value {
    let mut reply = envelope(UI_STATE_KIND, frame);
    if let (Some(target), Value::Object(fields)) = (reply.as_object_mut(), state) {
        for (key, value) in fields {
            target.entry(key).or_insert(value);
        }
    }
    reply
}

pub fn requested_context(params: &Value, scope: Option<ContextId>) -> Result<Option<ContextId>, CommandError> {
    match params.get("context") {
        None | Some(Value::Null) => Ok(scope),
        Some(value) => value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .map(|id| Some(ContextId(id)))
            .ok_or_else(CommandError::invalid_params),
    }
}

pub fn check_context(params: &Value, scope: Option<ContextId>, active: Option<ContextId>) -> Result<(), CommandError> {
    match requested_context(params, scope)? {
        Some(id) if Some(id) != active => Err(CommandError::context_not_active()),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptureRequest {
    pub max_px: f64,
}

pub fn parse_capture(params: &Value) -> Result<CaptureRequest, CommandError> {
    if !params.is_null() && !params.is_object() {
        return Err(CommandError::invalid_params());
    }
    match params.get("viewport") {
        None | Some(Value::Null) => {}
        Some(Value::String(id)) if id == ROOT_VIEWPORT => {}
        Some(Value::String(_)) => return Err(CommandError::not_found()),
        Some(_) => return Err(CommandError::invalid_params()),
    }
    let max_px = match params.get("max_px") {
        None | Some(Value::Null) => PREVIEW_MAX_PX,
        Some(value) => value.as_f64().ok_or_else(CommandError::invalid_params)?,
    };
    if !max_px.is_finite() || max_px < 1.0 {
        return Err(CommandError::invalid_params());
    }
    if max_px > PREVIEW_MAX_PX {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    Ok(CaptureRequest { max_px })
}

pub fn fit_size(size: [usize; 2], max_px: f64) -> [u32; 2] {
    let longest = size[0].max(size[1]).max(1) as f64;
    let scale = (max_px / longest).min(1.0);
    let fit = |value: usize| ((value as f64 * scale).round() as u32).max(1);
    [fit(size[0]), fit(size[1])]
}

pub fn scaled_rgba(image: &egui::ColorImage, max_px: f64) -> ([u32; 2], Vec<u8>) {
    let [width, height] = fit_size(image.size, max_px);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        let source_y = (y * image.size[1] / height as usize).min(image.size[1].saturating_sub(1));
        for x in 0..width as usize {
            let source_x = (x * image.size[0] / width as usize).min(image.size[0].saturating_sub(1));
            let color = image.pixels[source_y * image.size[0] + source_x];
            pixels.extend_from_slice(&color.to_srgba_unmultiplied());
        }
    }
    ([width, height], pixels)
}

pub fn encode_screenshot(image: &egui::ColorImage, max_px: f64) -> Result<(Vec<u8>, u32, u32), CommandError> {
    if image.size[0] == 0 || image.size[1] == 0 || image.pixels.len() != image.size[0] * image.size[1] {
        return Err(CommandError::no_window());
    }
    let ([width, height], pixels) = scaled_rgba(image, max_px);
    let rgba = dtool_export::raster::RgbaImage::from_raw(width, height, pixels).ok_or_else(CommandError::no_window)?;
    let png = dtool_export::raster::encode(&rgba, &dtool_export::raster::ExportOptions::default())
        .map_err(|_| CommandError::new("EXPORT_FAILED"))?;
    if png.len() > PREVIEW_MAX_BYTES {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    Ok((png, width, height))
}

pub fn screenshot_reply(frame: &UiFrame, image: &egui::ColorImage, max_px: f64) -> Result<Value, CommandError> {
    let (png, width, height) = encode_screenshot(image, max_px)?;
    let mut reply = envelope(UI_SCREENSHOT_KIND, frame);
    if let Some(fields) = reply.as_object_mut() {
        fields.insert("mime_type".into(), json!("image/png"));
        fields.insert("png_base64".into(), json!(base64(&png)));
        fields.insert("width".into(), json!(width));
        fields.insert("height".into(), json!(height));
        fields.insert("byte_size".into(), json!(png.len()));
    }
    Ok(reply)
}

pub fn reply(request: HostRequest, frame: &UiFrame, result: Result<Value, CommandError>) {
    let _ = request.reply.send(HostReply { context: frame.context_id, result, generation: frame.context_id.map(|_| frame.generation) });
}

pub fn request_capture(ctx: &egui::Context, request: HostRequest, frame: UiFrame, capture: CaptureRequest) {
    let sender = request.reply;
    let wake = ctx.clone();
    ctx.request_screenshot(move |image| {
        let result = screenshot_reply(&frame, &image, capture.max_px);
        let _ = sender.send(HostReply { context: frame.context_id, result, generation: frame.context_id.map(|_| frame.generation) });
        wake.request_repaint();
    });
    ctx.request_repaint();
}

pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let a = group[0];
        let b = group.get(1).copied().unwrap_or(0);
        let c = group.get(2).copied().unwrap_or(0);
        result.push(ALPHABET[(a >> 2) as usize] as char);
        result.push(ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        result.push(if group.len() > 1 { ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char } else { '=' });
        result.push(if group.len() > 2 { ALPHABET[(c & 63) as usize] as char } else { '=' });
    }
    result
}