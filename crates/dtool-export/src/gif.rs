use dtool_protocol::{CommandError, CommandResult};
use crate::{ExportArea, ExportControl, ExportReceipt, ExportRequest};
use dtool_core::Document;

#[derive(Debug, Clone)]
pub struct GifOptions {
    pub size: [u32; 2],
    pub resample: String,
    pub greyscale: bool,
    pub matte: Option<String>,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self {
            size: [0, 0],
            resample: "bicubic".to_string(),
            greyscale: false,
            matte: None,
        }
    }
}

fn parse_matte(matte: &str) -> Option<[u8; 3]> {
    let hex = matte.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

pub fn export_gif(
    doc: &Document,
    req: &ExportRequest,
    opts: &GifOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }

    let width = opts.size[0];
    let height = opts.size[1];

    if width == 0 || height == 0 {
        return Err(crate::export_error("INVALID_PARAMS", "error.invalid_params"));
    }

    let (_, _, rendered, _) = crate::raster::render_from_request(doc, req, [width, height], control)?;
    let matte = match opts.matte.as_deref() {
        Some(value) => Some(parse_matte(value).ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.invalid_params"))?),
        None => None,
    };
    let mut warnings = Vec::new();
    if !matches!(opts.resample.as_str(), "nearest" | "bilinear" | "bicubic" | "lanczos") {
        warnings.push("warning.gif.resample_default".to_string());
    }

    let mut img = image::RgbaImage::new(width, height);

    for (x, y, pixel) in img.enumerate_pixels_mut() {
        if x == 0 && control.is_cancelled() {
            return Err(CommandError::new("cancelled"));
        }

        let rgba = render_pixel(&rendered, req.area.clone(), x, y, width, height)?;
        
        let mut r = rgba[0];
        let mut g = rgba[1];
        let mut b = rgba[2];
        let mut a = rgba[3];

        if let Some(background) = matte {
            let alpha = f32::from(a) / 255.0;
            let mix = |value: u8, back: u8| (f32::from(value) * alpha + f32::from(back) * (1.0 - alpha)).round() as u8;
            r = mix(r, background[0]);
            g = mix(g, background[1]);
            b = mix(b, background[2]);
            a = 255;
        }

        if opts.greyscale {
            let grey = (r as u32 + g as u32 + b as u32) / 3;
            r = grey as u8;
            g = grey as u8;
            b = grey as u8;
        }

        *pixel = image::Rgba([r, g, b, a]);
    }

    let mut buffer = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut buffer);

        encoder
            .encode(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
            .map_err(crate::export_io_error)?;
    }

    let bytes = buffer.len() as u64;
    
    std::fs::write(&req.path, &buffer)
        .map_err(crate::export_io_error)?;

    control.set_progress(1.0);

    Ok(ExportReceipt {
        path: req.path.clone(),
        bytes,
        warnings,
    })
}

pub fn export_gif_value(
    doc: &Document,
    req: &ExportRequest,
    opts: &GifOptions,
    control: &ExportControl,
) -> CommandResult {
    let receipt = export_gif(doc, req, opts, control)?;
    Ok(serde_json::json!({
        "path": receipt.path.to_string_lossy(),
        "bytes": receipt.bytes,
        "warnings": receipt.warnings,
    }))
}

fn render_pixel(
    rendered: &[u8],
    area: ExportArea,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<[u8; 4], CommandError> {
    if x >= width || y >= height {
        return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area"));
    }
    let offset = (y as usize * width as usize + x as usize) * 4;
    match rendered.get(offset..offset + 4) {
        Some(pixel) => {
            let alpha = if matches!(area, ExportArea::SelectionWithBackground) && pixel[3] == 0 { 255 } else { pixel[3] };
            if alpha == 255 && pixel[3] == 0 {
                Ok([255, 255, 255, 255])
            } else {
                Ok([pixel[0], pixel[1], pixel[2], alpha])
            }
        }
        None => Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area")),
    }
}
