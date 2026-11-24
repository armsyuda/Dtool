use dtool_core::document::Document;
use dtool_core::ObjectId;
use dtool_protocol::{CommandError, CommandResult};
use crate::raster;
use crate::{flatten_rgba, ExportControl, ExportReceipt, ExportRequest};

#[derive(Debug, Clone)]
pub struct JpegOptions {
    pub size: [u32; 2],
    pub resample: String,
    pub quality: u8,
    pub progressive: bool,
    pub matte: Option<String>,
}

impl Default for JpegOptions {
    fn default() -> Self {
        Self {
            size: [0, 0],
            resample: "bicubic".to_string(),
            quality: 90,
            progressive: false,
            matte: None,
        }
    }
}

fn matte_colour(matte: Option<&str>) -> Result<[u8; 3], CommandError> {
    match matte {
        Some(value) => raster::parse_matte(value)
            .ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.invalid_params")),
        None => Ok([255, 255, 255]),
    }
}

fn encode_jpeg(width: u32, height: u32, rgb: &[u8], quality: u8, progressive: bool) -> Result<Vec<u8>, CommandError> {
    let width = u16::try_from(width).map_err(|_| CommandError::new("REQUEST_TOO_LARGE"))?;
    let height = u16::try_from(height).map_err(|_| CommandError::new("REQUEST_TOO_LARGE"))?;
    let mut buffer = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut buffer, quality);
    encoder.set_progressive(progressive);
    encoder
        .encode(rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(crate::export_io_error)?;
    Ok(buffer)
}

pub fn encode_image_bytes_to_jpeg(input: &[u8], quality: u8, progressive: bool) -> Result<Vec<u8>, CommandError> {
    let decoded = image::load_from_memory(input).map_err(crate::export_io_error)?;
    let rgba = decoded.to_rgba8();
    let rgb = crate::flatten_rgba(rgba.as_raw(), [255, 255, 255]);
    let quality = if quality == 0 { 90 } else { quality };
    encode_jpeg(rgba.width(), rgba.height(), &rgb, quality, progressive)
}

fn option_warnings(opts: &JpegOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    if !matches!(opts.resample.as_str(), "nearest" | "bilinear" | "bicubic" | "lanczos") {
        warnings.push("warning.jpeg.resample_default".to_string());
    }
    if opts.quality == 0 {
        warnings.push("warning.jpeg.quality_default".to_string());
    }
    warnings
}

pub fn export_jpeg(doc: &Document, object: Option<ObjectId>) -> Result<Vec<u8>, CommandError> {
    let (width, height, pixels) = raster::render_rgba(doc, object, [0, 0])?;
    let rgb = flatten_rgba(&pixels, [255, 255, 255]);
    encode_jpeg(width, height, &rgb, 90, false)
}

pub fn export_jpeg_to(
    doc: &Document,
    req: &ExportRequest,
    opts: &JpegOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    let (width, height, pixels, mut warnings) =
        raster::render_from_request(doc, req, opts.size, control)?;
    warnings.extend(option_warnings(opts));
    let matte = matte_colour(opts.matte.as_deref())?;
    let rgb = flatten_rgba(&pixels, matte);
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }
    let quality = if opts.quality == 0 { 90 } else { opts.quality };
    let buffer = encode_jpeg(width, height, &rgb, quality, opts.progressive)?;
    raster::write_receipt(&req.path, &buffer, warnings, control)
}

pub fn export_jpeg_value(
    doc: &Document,
    req: &ExportRequest,
    opts: &JpegOptions,
    control: &ExportControl,
) -> CommandResult {
    let receipt = export_jpeg_to(doc, req, opts, control)?;
    Ok(serde_json::json!({
        "path": receipt.path.to_string_lossy(),
        "bytes": receipt.bytes,
        "warnings": receipt.warnings,
    }))
}
