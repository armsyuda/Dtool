use dtool_core::document::Document;
use dtool_core::ObjectId;
use dtool_protocol::{CommandError, CommandResult};
use image::ImageEncoder;
use crate::raster;
use crate::{ExportControl, ExportReceipt, ExportRequest};

#[derive(Debug, Clone)]
pub struct PngOptions {
    pub size: [u32; 2],
    pub resample: String,
    pub matte: Option<String>,
}

impl Default for PngOptions {
    fn default() -> Self {
        Self {
            size: [0, 0],
            resample: "bicubic".to_string(),
            matte: None,
        }
    }
}

fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, CommandError> {
    let mut buffer = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buffer);
    encoder
        .write_image(pixels, width, height, image::ExtendedColorType::Rgba8)
        .map_err(crate::export_io_error)?;
    Ok(buffer)
}

fn option_warnings(opts: &PngOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    if !matches!(opts.resample.as_str(), "nearest" | "bilinear" | "bicubic" | "lanczos") {
        warnings.push("warning.png.resample_default".to_string());
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

pub fn export_png(doc: &Document, object: Option<ObjectId>) -> Result<Vec<u8>, CommandError> {
    let (width, height, pixels) = raster::render_rgba(doc, object, [0, 0])?;
    encode_png(width, height, &pixels)
}

pub fn export_png_to(
    doc: &Document,
    req: &ExportRequest,
    opts: &PngOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    let (width, height, pixels, mut warnings) =
        raster::render_from_request(doc, req, opts.size, control)?;
    warnings.extend(option_warnings(opts));
    let pixels = finish_pixels(pixels, opts.matte.as_deref())?;
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }
    let buffer = encode_png(width, height, &pixels)?;
    raster::write_receipt(&req.path, &buffer, warnings, control)
}

pub fn export_png_value(
    doc: &Document,
    req: &ExportRequest,
    opts: &PngOptions,
    control: &ExportControl,
) -> CommandResult {
    let receipt = export_png_to(doc, req, opts, control)?;
    Ok(serde_json::json!({
        "path": receipt.path.to_string_lossy(),
        "bytes": receipt.bytes,
        "warnings": receipt.warnings,
    }))
}
