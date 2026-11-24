use std::sync::OnceLock;
use dtool_core::document::Document;
use dtool_core::ObjectId;
use dtool_protocol::{CommandError, CommandResult};
use crate::raster;
use crate::{ExportControl, ExportReceipt, ExportRequest};

pub type LossyWebpEncoder = fn(&[u8], u32, u32, f32) -> Result<Vec<u8>, String>;

static LOSSY_ENCODER: OnceLock<LossyWebpEncoder> = OnceLock::new();

pub fn register_lossy_encoder(encoder: LossyWebpEncoder) -> bool {
    LOSSY_ENCODER.set(encoder).is_ok() || LOSSY_ENCODER.get().is_some()
}

pub fn lossy_encoder_available() -> bool {
    LOSSY_ENCODER.get().is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebpEncodeError {
    LossyUnavailable,
    Failed(String),
}

impl WebpEncodeError {
    pub fn to_command_error(&self) -> CommandError {
        match self {
            WebpEncodeError::LossyUnavailable => crate::export_error("NOT_IMPLEMENTED", "error.not_implemented"),
            WebpEncodeError::Failed(_) => crate::export_error("EXPORT_FAILED", "error.export.failed"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WebpOptions {
    pub size: [u32; 2],
    pub resample: String,
    pub quality: u8,
    pub lossless: bool,
    pub matte: Option<String>,
}

impl Default for WebpOptions {
    fn default() -> Self {
        Self {
            size: [0, 0],
            resample: "bicubic".to_string(),
            quality: 80,
            lossless: true,
            matte: None,
        }
    }
}

pub fn is_lossy_webp(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return false;
    }
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let fourcc = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7]]) as usize;
        match fourcc {
            b"VP8 " => return true,
            b"VP8L" => return false,
            _ => {}
        }
        offset = match offset.checked_add(8 + size + (size & 1)) {
            Some(next) => next,
            None => return false,
        };
    }
    false
}

pub fn encode_rgba(width: u32, height: u32, pixels: &[u8], lossless: bool, quality: u8) -> Result<Vec<u8>, WebpEncodeError> {
    if lossless {
        let mut buffer = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut buffer)
            .encode(pixels, width, height, image::ExtendedColorType::Rgba8)
            .map_err(|error| WebpEncodeError::Failed(error.to_string()))?;
        return Ok(buffer);
    }
    crate::webp_lossy::install();
    let encoder = LOSSY_ENCODER.get().ok_or(WebpEncodeError::LossyUnavailable)?;
    let bytes = encoder(pixels, width, height, f32::from(quality.clamp(1, 100))).map_err(WebpEncodeError::Failed)?;
    if !is_lossy_webp(&bytes) {
        return Err(WebpEncodeError::Failed("webp_not_lossy".to_string()));
    }
    Ok(bytes)
}

fn encode_webp(width: u32, height: u32, pixels: &[u8], lossless: bool, quality: u8) -> Result<Vec<u8>, CommandError> {
    encode_rgba(width, height, pixels, lossless, quality).map_err(|error| error.to_command_error())
}

fn option_warnings(opts: &WebpOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    if !matches!(opts.resample.as_str(), "nearest" | "bilinear" | "bicubic" | "lanczos") {
        warnings.push("warning.webp.resample_default".to_string());
    }
    warnings
}

fn finish_pixels(mut pixels: Vec<u8>, matte: Option<&str>) -> Result<Vec<u8>, CommandError> {
    if let Some(value) = matte {
        let colour = raster::parse_matte(value)
            .ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.invalid_params"))?;
        raster::apply_matte(&mut pixels, colour);
    }
    Ok(pixels)
}

pub fn export_webp(doc: &Document, object: Option<ObjectId>) -> Result<Vec<u8>, CommandError> {
    let (width, height, pixels) = raster::render_rgba(doc, object, [0, 0])?;
    encode_webp(width, height, &pixels, true, 100)
}

pub fn export_webp_to(
    doc: &Document,
    req: &ExportRequest,
    opts: &WebpOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    if !opts.lossless && !lossy_encoder_available() {
        return Err(WebpEncodeError::LossyUnavailable.to_command_error());
    }
    let (width, height, pixels, mut warnings) =
        raster::render_from_request(doc, req, opts.size, control)?;
    warnings.extend(option_warnings(opts));
    let pixels = finish_pixels(pixels, opts.matte.as_deref())?;
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }
    let buffer = encode_webp(width, height, &pixels, opts.lossless, opts.quality)?;
    raster::write_receipt(&req.path, &buffer, warnings, control)
}

pub fn export_webp_value(
    doc: &Document,
    req: &ExportRequest,
    opts: &WebpOptions,
    control: &ExportControl,
) -> CommandResult {
    let receipt = export_webp_to(doc, req, opts, control)?;
    Ok(serde_json::json!({
        "path": receipt.path.to_string_lossy(),
        "bytes": receipt.bytes,
        "warnings": receipt.warnings,
    }))
}