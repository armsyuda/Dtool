use dtool_core::document::Document;
use dtool_core::ObjectId;
use dtool_protocol::CommandError;
use crate::{DocumentExportExt, ExportArea, ExportControl, ExportFormat, ExportRequest, ObjectExportExt};

pub fn artboard_region(doc: &Document, id: ObjectId) -> Option<(f32, f32, f32, f32)> {
    doc.get_object(id)
        .filter(|object| matches!(object.kind, dtool_core::ObjectKind::Artboard))
        .and_then(|_| doc.artboard_bounds(id))
}

pub fn artboard_file_name(doc: &Document, id: ObjectId) -> String {
    let name = doc
        .get_object(id)
        .filter(|object| matches!(object.kind, dtool_core::ObjectKind::Artboard))
        .map(|object| object.name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect::<String>())
        .map(|name| name.trim().trim_end_matches('.').to_string())
        .unwrap_or_default();
    if name.is_empty() { format!("artboard_{}", id.0) } else { name }
}

pub fn area_name_in(doc: &Document, area: &ExportArea) -> String {
    match area {
        ExportArea::Artboard(id) => artboard_file_name(doc, *id),
        other => area_name(other),
    }
}

pub(crate) fn parse_matte(matte: &str) -> Option<[u8; 3]> {    let hex = matte.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

pub(crate) fn apply_matte(pixels: &mut [u8], matte: [u8; 3]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = f32::from(pixel[3]) / 255.0;
        for channel in 0..3 {
            let value = f32::from(pixel[channel]) * alpha + f32::from(matte[channel]) * (1.0 - alpha);
            pixel[channel] = value.round().clamp(0.0, 255.0) as u8;
        }
        pixel[3] = 255;
    }
}

fn fallback_extent(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        72.0
    }
}

pub(crate) fn region_for(doc: &Document, object: Option<ObjectId>) -> Result<(f32, f32, f32, f32), CommandError> {
    match object {
        Some(id) => {
            let object = doc
                .get_object(id)
                .ok_or_else(|| crate::export_error("NOT_FOUND", "error.not_found"))?;
            let width = fallback_extent(object.width());
            let height = fallback_extent(object.height());
            Ok((object.x(), object.y(), width, height))
        }
        None => {
            let width = fallback_extent(DocumentExportExt::width(doc));
            let height = fallback_extent(DocumentExportExt::height(doc));
            Ok((0.0, 0.0, width, height))
        }
    }
}

pub(crate) fn pixel_size(region: (f32, f32, f32, f32), _doc_dpi: f32, requested: [u32; 2]) -> Result<(u32, u32), CommandError> {
    if requested[0] > 0 && requested[1] > 0 {
        return Ok((requested[0], requested[1]));
    }
    let width = (region.2.round().max(1.0)) as u32;
    let height = (region.3.round().max(1.0)) as u32;
    Ok((width, height))
}

pub(crate) fn render_rgba(
    doc: &Document,
    object: Option<ObjectId>,
    size: [u32; 2],
) -> Result<(u32, u32, Vec<u8>), CommandError> {
    let region = region_for(doc, object)?;
    let (width, height) = pixel_size(region, doc.dpi, size)?;
    let pixels = doc.render_rgba_region(region, width as usize, height as usize);
    if pixels.len() != width as usize * height as usize * 4 {
        return Err(crate::export_error("EXPORT_FAILED", "error.export.failed"));
    }
    Ok((width, height, pixels))
}

pub(crate) fn render_from_request(
    doc: &Document,
    req: &ExportRequest,
    size: [u32; 2],
    control: &ExportControl,
) -> Result<(u32, u32, Vec<u8>, Vec<String>), CommandError> {
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }
    let mut warnings = Vec::new();
    if let ExportArea::Artboard(id) = &req.area {
        if artboard_region(doc, *id).is_none() {
            return Err(crate::export_error("NOT_FOUND", "error.export.bad_area"));
        }
    }
    let area = if use_page_runs(doc, &req.area, req.format) {
        page_run_areas(doc, &req.area, req.pages.as_deref()).into_iter().next()
            .ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.export.bad_area"))?
    } else {
        req.area.clone()
    };
    let region = match doc.area_bounds(&area) {
        Some(bounds) => bounds,
        None => {
            if matches!(req.area, ExportArea::WholeDocument) {
                region_for(doc, None)?
            } else if matches!(req.area, ExportArea::Artboard(_)) {
                return Err(crate::export_error("NOT_FOUND", "error.export.bad_area"));
            } else {
                return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area"));
            }
        }
    };
    let (width, height) = pixel_size(region, doc.dpi, size)?;
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }
    let pixels = render_rgba_for_area(doc, &area, region, [width as usize, height as usize]);
    if pixels.len() != width as usize * height as usize * 4 {
        return Err(crate::export_error("EXPORT_FAILED", "error.export.failed"));
    }
    if size[0] == 0 || size[1] == 0 {
        warnings.push("warning.raster.size_from_dpi".to_string());
    }
    Ok((width, height, pixels, warnings))
}

pub(crate) fn write_receipt(
    path: &std::path::PathBuf,
    buffer: &[u8],
    warnings: Vec<String>,
    control: &ExportControl,
) -> Result<crate::ExportReceipt, CommandError> {
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }
    crate::write_atomic(path, buffer).map_err(crate::export_io_error)?;
    control.set_progress(1.0);
    Ok(crate::ExportReceipt {
        path: path.clone(),
        bytes: buffer.len() as u64,
        warnings,
    })
}

pub type RgbaImage = image::RgbaImage;

pub const MAX_EXPORT_SIDE: u32 = 32768;
pub const MAX_EXPORT_PIXELS: u64 = 268_435_456;
pub const ERROR_UNAVAILABLE: &str = "EXPORT_OPTION_UNAVAILABLE";
pub const ERROR_PRESET_INVALID: &str = "EXPORT_PRESET_INVALID";
pub const ERROR_COLLISION: &str = "EXPORT_COLLISION";
pub const ERROR_FAILED: &str = "EXPORT_FAILED";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn white() -> Self {
        Self::new(255, 255, 255, 255)
    }

    pub fn rgb(&self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }

    pub fn parse(value: &str) -> Option<Self> {
        parse_matte(value).map(|c| Self::new(c[0], c[1], c[2], 255))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum RenderIntent {
    #[default]
    Perceptual,
    RelativeColorimetric,
    Saturation,
    AbsoluteColorimetric,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Resample {
    Nearest,
    Bilinear,
    #[default]
    Bicubic,
    Lanczos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Deferred {
    V3,
    Later,
    Planned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    Unavailable(String),
    NotFound(String),
    InvalidArea(String),
    Io(String),
    Cancelled,
    Collision(std::path::PathBuf),
}

impl ExportError {
    pub fn code(&self) -> &'static str {
        match self {
            ExportError::Unavailable(_) => ERROR_UNAVAILABLE,
            ExportError::NotFound(_) => "NOT_FOUND",
            ExportError::InvalidArea(_) => "INVALID_PARAMS",
            ExportError::Io(_) => ERROR_FAILED,
            ExportError::Cancelled => "CANCELLED",
            ExportError::Collision(_) => ERROR_COLLISION,
        }
    }

    pub fn message_key(&self) -> &'static str {
        match self {
            ExportError::Unavailable(_) => "error.export.option_unavailable",
            ExportError::NotFound(_) | ExportError::InvalidArea(_) => "error.export.bad_area",
            ExportError::Io(_) => "error.export.failed",
            ExportError::Cancelled => "error.export.cancelled",
            ExportError::Collision(_) => "error.export.collision",
        }
    }

    pub fn to_command_error(&self) -> CommandError {
        crate::export_error(self.code(), self.message_key())
    }
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Unavailable(what) => write!(f, "{}: {}", ERROR_UNAVAILABLE, what),
            ExportError::NotFound(what) => write!(f, "NOT_FOUND: {}", what),
            ExportError::InvalidArea(what) => write!(f, "INVALID_PARAMS: {}", what),
            ExportError::Io(what) => write!(f, "{}: {}", ERROR_FAILED, what),
            ExportError::Cancelled => write!(f, "CANCELLED"),
            ExportError::Collision(path) => write!(f, "{}: {}", ERROR_COLLISION, path.display()),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<ExportError> for CommandError {
    fn from(error: ExportError) -> Self {
        error.to_command_error()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PdfColourSpace {
    #[serde(rename = "RGB", alias = "rgb")]
    Rgb,
    #[default]
    #[serde(rename = "CMYK", alias = "cmyk")]
    Cmyk,
}

impl PdfColourSpace {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Cmyk => "CMYK",
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub area: ExportArea,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub scale: Option<f32>,
    pub dpi: Option<f32>,
    pub resample: Resample,
    pub quality: u8,
    pub lossless: bool,
    pub progressive: bool,
    pub bit_depth: u8,
    pub transparent: bool,
    pub matte: Option<Rgba>,
    pub icc: Option<String>,
    pub embed_icc: bool,
    pub pdf_colour_space: Option<PdfColourSpace>,
    pub intent: RenderIntent,
    pub embed_metadata: bool,
    pub skip_hidden: bool,
    pub include_bleed: bool,
    pub pdf_crop_marks: bool,
    pub pdf_fold_marks: bool,
    pub pdf_raster_dpi: Option<f32>,
    pub pdf_outline_text: bool,
    pub pdf_outline_strokes: bool,
    pub pdf_include_hyperlinks: bool,
    pub pdf_include_bookmarks: bool,
    pub pages: Option<Vec<u32>>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Png,
            area: ExportArea::WholeDocument,
            width: None,
            height: None,
            scale: None,
            dpi: None,
            resample: Resample::Bicubic,
            quality: 90,
            lossless: false,
            progressive: false,
            bit_depth: 8,
            transparent: true,
            matte: None,
            icc: None,
            embed_icc: false,
            pdf_colour_space: None,
            intent: RenderIntent::Perceptual,
            embed_metadata: false,
            skip_hidden: true,
            include_bleed: false,
            pdf_crop_marks: false,
            pdf_fold_marks: false,
            pdf_raster_dpi: None,
            pdf_outline_text: true,
            pdf_outline_strokes: false,
            pdf_include_hyperlinks: true,
            pdf_include_bookmarks: true,
            pages: None,
        }
    }
}

impl ExportOptions {
    pub fn for_format(format: ExportFormat) -> Self {
        let mut options = Self { format, ..Self::default() };
        match format {
            ExportFormat::Jpeg => {
                options.transparent = false;
                options.quality = 90;
            }
            ExportFormat::Webp => {
                options.lossless = true;
                options.quality = 80;
            }
            _ => {}
        }
        options
    }

    pub fn merge_json(&self, patch: &serde_json::Value) -> Result<Self, ExportError> {
        let mut base = serde_json::to_value(self).map_err(|e| ExportError::Io(e.to_string()))?;
        let Some(fields) = patch.as_object() else {
            return Ok(self.clone());
        };
        let mut next_format = None;
        for (key, value) in fields {
            if key == "format" {
                if let Some(name) = value.as_str() {
                    let format = parse_format(name)?;
                    next_format = Some(format);
                    base[key] = serde_json::to_value(format).map_err(|e| ExportError::Io(e.to_string()))?;
                    continue;
                }
            }
            if key == "matte" {
                if let Some(name) = value.as_str() {
                    let colour = Rgba::parse(name).ok_or_else(|| ExportError::Unavailable("matte".to_string()))?;
                    base[key] = serde_json::to_value(colour).map_err(|e| ExportError::Io(e.to_string()))?;
                    continue;
                }
            }
            if base.get(key).is_none() {
                return Err(ExportError::Unavailable(key.clone()));
            }
            base[key] = value.clone();
        }
        let mut merged: ExportOptions = serde_json::from_value(base).map_err(|e| ExportError::Unavailable(e.to_string()))?;
        if let Some(format) = next_format.filter(|format| *format != self.format) {
            if !fields.contains_key("transparent") {
                merged.transparent = format != ExportFormat::Jpeg;
            }
            if format != ExportFormat::Pdf && !fields.contains_key("pdf_colour_space") {
                merged.pdf_colour_space = None;
            }
            if format != ExportFormat::Pdf && !fields.contains_key("pdf_raster_dpi") {
                merged.pdf_raster_dpi = None;
            }
            if format != ExportFormat::Pdf && !fields.contains_key("pdf_crop_marks") {
                merged.pdf_crop_marks = false;
            }
        }
        if merged.area != self.area && !merged.area.is_page_based() {
            if !fields.contains_key("include_bleed") {
                merged.include_bleed = false;
            }
            if !fields.contains_key("pdf_crop_marks") {
                merged.pdf_crop_marks = false;
            }
        }
        if (merged.format != ExportFormat::Pdf || !merged.area.limits_objects_to_page())
            && !fields.contains_key("pdf_fold_marks")
        {
            merged.pdf_fold_marks = false;
        }
        merged.validate()?;
        Ok(merged)
    }

    pub fn validate(&self) -> Result<(), ExportError> {
        if !export_available(self.format) {
            return Err(ExportError::Unavailable(format_name(self.format).to_string()));
        }
        if self.width == Some(0) || self.height == Some(0) {
            return Err(ExportError::Unavailable("width".to_string()));
        }
        if let Some(scale) = self.scale {
            if !scale.is_finite() || scale <= 0.0 {
                return Err(ExportError::Unavailable("scale".to_string()));
            }
        }
        if let Some(dpi) = self.dpi {
            if !dpi.is_finite() || dpi <= 0.0 {
                return Err(ExportError::Unavailable("dpi".to_string()));
            }
        }
        if self.bit_depth != 8 {
            return Err(ExportError::Unavailable("bit_depth".to_string()));
        }
        if let Some(field) = self.unapplied_field() {
            return Err(ExportError::Unavailable(field.to_string()));
        }
        Ok(())
    }

    pub fn unapplied_field(&self) -> Option<&'static str> {
        let pdf = self.format == ExportFormat::Pdf;
        if !pdf && self.pdf_colour_space.is_some() {
            return Some("pdf_colour_space");
        }
        if let Some(dpi) = self.pdf_raster_dpi
            && (!pdf || !(dtool_core::effect_raster::MIN_DPI..=dtool_core::effect_raster::MAX_DPI).contains(&dpi))
        {
            return Some("pdf_raster_dpi");
        }
        if self.pdf_crop_marks && (!pdf || !self.area.is_page_based()) {
            return Some("pdf_crop_marks");
        }
        if self.pdf_fold_marks && (!pdf || !self.area.limits_objects_to_page()) {
            return Some("pdf_fold_marks");
        }
        let embeddable = pdf || matches!(self.format, ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Webp);
        if self.resample != Resample::Bicubic && !matches!(self.format, ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Webp) {
            return Some("resample");
        }
        if pdf && self.icc.is_some() && !self.embed_icc {
            return Some("icc");
        }
        if pdf && self.intent != RenderIntent::Perceptual {
            return Some("intent");
        }
        if self.icc.is_some() && !embeddable {
            return Some("icc");
        }
        if self.embed_icc && !embeddable {
            return Some("embed_icc");
        }
        if self.intent != RenderIntent::Perceptual && self.icc.is_none() {
            return Some("intent");
        }
        if self.embed_metadata && !embeddable {
            return Some("embed_metadata");
        }
        if !self.skip_hidden {
            return Some("skip_hidden");
        }
        if self.include_bleed && (!self.area.is_page_based() || !(embeddable || pdf)) {
            return Some("include_bleed");
        }
        None
    }

    pub fn effective_quality(&self) -> u8 {
        self.quality.clamp(1, 100)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormatSchema {
    pub format: String,
    pub extension: String,
    pub bundle: String,
    pub fields: Vec<String>,
    pub available: bool,
    pub deferred: Option<Deferred>,
}

pub const RASTER_FORMATS: [ExportFormat; 5] = [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Webp, ExportFormat::Tiff, ExportFormat::Gif];
pub const EXPOSED_RASTER_FORMATS: [ExportFormat; 3] = [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Webp];

pub fn format_name(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Png => "png",
        ExportFormat::Jpeg => "jpeg",
        ExportFormat::Webp => "webp",
        ExportFormat::Tiff => "tiff",
        ExportFormat::Gif => "gif",
        ExportFormat::Pdf => "pdf",
        ExportFormat::Svg => "svg",
        ExportFormat::Eps => "eps",
        ExportFormat::Wmf => "wmf",
    }
}

pub fn format_extension(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Jpeg => "jpg",
        ExportFormat::Tiff => "tif",
        other => format_name(other),
    }
}

pub fn format_available(format: ExportFormat) -> bool {
    EXPOSED_RASTER_FORMATS.contains(&format)
}

pub const E06_OFFERED_FORMATS: &[ExportFormat] = &[
    ExportFormat::Png,
    ExportFormat::Jpeg,
    ExportFormat::Pdf,
    ExportFormat::Svg,
    ExportFormat::Webp,
    ExportFormat::Eps,
];

pub const E06_HELD_FORMATS: &[ExportFormat] = &[
    ExportFormat::Tiff,
    ExportFormat::Gif,
    ExportFormat::Wmf,
];

pub fn is_e06_offered(format: ExportFormat) -> bool {
    E06_OFFERED_FORMATS.contains(&format)
}

pub fn is_e06_held(format: ExportFormat) -> bool {
    E06_HELD_FORMATS.contains(&format)
}

pub fn export_available(format: ExportFormat) -> bool {
    is_e06_offered(format) && (matches!(format, ExportFormat::Pdf | ExportFormat::Svg | ExportFormat::Eps) || format_available(format))
}

pub fn parse_format(name: &str) -> Result<ExportFormat, ExportError> {
    let lower = name.trim().to_ascii_lowercase();
    let format = match lower.as_str() {
        "png" => ExportFormat::Png,
        "jpeg" | "jpg" => ExportFormat::Jpeg,
        "webp" => ExportFormat::Webp,
        "tiff" | "tif" => ExportFormat::Tiff,
        "gif" => ExportFormat::Gif,
        "pdf" => ExportFormat::Pdf,
        "svg" => ExportFormat::Svg,
        "eps" => ExportFormat::Eps,
        "wmf" => ExportFormat::Wmf,
        _ => return Err(ExportError::Unavailable(lower)),
    };
    Ok(format)
}

pub fn options_schema() -> Vec<FormatSchema> {
    let raster_fields = ["area", "width", "height", "scale", "dpi", "transparent", "matte"];
    let entry = |format: &str, extension: &str, bundle: &str, extra: &[&str], available: bool, deferred: Option<Deferred>| {
        let mut fields: Vec<String> = vec!["format".to_string()];
        if available {
            fields.extend(raster_fields.iter().map(|f| f.to_string()));
        }
        fields.extend(extra.iter().map(|f| f.to_string()));
        FormatSchema {
            format: format.to_string(),
            extension: extension.to_string(),
            bundle: bundle.to_string(),
            fields,
            available,
            deferred,
        }
    };
    vec![
        entry("png", "png", "B54", &["bit_depth", "resample", "icc", "embed_icc", "intent", "embed_metadata", "include_bleed"], true, None),
        entry("jpeg", "jpg", "B54", &["quality", "progressive", "resample", "icc", "embed_icc", "intent", "embed_metadata", "include_bleed"], true, None),
        entry("webp", "webp", "B54", &["quality", "lossless", "resample", "icc", "embed_icc", "intent", "embed_metadata", "include_bleed"], true, None),
        entry("tiff", "tif", "B54", &["bit_depth"], false, Some(Deferred::Planned)),
        entry("gif", "gif", "B54", &[], false, Some(Deferred::Planned)),
        entry("pdf", "pdf", "B55", &["pdf_colour_space", "icc", "embed_icc", "embed_metadata", "include_bleed", "pdf_crop_marks", "pdf_fold_marks", "pdf_raster_dpi", "pdf_outline_text", "pdf_outline_strokes", "pdf_include_hyperlinks", "pdf_include_bookmarks"], true, None),
        entry("svg", "svg", "B55", &[], true, None),
        entry("eps", "eps", "B54", &[], true, None),
        entry("wmf", "wmf", "B55", &[], false, Some(Deferred::Planned)),
        entry("psd", "psd", "B54", &[], false, Some(Deferred::V3)),
    ]
}

fn clamp_side(value: f32) -> Result<u32, ExportError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ExportError::Unavailable("size".to_string()));
    }
    let side = value.round().max(1.0);
    if side > MAX_EXPORT_SIDE as f32 {
        return Err(ExportError::Unavailable("size".to_string()));
    }
    Ok(side as u32)
}

pub fn target_size(region: (f32, f32, f32, f32), doc_dpi: f32, opts: &ExportOptions) -> Result<(u32, u32), ExportError> {
    let (rw, rh) = (fallback_extent(region.2), fallback_extent(region.3));
    let aspect = rh / rw;
    let size = match (opts.width, opts.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, clamp_side(w as f32 * aspect)?),
        (None, Some(h)) => (clamp_side(h as f32 / aspect)?, h),
        (None, None) => {
            let base_dpi = if doc_dpi.is_finite() && doc_dpi > 0.0 { doc_dpi } else { 72.0 };
            let factor = match (opts.scale, opts.dpi) {
                (Some(scale), _) => scale,
                (None, Some(dpi)) => dpi / base_dpi,
                (None, None) => 1.0,
            };
            (clamp_side(rw * factor)?, clamp_side(rh * factor)?)
        }
    };
    if size.0 == 0 || size.1 == 0 || size.0 > MAX_EXPORT_SIDE || size.1 > MAX_EXPORT_SIDE || u64::from(size.0) * u64::from(size.1) > MAX_EXPORT_PIXELS {
        return Err(ExportError::Unavailable("size".to_string()));
    }
    Ok(size)
}

pub fn area_name(area: &ExportArea) -> String {
    match area {
        ExportArea::WholeDocument => "document".to_string(),
        ExportArea::CurrentPage => "page".to_string(),
        ExportArea::AllPages => "pages".to_string(),
        ExportArea::CurrentSpread => "spread".to_string(),
        ExportArea::AllSpreads => "spreads".to_string(),
        ExportArea::Page(index) => format!("page_{:02}", index + 1),
        ExportArea::Spread(index) => format!("spread_{:02}", index + 1),
        ExportArea::PageRun { first, last } if first == last => format!("page_{:02}", u64::from(*first) + 1),
        ExportArea::PageRun { first, last } => format!("pages_{:02}-{:02}", u64::from(*first) + 1, u64::from(*last) + 1),
        ExportArea::SelectionWithBackground => "selection".to_string(),
        ExportArea::SelectionWithoutBackground => "selection_only".to_string(),
        ExportArea::Artboard(id) => format!("artboard_{}", id.0),
        ExportArea::Rect(_) => "area".to_string(),
    }
}

pub fn area_region(doc: &Document, area: &ExportArea) -> Result<(f32, f32, f32, f32), ExportError> {
    if let ExportArea::Artboard(id) = area {
        if artboard_region(doc, *id).is_none() {
            return Err(ExportError::NotFound(format!("artboard:{}", id.0)));
        }
    }
    match doc.area_bounds(area) {
        Some(bounds) => Ok(bounds),
        None if matches!(area, ExportArea::WholeDocument) => region_for(doc, None).map_err(|e| ExportError::Io(e.code)),
        None => Err(ExportError::InvalidArea(area_name(area))),
    }
}

fn use_page_runs(doc: &Document, area: &ExportArea, format: ExportFormat) -> bool {
    !matches!(format, ExportFormat::Pdf | ExportFormat::Svg | ExportFormat::Eps | ExportFormat::Wmf)
        && area.limits_objects_to_page()
        && (doc.layout_artboard_origins.is_some() || doc.sections().iter().any(|section| !section.include_on_export))
}

fn page_run_areas(doc: &Document, area: &ExportArea, pages: Option<&[u32]>) -> Vec<ExportArea> {
    crate::output_page_batches(doc, area, pages).into_iter().filter_map(|batch| {
        Some(ExportArea::PageRun { first: *batch.first()? as u32, last: *batch.last()? as u32 })
    }).collect()
}

pub fn resolve_output_areas(
    doc: &Document,
    options: &ExportOptions,
    context: &crate::ExportContext,
) -> Result<Vec<crate::ResolvedArea>, ExportError> {
    let bad_area = || ExportError::InvalidArea(area_name(&options.area));
    let pages = options.pages.as_deref();
    let resolved = crate::resolve_areas(doc, &options.area, context, pages).map_err(|_| bad_area())?;
    if !use_page_runs(doc, &options.area, options.format) {
        return Ok(resolved);
    }
    let selector = if options.area.is_multi() { &options.area } else { &resolved[0].area };
    let areas = page_run_areas(doc, selector, pages);
    if areas.is_empty() {
        return Err(bad_area());
    }
    let many = areas.len() > 1;
    Ok(areas.into_iter().enumerate().map(|(index, area)| crate::ResolvedArea {
        suffix: many.then(|| format!("-{:02}", index + 1)),
        area,
        only: None,
    }).collect())
}

pub fn render_area(doc: &Document, area: &ExportArea, opts: &ExportOptions) -> Result<Vec<(String, RgbaImage)>, ExportError> {
    opts.validate()?;
    if use_page_runs(doc, area, opts.format) {
        let options = ExportOptions { area: area.clone(), ..opts.clone() };
        return resolve_output_areas(doc, &options, &crate::ExportContext::default())?.iter().map(|output| {
            let region = bleed_region(doc, &output.area, area_region(doc, &output.area)?, opts)?;
            let image = render_region_image(doc, &output.area, region, opts)?;
            let name = output.suffix.as_ref().map_or_else(|| area_name_in(doc, area), |suffix| format!("{}{suffix}", area_name(area)));
            Ok((name, image))
        }).collect();
    }
    if area.is_multi() {
        let regions = crate::area_regions(doc, area, opts.pages.as_deref()).ok_or_else(|| ExportError::InvalidArea(area_name(area)))?;
        let mut out = Vec::with_capacity(regions.len());
        for (index, region) in regions.into_iter().enumerate() {
            let region = bleed_region(doc, &ExportArea::WholeDocument, region, opts)?;
            let image = render_region_image(doc, area, region, opts)?;
            out.push((format!("{}-{:02}", area_name(area), index + 1), image));
        }
        return Ok(out);
    }
    let region = bleed_region(doc, area, area_region(doc, area)?, opts)?;
    let image = render_region_image(doc, area, region, opts)?;
    Ok(vec![(area_name_in(doc, area), image)])
}

pub(crate) fn render_rgba_for_area(doc: &Document, area: &ExportArea, region: (f32, f32, f32, f32), [w, h]: [usize; 2]) -> Vec<u8> {
    match area {
        ExportArea::Artboard(id) => crate::render_artboard_rgba_region(doc, *id, region, [w, h]),
        ExportArea::PageRun { first, last } => {
            let pages = crate::page_run(doc, *first, *last).into_iter().flatten().collect::<Vec<_>>();
            crate::render_pages_rgba_region(doc, &pages, region, [w, h])
        }
        _ => doc.render_rgba_region(region, w, h),
    }
}

fn render_region_image(doc: &Document, area: &ExportArea, region: (f32, f32, f32, f32), opts: &ExportOptions) -> Result<RgbaImage, ExportError> {
    let per_unit = document_unit_points(doc) * doc.dpi.max(1.0) / 72.0;
    let pixel_region = (region.0 * per_unit, region.1 * per_unit, region.2 * per_unit, region.3 * per_unit);
    let (width, height) = target_size(pixel_region, doc.dpi, opts)?;
    let native = native_size(pixel_region);
    let render = |w: u32, h: u32| {
        let size = [w as usize, h as usize];
        if let ExportArea::PageRun { first, last } = area
            && let Some(pages) = crate::page_run(doc, *first, *last)
        {
            let pages = pages.collect::<Vec<_>>();
            if opts.include_bleed && let Some(pixels) = crate::render_facing_bleed_rgba_region(doc, &pages, region, size, bleed_insets(doc)) {
                return pixels;
            }
            return crate::render_pages_rgba_region(doc, &pages, region, size);
        }
        render_rgba_for_area(doc, area, region, size)
    };
    let image = match (resample_filter(opts.resample), native) {
        (Some(filter), Some((nw, nh))) if (nw, nh) != (width, height) => {
            let pixels = render(nw, nh);
            let source = RgbaImage::from_raw(nw, nh, pixels).ok_or_else(|| ExportError::Io(ERROR_FAILED.to_string()))?;
            image::imageops::resize(&source, width, height, filter)
        }
        _ => {
            let pixels = render(width, height);
            RgbaImage::from_raw(width, height, pixels).ok_or_else(|| ExportError::Io(ERROR_FAILED.to_string()))?
        }
    };
    Ok(image)
}

pub fn resample_filter(resample: Resample) -> Option<image::imageops::FilterType> {
    match resample {
        Resample::Nearest => Some(image::imageops::FilterType::Nearest),
        Resample::Bilinear => Some(image::imageops::FilterType::Triangle),
        Resample::Lanczos => Some(image::imageops::FilterType::Lanczos3),
        Resample::Bicubic => None,
    }
}

fn native_size(pixel_region: (f32, f32, f32, f32)) -> Option<(u32, u32)> {
    let (w, h) = (clamp_side(pixel_region.2).ok()?, clamp_side(pixel_region.3).ok()?);
    (u64::from(w) * u64::from(h) <= MAX_EXPORT_PIXELS).then_some((w, h))
}

pub fn document_unit_points(doc: &Document) -> f32 {
    match doc.units {
        dtool_core::document::Units::Px => 72.0 / doc.dpi.max(1.0),
        dtool_core::document::Units::Pt => 1.0,
        dtool_core::document::Units::Mm => 72.0 / 25.4,
        dtool_core::document::Units::Cm => 720.0 / 25.4,
        dtool_core::document::Units::In => 72.0,
    }
}

pub fn bleed_region(doc: &Document, area: &ExportArea, region: (f32, f32, f32, f32), opts: &ExportOptions) -> Result<(f32, f32, f32, f32), ExportError> {
    if !opts.include_bleed {
        return Ok(region);
    }
    if !area.is_page_based() {
        return Err(ExportError::Unavailable("include_bleed".to_string()));
    }
    let [left, top, right, bottom] = bleed_insets(doc);
    Ok((region.0 - left, region.1 - top, region.2 + left + right, region.3 + top + bottom))
}

fn bleed_insets(doc: &Document) -> [f32; 4] {
    let bleed = doc.setup.bleed;
    if !bleed.enabled {
        return [0.0; 4];
    }
    [bleed.left, bleed.top, bleed.right, bleed.bottom].map(|value| {
        if value.is_finite() && value > 0.0 { value } else { 0.0 }
    })
}

fn lcms_intent(intent: RenderIntent) -> lcms2::Intent {
    match intent {
        RenderIntent::Perceptual => lcms2::Intent::Perceptual,
        RenderIntent::RelativeColorimetric => lcms2::Intent::RelativeColorimetric,
        RenderIntent::Saturation => lcms2::Intent::Saturation,
        RenderIntent::AbsoluteColorimetric => lcms2::Intent::AbsoluteColorimetric,
    }
}

pub struct OutputProfile {
    pub name: String,
    pub bytes: std::sync::Arc<[u8]>,
}

pub fn output_profile(opts: &ExportOptions) -> Result<Option<OutputProfile>, ExportError> {
    if opts.icc.is_none() && !opts.embed_icc {
        return Ok(None);
    }
    let srgb = dtool_core::colour::ProfileStore::SRGB;
    let name = opts.icc.clone().unwrap_or_else(|| srgb.to_string());
    let services = dtool_core::colour::services().read().map_err(|_| ExportError::Io(ERROR_FAILED.to_string()))?;
    let profile = services.profiles.find(&name).ok_or_else(|| ExportError::Unavailable("icc".to_string()))?;
    if profile.model != dtool_core::colour::ColourModel::Rgb {
        return Err(ExportError::Unavailable("icc".to_string()));
    }
    Ok(Some(OutputProfile { name: profile.name.clone(), bytes: profile.bytes.clone() }))
}

fn convert_to_profile(pixels: &mut [u8], channels: usize, profile: &OutputProfile, intent: RenderIntent) -> Result<(), ExportError> {
    if profile.name == dtool_core::colour::ProfileStore::SRGB {
        return Ok(());
    }
    let source = lcms2::Profile::new_srgb();
    let target = lcms2::Profile::new_icc(&profile.bytes).map_err(io_error)?;
    if channels == 4 {
        let transform: lcms2::Transform<[u8; 4], [u8; 4]> = lcms2::Transform::new(&source, lcms2::PixelFormat::RGBA_8, &target, lcms2::PixelFormat::RGBA_8, lcms_intent(intent)).map_err(io_error)?;
        let input: Vec<[u8; 4]> = pixels.chunks_exact(4).map(|px| [px[0], px[1], px[2], px[3]]).collect();
        let mut output = input.clone();
        transform.transform_pixels(&input, &mut output);
        for (dst, src) in pixels.chunks_exact_mut(4).zip(output) {
            dst.copy_from_slice(&src);
        }
    } else {
        let transform: lcms2::Transform<[u8; 3], [u8; 3]> = lcms2::Transform::new(&source, lcms2::PixelFormat::RGB_8, &target, lcms2::PixelFormat::RGB_8, lcms_intent(intent)).map_err(io_error)?;
        let input: Vec<[u8; 3]> = pixels.chunks_exact(3).map(|px| [px[0], px[1], px[2]]).collect();
        let mut output = input.clone();
        transform.transform_pixels(&input, &mut output);
        for (dst, src) in pixels.chunks_exact_mut(3).zip(output) {
            dst.copy_from_slice(&src);
        }
    }
    Ok(())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(data.len() + 12);
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(kind);
    chunk.extend_from_slice(data);
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    chunk.extend_from_slice(&crc32(&body).to_be_bytes());
    chunk
}

fn png_itxt(keyword: &str, value: &str) -> Vec<u8> {
    let mut data = keyword.as_bytes().to_vec();
    data.extend_from_slice(&[0, 0, 0, 0, 0]);
    data.extend_from_slice(value.as_bytes());
    png_chunk(b"iTXt", &data)
}

fn png_with_metadata(bytes: Vec<u8>, meta: &dtool_core::metadata::DocMetadata) -> Result<Vec<u8>, ExportError> {
    const IHDR_END: usize = 8 + 25;
    if bytes.len() < IHDR_END || &bytes[12..16] != b"IHDR" {
        return Err(ExportError::Io(ERROR_FAILED.to_string()));
    }
    let mut chunks = Vec::new();
    for (keyword, value) in [("Title", meta.title.as_str()), ("Author", meta.author.as_str()), ("Description", meta.description.as_str()), ("Copyright", meta.copyright.as_str())] {
        if !value.is_empty() {
            chunks.extend(png_itxt(keyword, value));
        }
    }
    if !meta.keywords.is_empty() {
        chunks.extend(png_itxt("Keywords", &meta.keywords.join(", ")));
    }
    chunks.extend(png_itxt("XML:com.adobe.xmp", &dtool_core::metadata::write_xmp(meta)));
    let mut out = bytes[..IHDR_END].to_vec();
    out.extend(chunks);
    out.extend_from_slice(&bytes[IHDR_END..]);
    Ok(out)
}

fn jpeg_with_xmp(bytes: Vec<u8>, meta: &dtool_core::metadata::DocMetadata) -> Result<Vec<u8>, ExportError> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err(ExportError::Io(ERROR_FAILED.to_string()));
    }
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend_from_slice(dtool_core::metadata::write_xmp(meta).as_bytes());
    let length = u16::try_from(payload.len() + 2).map_err(|_| ExportError::Unavailable("embed_metadata".to_string()))?;
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF && (bytes[at + 1] == 0xE0 || bytes[at + 1] == 0xE2) {
        at += 2 + usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
    }
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend(payload);
    out.extend_from_slice(&bytes[at..]);
    Ok(out)
}

fn riff_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = kind.to_vec();
    chunk.extend_from_slice(&(data.len() as u32).to_le_bytes());
    chunk.extend_from_slice(data);
    if data.len() % 2 == 1 {
        chunk.push(0);
    }
    chunk
}

fn webp_with(bytes: Vec<u8>, size: (u32, u32), alpha: bool, icc: Option<&[u8]>, xmp: Option<&[u8]>) -> Result<Vec<u8>, ExportError> {
    if icc.is_none() && xmp.is_none() {
        return Ok(bytes);
    }
    if bytes.len() < 20 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(ExportError::Io(ERROR_FAILED.to_string()));
    }
    let mut chunks: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let kind = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        let len = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let end = (at + 8 + len).min(bytes.len());
        chunks.push((kind, bytes[at + 8..end].to_vec()));
        at = at + 8 + len + (len % 2);
    }
    let mut flags = 0u8;
    if icc.is_some() { flags |= 0x20; }
    if xmp.is_some() { flags |= 0x04; }
    let vp8x = match chunks.iter().position(|(kind, _)| kind == b"VP8X") {
        Some(index) => {
            let mut data = chunks.remove(index).1;
            if let Some(first) = data.first_mut() { *first |= flags; }
            data
        }
        None => {
            if alpha { flags |= 0x10; }
            let (w, h) = (size.0.saturating_sub(1), size.1.saturating_sub(1));
            vec![flags, 0, 0, 0, w as u8, (w >> 8) as u8, (w >> 16) as u8, h as u8, (h >> 8) as u8, (h >> 16) as u8]
        }
    };
    let mut body = b"WEBP".to_vec();
    body.extend(riff_chunk(b"VP8X", &vp8x));
    if let Some(icc) = icc {
        body.extend(riff_chunk(b"ICCP", icc));
    }
    for (kind, data) in chunks.iter().filter(|(kind, _)| kind != b"ICCP" && kind != b"XMP ") {
        body.extend(riff_chunk(kind, data));
    }
    if let Some(xmp) = xmp {
        body.extend(riff_chunk(b"XMP ", xmp));
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    Ok(out)
}

pub fn encode_document(doc: &Document, img: &RgbaImage, opts: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    encode_with(img, opts, Some(&doc.metadata))
}

pub fn encode_preview_document(doc: &Document, img: &RgbaImage, opts: &ExportOptions) -> Result<EncodedPreview, ExportError> {
    let bytes = encode_document(doc, img, opts)?;
    let image = decode(&bytes, opts.format)?;
    Ok(EncodedPreview { bytes, image })
}

#[cfg(windows)]
#[path = "pdf_preview.rs"]
mod pdf_preview;

pub fn encode_pdf_preview(doc: &Document, opts: &ExportOptions) -> Result<EncodedPreview, ExportError> {
    encode_pdf_preview_at(doc, opts, 0)
}

pub fn encode_pdf_preview_at(doc: &Document, opts: &ExportOptions, page_index: u32) -> Result<EncodedPreview, ExportError> {
    let bytes = encode_output(doc, opts, std::path::Path::new(""))?.into_bytes();
    let image = render_pdf_preview_page(&bytes, doc, opts, page_index)?;
    Ok(EncodedPreview { bytes, image })
}

pub fn render_pdf_preview_page(bytes: &[u8], doc: &Document, opts: &ExportOptions, page_index: u32) -> Result<RgbaImage, ExportError> {
    #[cfg(windows)]
    {
        // The PDF page already incorporates the export dimensions.
        let preview = ExportOptions { width: None, height: None, scale: None, ..opts.clone() };
        return pdf_preview::render_page(bytes, page_index, doc.dpi, &preview);
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        let mut preview_options = opts.clone();
        preview_options.icc = None;
        preview_options.embed_icc = false;
        preview_options.embed_metadata = false;
        if opts.area.limits_objects_to_page() {
            let areas = page_run_areas(doc, &opts.area, opts.pages.as_deref());
            let area = areas.get(page_index as usize)
                .ok_or_else(|| ExportError::InvalidArea(area_name(&opts.area)))?;
            let region = bleed_region(doc, area, area_region(doc, area)?, &preview_options)?;
            render_region_image(doc, area, region, &preview_options)
        } else if page_index == 0 {
            render_preview(doc, &preview_options)
        } else {
            Err(ExportError::InvalidArea("pages".into()))
        }
    }
}

pub fn encode_svg_preview(doc: &Document, opts: &ExportOptions) -> Result<EncodedPreview, ExportError> {
    use std::sync::{Arc, OnceLock};

    let encoded = encode_output(doc, opts, std::path::Path::new(""))?;
    let region = area_region(doc, &opts.area)?;
    let per_unit = document_unit_points(doc) * doc.dpi.max(1.0) / 72.0;
    let (width, height) = target_size((0.0, 0.0, region.2 * per_unit, region.3 * per_unit), doc.dpi, opts)?;
    static FONT_DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    let fontdb = FONT_DB.get_or_init(|| {
        let mut fonts = fontdb::Database::new();
        fonts.load_system_fonts();
        fonts.set_serif_family("Times New Roman");
        fonts.set_sans_serif_family("Arial");
        fonts.set_monospace_family("Courier New");
        Arc::new(fonts)
    });
    let options = resvg::usvg::Options { fontdb: Arc::clone(fontdb), ..Default::default() };
    let tree = resvg::usvg::Tree::from_data(&encoded.bytes, &options).map_err(io_error)?;
    let size = tree.size();
    let transform = resvg::tiny_skia::Transform::from_scale(width as f32 / size.width(), height as f32 / size.height());
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| ExportError::Unavailable("size".into()))?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let pixels = pixmap.pixels().iter().flat_map(|pixel| {
        let colour = pixel.demultiply();
        [colour.red(), colour.green(), colour.blue(), colour.alpha()]
    }).collect();
    let image = RgbaImage::from_raw(width, height, pixels).ok_or_else(|| ExportError::Io(ERROR_FAILED.into()))?;
    Ok(EncodedPreview { bytes: encoded.bytes, image })
}

pub fn render_preview(doc: &Document, opts: &ExportOptions) -> Result<RgbaImage, ExportError> {
    render_area(doc, &opts.area, opts)?
        .into_iter()
        .next()
        .map(|(_, image)| image)
        .ok_or_else(|| ExportError::Io(ERROR_FAILED.to_string()))
}

fn matte_for(opts: &ExportOptions) -> Option<[u8; 3]> {
    match opts.matte {
        Some(colour) => Some(colour.rgb()),
        None if !opts.transparent => Some([255, 255, 255]),
        None => None,
    }
}

fn io_error<E: std::fmt::Display>(error: E) -> ExportError {
    ExportError::Io(error.to_string())
}

pub fn encode(img: &RgbaImage, opts: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    encode_with(img, opts, None)
}

fn encode_with(img: &RgbaImage, opts: &ExportOptions, meta: Option<&dtool_core::metadata::DocMetadata>) -> Result<Vec<u8>, ExportError> {
    opts.validate()?;
    if opts.embed_metadata && meta.is_none() {
        return Err(ExportError::Unavailable("embed_metadata".to_string()));
    }
    let meta = meta.filter(|_| opts.embed_metadata);
    let profile = output_profile(opts)?;
    let embed = profile.as_ref().filter(|_| opts.embed_icc).map(|profile| profile.bytes.to_vec());
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return Err(ExportError::Unavailable("size".to_string()));
    }
    let mut pixels = img.as_raw().clone();
    let mut buffer = Vec::new();
    match opts.format {
        ExportFormat::Jpeg => {
            let w = u16::try_from(width).map_err(|_| ExportError::Unavailable("width".to_string()))?;
            let h = u16::try_from(height).map_err(|_| ExportError::Unavailable("height".to_string()))?;
            let mut rgb = crate::flatten_rgba(&pixels, opts.matte.map(|c| c.rgb()).unwrap_or([255, 255, 255]));
            if let Some(profile) = &profile {
                convert_to_profile(&mut rgb, 3, profile, opts.intent)?;
            }
            let mut encoder = jpeg_encoder::Encoder::new(&mut buffer, opts.effective_quality());
            encoder.set_progressive(opts.progressive);
            if let Some(icc) = &embed {
                encoder.add_icc_profile(icc).map_err(io_error)?;
            }
            encoder.encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb).map_err(io_error)?;
            if let Some(meta) = meta {
                buffer = jpeg_with_xmp(buffer, meta)?;
            }
        }
        ExportFormat::Png => {
            use image::ImageEncoder;
            if let Some(matte) = matte_for(opts) {
                apply_matte(&mut pixels, matte);
            }
            if let Some(profile) = &profile {
                convert_to_profile(&mut pixels, 4, profile, opts.intent)?;
            }
            let mut encoder = image::codecs::png::PngEncoder::new(&mut buffer);
            if let Some(icc) = &embed {
                encoder.set_icc_profile(icc.clone()).map_err(|_| ExportError::Unavailable("embed_icc".to_string()))?;
            }
            encoder.write_image(&pixels, width, height, image::ExtendedColorType::Rgba8).map_err(io_error)?;
            if let Some(meta) = meta {
                buffer = png_with_metadata(buffer, meta)?;
            }
        }
        ExportFormat::Webp => {
            if let Some(matte) = matte_for(opts) {
                apply_matte(&mut pixels, matte);
            }
            if let Some(profile) = &profile {
                convert_to_profile(&mut pixels, 4, profile, opts.intent)?;
            }
            buffer = crate::webp::encode_rgba(width, height, &pixels, opts.lossless, opts.effective_quality()).map_err(|error| match error {
                crate::webp::WebpEncodeError::LossyUnavailable => ExportError::Unavailable("webp_lossy".to_string()),
                crate::webp::WebpEncodeError::Failed(message) => ExportError::Io(message),
            })?;
            let alpha = pixels.chunks_exact(4).any(|px| px[3] != 255);
            let xmp = meta.map(|meta| dtool_core::metadata::write_xmp(meta).into_bytes());
            buffer = webp_with(buffer, (width, height), alpha, embed.as_deref(), xmp.as_deref())?;
        }
        ExportFormat::Tiff => {
            if let Some(matte) = matte_for(opts) {
                apply_matte(&mut pixels, matte);
            }
            use image::ImageEncoder;
            let mut cursor = std::io::Cursor::new(Vec::new());
            image::codecs::tiff::TiffEncoder::new(&mut cursor)
                .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
                .map_err(io_error)?;
            buffer = cursor.into_inner();
        }
        ExportFormat::Gif => {
            if let Some(matte) = matte_for(opts) {
                apply_matte(&mut pixels, matte);
            }
            {
                let mut encoder = image::codecs::gif::GifEncoder::new(&mut buffer);
                encoder
                    .encode(&pixels, width, height, image::ExtendedColorType::Rgba8)
                    .map_err(io_error)?;
            }
        }
        other => return Err(ExportError::Unavailable(format_name(other).to_string())),
    }
    Ok(buffer)
}

pub fn image_format(format: ExportFormat) -> Option<image::ImageFormat> {
    match format {
        ExportFormat::Png => Some(image::ImageFormat::Png),
        ExportFormat::Jpeg => Some(image::ImageFormat::Jpeg),
        ExportFormat::Webp => Some(image::ImageFormat::WebP),
        ExportFormat::Tiff => Some(image::ImageFormat::Tiff),
        ExportFormat::Gif => Some(image::ImageFormat::Gif),
        _ => None,
    }
}

pub fn decode(bytes: &[u8], format: ExportFormat) -> Result<RgbaImage, ExportError> {
    let kind = image_format(format).ok_or_else(|| ExportError::Unavailable(format_name(format).to_string()))?;
    image::load_from_memory_with_format(bytes, kind)
        .map(|decoded| decoded.to_rgba8())
        .map_err(io_error)
}

#[derive(Debug, Clone)]
pub struct EncodedPreview {
    pub bytes: Vec<u8>,
    pub image: RgbaImage,
}

impl EncodedPreview {
    pub fn estimated_size(&self) -> u64 {
        self.bytes.len() as u64
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.image.dimensions()
    }

    pub fn thumbnail(&self, width: u32, height: u32) -> RgbaImage {
        image::imageops::thumbnail(&self.image, width, height)
    }
}

pub fn encode_preview(img: &RgbaImage, opts: &ExportOptions) -> Result<EncodedPreview, ExportError> {
    let bytes = encode(img, opts)?;
    let image = decode(&bytes, opts.format)?;
    Ok(EncodedPreview { bytes, image })
}

pub fn estimate_size(img: &RgbaImage, opts: &ExportOptions) -> Option<u64> {
    encode(img, opts).ok().map(|bytes| bytes.len() as u64)
}

pub fn write_encoded(path: &std::path::Path, bytes: &[u8], overwrite: bool) -> Result<crate::ExportReceipt, ExportError> {
    if path.as_os_str().is_empty() {
        return Err(ExportError::Io("path".to_string()));
    }
    if !overwrite && path.exists() {
        return Err(ExportError::Collision(path.to_path_buf()));
    }
    crate::write_atomic_with_policy(path, bytes, overwrite).map_err(|error| {
        if !overwrite && error.kind() == std::io::ErrorKind::AlreadyExists {
            ExportError::Collision(path.to_path_buf())
        } else {
            io_error(error)
        }
    })?;
    Ok(crate::ExportReceipt {
        path: path.to_path_buf(),
        bytes: bytes.len() as u64,
        warnings: Vec::new(),
    })
}

pub struct EncodedExport {
    bytes: Vec<u8>,
    warnings: Vec<String>,
}

impl From<Vec<u8>> for EncodedExport {
    fn from(bytes: Vec<u8>) -> Self {
        Self { bytes, warnings: Vec::new() }
    }
}

impl EncodedExport {
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn write(self, path: &std::path::Path, overwrite: bool) -> Result<crate::ExportReceipt, ExportError> {
        let mut receipt = write_encoded(path, &self.bytes, overwrite)?;
        receipt.warnings = self.warnings;
        Ok(receipt)
    }
}

pub fn encode_output(doc: &Document, options: &ExportOptions, path: &std::path::Path) -> Result<EncodedExport, ExportError> {
    options.validate()?;
    let request = ExportRequest {
        format: options.format,
        area: options.area.clone(),
        pages: options.pages.clone(),
        path: path.to_path_buf(),
        preset: None,
        options: serde_json::Value::Null,
    };
    let control = ExportControl::new();
    let encoded = match options.format {
        ExportFormat::Pdf => {
            let mut pdf = crate::pdf::PdfOptions {
                colour_space: options.pdf_colour_space.unwrap_or_default().label().to_string(),
                include_bleed: options.include_bleed,
                embed_icc: options.embed_icc,
                embed_metadata: options.embed_metadata,
                profile: options.icc.clone(),
                raster_dpi_override: options.pdf_raster_dpi,
                fonts: if options.pdf_outline_text { crate::pdf::PdfFonts::TextAsCurves } else { crate::pdf::PdfFonts::All },
                outline_strokes: options.pdf_outline_strokes,
                include_hyperlinks: options.pdf_include_hyperlinks,
                include_bookmarks: options.pdf_include_bookmarks,
                scale: pdf_scale(doc, options)?,
                ..Default::default()
            };
            pdf.printers_marks = options.pdf_crop_marks || options.pdf_fold_marks;
            pdf.marks.crop = options.pdf_crop_marks;
            pdf.marks.fold = options.pdf_fold_marks;
            crate::pdf::encode_pdf(doc, &request, &pdf, &control)
        }
        ExportFormat::Svg => crate::svg::encode_svg(doc, &request, &Default::default(), &control),
        ExportFormat::Eps => crate::eps::encode_eps(doc, &request, &control),
        _ => return render_preview(doc, options).and_then(|image| encode_document(doc, &image, options)).map(EncodedExport::from),
    };
    encoded.map(|(bytes, warnings)| EncodedExport { bytes, warnings }).map_err(|error| match error.code.as_str() {
        ERROR_UNAVAILABLE => ExportError::Unavailable(error.message_key),
        "INVALID_PARAMS" => ExportError::InvalidArea(error.message_key),
        "NOT_FOUND" => ExportError::NotFound(error.message_key),
        "CANCELLED" => ExportError::Cancelled,
        _ => ExportError::Io(error.code),
    })
}

fn pdf_scale(doc: &Document, options: &ExportOptions) -> Result<f32, ExportError> {
    if options.width.is_none() && options.height.is_none() {
        return Ok(options.scale.unwrap_or(1.0));
    }
    let region = if options.area.limits_objects_to_page() && doc.sections().iter().any(|section| !section.include_on_export) {
        let batches = crate::output_page_batches(doc, &options.area, options.pages.as_deref());
        crate::pdf_regions_for_batches(doc, &batches).and_then(|regions| regions.first().copied())
            .ok_or_else(|| ExportError::InvalidArea(area_name(&options.area)))?
    } else {
        area_region(doc, &options.area)?
    };
    let (_, _, width, height) = region;
    let pixels_per_unit = document_unit_points(doc) * doc.dpi.max(1.0) / 72.0;
    let factor = options.width.map(|value| value as f32 / (width * pixels_per_unit))
        .or_else(|| options.height.map(|value| value as f32 / (height * pixels_per_unit)))
        .unwrap_or(1.0);
    if factor.is_finite() && factor > 0.0 { Ok(factor) }
    else { Err(ExportError::InvalidArea("size".to_string())) }
}

pub const PREVIEW_ZOOM_MIN: f32 = 0.25;
pub const PREVIEW_ZOOM_MAX: f32 = 8.0;

pub fn clamp_preview_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(PREVIEW_ZOOM_MIN, PREVIEW_ZOOM_MAX)
    } else {
        1.0
    }
}

pub fn pixels_equal(a: &RgbaImage, b: &RgbaImage) -> bool {
    a.dimensions() == b.dimensions() && a.as_raw() == b.as_raw()
}

pub fn decode_file(path: &std::path::Path, format: ExportFormat) -> Result<RgbaImage, ExportError> {
    let bytes = std::fs::read(path).map_err(io_error)?;
    decode(&bytes, format)
}

impl EncodedPreview {
    pub fn matches_bytes(&self, bytes: &[u8], format: ExportFormat) -> Result<bool, ExportError> {
        if format == ExportFormat::Svg {
            return Ok(self.bytes.as_slice() == bytes);
        }
        Ok(pixels_equal(&self.image, &decode(bytes, format)?))
    }

    pub fn matches_file(&self, path: &std::path::Path, format: ExportFormat) -> Result<bool, ExportError> {
        Ok(pixels_equal(&self.image, &decode_file(path, format)?))
    }

    pub fn tiles(&self, max_side: u32) -> Vec<[u32; 4]> {
        let (width, height) = self.dimensions();
        preview_tiles(width, height, max_side)
    }

    pub fn tile_image(&self, tile: [u32; 4]) -> RgbaImage {
        crop_rgba(&self.image, tile)
    }
}

pub fn preview_tiles(width: u32, height: u32, max_side: u32) -> Vec<[u32; 4]> {
    let side = max_side.max(1);
    let mut tiles = Vec::new();
    let mut y = 0;
    while y < height {
        let h = side.min(height - y);
        let mut x = 0;
        while x < width {
            let w = side.min(width - x);
            tiles.push([x, y, w, h]);
            x += w;
        }
        y += h;
    }
    tiles
}

pub fn crop_rgba(img: &RgbaImage, tile: [u32; 4]) -> RgbaImage {
    let (width, height) = img.dimensions();
    let x = tile[0].min(width);
    let y = tile[1].min(height);
    let w = tile[2].min(width - x);
    let h = tile[3].min(height - y);
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

pub fn preview_pixel(preview: &EncodedPreview, view: [f32; 2], pan: [f32; 2], zoom: f32) -> Option<(u32, u32)> {
    let zoom = clamp_preview_zoom(zoom);
    let mapped = dtool_core::view_to_doc(&dtool_core::ViewState { zoom, scroll: pan, ..dtool_core::ViewState::default() }, view);
    let x = mapped[0].floor();
    let y = mapped[1].floor();
    let (width, height) = preview.dimensions();
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 || x >= width as f32 || y >= height as f32 {
        return None;
    }
    Some((x as u32, y as u32))
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| image::Rgba([(x * 7 % 256) as u8, (y * 13 % 256) as u8, ((x + y) * 5 % 256) as u8, 255]))
    }

    fn options(format: ExportFormat, quality: u8) -> ExportOptions {
        ExportOptions { quality, ..ExportOptions::for_format(format) }
    }

    #[test]
    fn jpeg_preview_shows_encoded_artefacts() {
        let source = gradient(64, 48);
        let preview = encode_preview(&source, &options(ExportFormat::Jpeg, 10)).unwrap();
        assert!(!pixels_equal(&preview.image, &source));
        assert!(preview.matches_bytes(&preview.bytes, ExportFormat::Jpeg).unwrap());
    }

    #[test]
    fn preview_equals_written_file() {
        let source = gradient(40, 30);
        let dir = std::env::temp_dir().join(format!("dtool_b54_t011_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for format in EXPOSED_RASTER_FORMATS {
            let preview = encode_preview(&source, &options(format, 35)).unwrap();
            let path = dir.join(format!("preview.{}", format_extension(format)));
            write_encoded(&path, &preview.bytes, true).unwrap();
            assert!(preview.matches_file(&path, format).unwrap());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn estimate_tracks_quality() {
        let source = gradient(96, 96);
        let low = encode_preview(&source, &options(ExportFormat::Jpeg, 10)).unwrap();
        let high = encode_preview(&source, &options(ExportFormat::Jpeg, 100)).unwrap();
        assert!(low.estimated_size() < high.estimated_size());
        assert_eq!(estimate_size(&source, &options(ExportFormat::Jpeg, 10)), Some(low.estimated_size()));
    }

    #[test]
    fn tiles_cover_preview_exactly() {
        let source = gradient(37, 21);
        let preview = encode_preview(&source, &options(ExportFormat::Png, 100)).unwrap();
        let tiles = preview.tiles(16);
        assert_eq!(tiles.len(), 6);
        let area: u64 = tiles.iter().map(|t| u64::from(t[2]) * u64::from(t[3])).sum();
        assert_eq!(area, 37 * 21);
        let mut rebuilt = RgbaImage::new(37, 21);
        for tile in &tiles {
            image::imageops::replace(&mut rebuilt, &preview.tile_image(*tile), i64::from(tile[0]), i64::from(tile[1]));
        }
        assert!(pixels_equal(&rebuilt, &preview.image));
    }

    #[test]
    fn zoom_maps_view_to_file_pixels() {
        let source = gradient(20, 10);
        let preview = encode_preview(&source, &options(ExportFormat::Png, 100)).unwrap();
        assert_eq!(preview_pixel(&preview, [3.0, 4.0], [0.0, 0.0], 1.0), Some((3, 4)));
        assert_eq!(preview_pixel(&preview, [9.0, 9.0], [-8.0, 0.0], 8.0), Some((2, 1)));
        assert_eq!(preview_pixel(&preview, [4.0, 0.0], [0.0, 0.0], 0.25), Some((16, 0)));
        assert_eq!(preview_pixel(&preview, [80.0, 0.0], [0.0, 0.0], 1.0), None);
        assert_eq!(clamp_preview_zoom(100.0), PREVIEW_ZOOM_MAX);
        assert_eq!(clamp_preview_zoom(0.01), PREVIEW_ZOOM_MIN);
    }
}
pub const EXPORT_JOB_RUN: &str = "export.job.run";
pub const EXPORT_JOB_LIST: &str = "export.job.list";
pub const EXPORT_JOB_DELETE: &str = "export.job.delete";
pub const EXPORT_JOB_COMMANDS: [&str; 3] = [EXPORT_JOB_RUN, EXPORT_JOB_LIST, EXPORT_JOB_DELETE];
pub const ERROR_PARTIAL_KEY: &str = "error.export.partial";
pub const ERROR_FAILED_KEY: &str = "error.export.failed";
pub const ERROR_CANCELLED_KEY: &str = "error.export.cancelled";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverwritePolicy {
    #[default]
    Ask,
    Overwrite,
    Skip,
}

impl OverwritePolicy {
    pub fn parse(value: Option<&serde_json::Value>) -> Result<Self, ExportError> {
        match value {
            None | Some(serde_json::Value::Null) => Ok(OverwritePolicy::Ask),
            Some(serde_json::Value::Bool(true)) => Ok(OverwritePolicy::Overwrite),
            Some(serde_json::Value::Bool(false)) => Ok(OverwritePolicy::Ask),
            Some(serde_json::Value::String(name)) => match name.as_str() {
                "ask" => Ok(OverwritePolicy::Ask),
                "overwrite" | "replace" => Ok(OverwritePolicy::Overwrite),
                "skip" => Ok(OverwritePolicy::Skip),
                _ => Err(ExportError::Unavailable("overwrite".to_string())),
            },
            Some(_) => Err(ExportError::Unavailable("overwrite".to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwriteDecision {
    Overwrite,
    Skip,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ExportJobItem {
    pub id: u64,
    pub name: String,
    pub path: std::path::PathBuf,
    pub options: ExportOptions,
}

impl ExportJobItem {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "path": self.path.to_string_lossy(),
            "format": format_name(self.options.format),
            "options": serde_json::to_value(&self.options).unwrap_or(serde_json::Value::Null),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobItemStatus {
    Written,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct JobItemResult {
    pub id: u64,
    pub path: std::path::PathBuf,
    pub status: JobItemStatus,
    pub bytes: u64,
    pub code: Option<String>,
    pub message_key: Option<String>,
    pub warnings: Vec<String>,
}

impl JobItemResult {
    fn written(item: &ExportJobItem, receipt: &crate::ExportReceipt) -> Self {
        JobItemResult { id: item.id, path: receipt.path.clone(), status: JobItemStatus::Written, bytes: receipt.bytes, code: None, message_key: None, warnings: receipt.warnings.clone() }
    }

    fn skipped(item: &ExportJobItem, code: &str, key: &str) -> Self {
        JobItemResult { id: item.id, path: item.path.clone(), status: JobItemStatus::Skipped, bytes: 0, code: Some(code.to_string()), message_key: Some(key.to_string()), warnings: Vec::new() }
    }

    fn failed(item: &ExportJobItem, error: &ExportError) -> Self {
        JobItemResult { id: item.id, path: item.path.clone(), status: JobItemStatus::Failed, bytes: 0, code: Some(error.code().to_string()), message_key: Some(error.message_key().to_string()), warnings: Vec::new() }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "path": self.path.to_string_lossy(),
            "status": self.status,
            "bytes": self.bytes,
            "code": self.code,
            "message_key": self.message_key,
            "warnings": self.warnings,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobReceipt {
    pub results: Vec<JobItemResult>,
    pub cancelled: bool,
}

impl JobReceipt {
    fn with_status(&self, status: JobItemStatus) -> Vec<&JobItemResult> {
        self.results.iter().filter(|r| r.status == status).collect()
    }

    pub fn written(&self) -> Vec<&JobItemResult> {
        self.with_status(JobItemStatus::Written)
    }

    pub fn skipped(&self) -> Vec<&JobItemResult> {
        self.with_status(JobItemStatus::Skipped)
    }

    pub fn failed(&self) -> Vec<&JobItemResult> {
        self.with_status(JobItemStatus::Failed)
    }

    pub fn is_partial(&self) -> bool {
        !self.written().is_empty() && (!self.failed().is_empty() || self.cancelled)
    }

    pub fn is_success(&self) -> bool {
        self.failed().is_empty() && !self.cancelled
    }

    pub fn code(&self) -> Option<&'static str> {
        if !self.failed().is_empty() {
            Some(ERROR_FAILED)
        } else if self.cancelled {
            Some("CANCELLED")
        } else {
            None
        }
    }

    pub fn message_key(&self) -> Option<&'static str> {
        if self.is_partial() {
            Some(ERROR_PARTIAL_KEY)
        } else if !self.failed().is_empty() {
            Some(ERROR_FAILED_KEY)
        } else if self.cancelled {
            Some(ERROR_CANCELLED_KEY)
        } else {
            None
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let list = |items: Vec<&JobItemResult>| items.into_iter().map(|r| r.to_json()).collect::<Vec<_>>();
        serde_json::json!({
            "ok": self.is_success(),
            "partial": self.is_partial(),
            "cancelled": self.cancelled,
            "code": self.code(),
            "message_key": self.message_key(),
            "written": list(self.written()),
            "skipped": list(self.skipped()),
            "failed": list(self.failed()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStep {
    Wrote(u64),
    Skipped(u64),
    Failed(u64),
    NeedsDecision(u64, std::path::PathBuf),
    Cancelled,
    Done,
}

#[derive(Debug, Clone, Default)]
pub struct ExportJob {
    pub items: Vec<ExportJobItem>,
    pub policy: OverwritePolicy,
    next_id: u64,
    cursor: usize,
    running: bool,
    cancel_requested: bool,
    receipt: JobReceipt,
}

impl ExportJob {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue(&mut self, name: &str, path: impl Into<std::path::PathBuf>, options: ExportOptions) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        let path = path.into();
        let name = if name.trim().is_empty() { path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() } else { name.to_string() };
        self.items.push(ExportJobItem { id, name, path, options });
        id
    }

    pub fn queue_json(&mut self, base: &ExportOptions, value: &serde_json::Value) -> Result<u64, ExportError> {
        let path = value.get("path").and_then(|v| v.as_str()).filter(|p| !p.trim().is_empty()).ok_or_else(|| ExportError::Io("path".to_string()))?;
        let name = value.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut patch = value.clone();
        if let Some(fields) = patch.as_object_mut() {
            fields.remove("path");
            fields.remove("name");
            fields.remove("id");
        }
        let options = base.merge_json(&patch)?;
        Ok(self.queue(&name, path, options))
    }

    pub fn list(&self) -> &[ExportJobItem] {
        &self.items
    }

    pub fn list_json(&self) -> serde_json::Value {
        serde_json::json!({
            "items": self.items.iter().map(|i| i.to_json()).collect::<Vec<_>>(),
            "running": self.running,
        })
    }

    pub fn delete(&mut self, id: u64) -> bool {
        if self.running {
            return false;
        }
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        before != self.items.len()
    }

    pub fn delete_json(&mut self, params: &serde_json::Value) -> serde_json::Value {
        let mut ids: Vec<u64> = params.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
        if let Some(id) = params.get("id").and_then(|v| v.as_u64()) {
            ids.push(id);
        }
        let all = params.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
        let deleted: Vec<u64> = if all && !self.running {
            let gone = self.items.iter().map(|i| i.id).collect();
            self.items.clear();
            gone
        } else {
            ids.into_iter().filter(|id| self.delete(*id)).collect()
        };
        serde_json::json!({ "deleted": deleted, "items": self.list_json()["items"].clone() })
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn start(&mut self, policy: OverwritePolicy) {
        self.policy = policy;
        self.cursor = 0;
        self.running = !self.items.is_empty();
        self.cancel_requested = false;
        self.receipt = JobReceipt::default();
    }

    pub fn request_cancel(&mut self) {
        if self.running {
            self.cancel_requested = true;
        }
    }

    pub fn current(&self) -> Option<&ExportJobItem> {
        if self.running { self.items.get(self.cursor) } else { None }
    }

    pub fn receipt(&self) -> &JobReceipt {
        &self.receipt
    }

    fn finish(&mut self) -> JobReceipt {
        self.running = false;
        let done: Vec<u64> = self.receipt.results.iter().filter(|r| r.status == JobItemStatus::Written).map(|r| r.id).collect();
        self.items.retain(|item| !done.contains(&item.id));
        self.cursor = 0;
        self.receipt.clone()
    }

    fn cancel_rest(&mut self) {
        self.receipt.cancelled = true;
        for item in &self.items[self.cursor..] {
            self.receipt.results.push(JobItemResult::skipped(item, "CANCELLED", ERROR_CANCELLED_KEY));
        }
        self.cursor = self.items.len();
    }

    pub fn step<P, R>(&mut self, decision: Option<OverwriteDecision>, produce: &mut P) -> JobStep
    where
        P: FnMut(&ExportJobItem) -> Result<R, ExportError>,
        R: Into<EncodedExport>,
    {
        if !self.running {
            return JobStep::Done;
        }
        if self.cancel_requested {
            self.cancel_rest();
            self.finish();
            return JobStep::Cancelled;
        }
        let Some(item) = self.items.get(self.cursor).cloned() else {
            self.finish();
            return JobStep::Done;
        };
        let exists = item.path.exists();
        let overwrite = match (exists, decision, self.policy) {
            (false, _, _) => false,
            (true, Some(OverwriteDecision::Overwrite), _) => true,
            (true, Some(OverwriteDecision::Skip), _) => {
                self.receipt.results.push(JobItemResult::skipped(&item, ERROR_COLLISION, "error.export.collision"));
                self.cursor += 1;
                return JobStep::Skipped(item.id);
            }
            (true, Some(OverwriteDecision::Cancel), _) => {
                self.cancel_rest();
                self.finish();
                return JobStep::Cancelled;
            }
            (true, None, OverwritePolicy::Overwrite) => true,
            (true, None, OverwritePolicy::Skip) => {
                self.receipt.results.push(JobItemResult::skipped(&item, ERROR_COLLISION, "error.export.collision"));
                self.cursor += 1;
                return JobStep::Skipped(item.id);
            }
            (true, None, OverwritePolicy::Ask) => return JobStep::NeedsDecision(item.id, item.path.clone()),
        };
        self.cursor += 1;
        match produce(&item).and_then(|encoded| encoded.into().write(&item.path, overwrite)) {
            Ok(receipt) => {
                self.receipt.results.push(JobItemResult::written(&item, &receipt));
                JobStep::Wrote(item.id)
            }
            Err(ExportError::Collision(_)) => {
                self.receipt.results.push(JobItemResult::skipped(&item, ERROR_COLLISION, "error.export.collision"));
                JobStep::Skipped(item.id)
            }
            Err(error) => {
                self.receipt.results.push(JobItemResult::failed(&item, &error));
                JobStep::Failed(item.id)
            }
        }
    }

    pub fn run_with<P, R, D, C>(&mut self, policy: OverwritePolicy, mut produce: P, mut decide: D, mut cancel: C) -> JobReceipt
    where
        P: FnMut(&ExportJobItem) -> Result<R, ExportError>,
        R: Into<EncodedExport>,
        D: FnMut(&ExportJobItem) -> OverwriteDecision,
        C: FnMut() -> bool,
    {
        self.start(policy);
        let mut decision = None;
        loop {
            match self.step(decision.take(), &mut produce) {
                JobStep::Done | JobStep::Cancelled => break,
                JobStep::NeedsDecision(_, _) => {
                    let answer = self.current().map(|item| decide(item)).unwrap_or(OverwriteDecision::Skip);
                    decision = Some(answer);
                }
                _ => {
                    if cancel() {
                        self.request_cancel();
                    }
                }
            }
        }
        self.receipt.clone()
    }

    pub fn run_document(&mut self, doc: &Document, policy: OverwritePolicy) -> JobReceipt {
        let decide_skip = |_: &ExportJobItem| OverwriteDecision::Skip;
        self.run_with(policy, |item| encode_output(doc, &item.options, &item.path), decide_skip, || false)
    }
}

pub fn encode_item(doc: &Document, item: &ExportJobItem) -> Result<Vec<u8>, ExportError> {
    encode_output(doc, &item.options, &item.path).map(|encoded| encoded.bytes)
}

#[cfg(test)]
mod job_tests {
    use super::*;

    fn bytes_for(item: &ExportJobItem) -> Result<Vec<u8>, ExportError> {
        let img = RgbaImage::from_fn(8, 8, |x, y| image::Rgba([(x * 30) as u8, (y * 30) as u8, 90, 255]));
        encode(&img, &item.options)
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dtool_b54_t012_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_read_only(path: &std::path::Path, read_only: bool) {
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_readonly(read_only);
        std::fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn read_only_target_gives_partial_receipt() {
        let dir = temp_dir("partial");
        let locked = dir.join("b.png");
        std::fs::write(&locked, b"old").unwrap();
        set_read_only(&locked, true);
        let mut job = ExportJob::new();
        let opts = ExportOptions::for_format(ExportFormat::Png);
        job.queue("a", dir.join("a.png"), opts.clone());
        job.queue("b", &locked, opts.clone());
        job.queue("c", dir.join("c.png"), opts);
        let receipt = job.run_with(OverwritePolicy::Ask, bytes_for, |_| OverwriteDecision::Overwrite, || false);
        set_read_only(&locked, false);
        assert_eq!(receipt.written().len(), 2);
        assert_eq!(receipt.failed().len(), 1);
        assert!(receipt.skipped().is_empty());
        assert!(receipt.is_partial());
        assert_eq!(receipt.code(), Some(ERROR_FAILED));
        assert_eq!(receipt.message_key(), Some(ERROR_PARTIAL_KEY));
        assert_eq!(receipt.to_json()["written"].as_array().unwrap().len(), 2);
        assert_eq!(std::fs::read(&locked).unwrap(), b"old");
        assert_eq!(job.list().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ask_before_overwrite_and_skip() {
        let dir = temp_dir("ask");
        let existing = dir.join("a.png");
        std::fs::write(&existing, b"keep").unwrap();
        let mut job = ExportJob::new();
        job.queue("a", &existing, ExportOptions::for_format(ExportFormat::Png));
        job.start(OverwritePolicy::Ask);
        let mut produce = bytes_for;
        assert_eq!(job.step(None, &mut produce), JobStep::NeedsDecision(1, existing.clone()));
        assert_eq!(job.step(Some(OverwriteDecision::Skip), &mut produce), JobStep::Skipped(1));
        assert_eq!(job.step(None, &mut produce), JobStep::Done);
        assert_eq!(job.receipt().skipped().len(), 1);
        assert_eq!(std::fs::read(&existing).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_stops_after_current_file() {
        let dir = temp_dir("cancel");
        let mut job = ExportJob::new();
        for name in ["a", "b", "c"] {
            job.queue(name, dir.join(format!("{}.png", name)), ExportOptions::for_format(ExportFormat::Png));
        }
        let receipt = job.run_with(OverwritePolicy::Ask, bytes_for, |_| OverwriteDecision::Skip, || true);
        assert!(receipt.cancelled);
        assert_eq!(receipt.written().len(), 1);
        assert_eq!(receipt.skipped().len(), 2);
        assert!(dir.join("a.png").exists());
        assert!(!dir.join("b.png").exists());
        assert_eq!(receipt.message_key(), Some(ERROR_PARTIAL_KEY));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_and_delete_queued_items() {
        let mut job = ExportJob::new();
        let base = ExportOptions::for_format(ExportFormat::Png);
        let a = job.queue_json(&base, &serde_json::json!({ "path": "a.jpg", "format": "jpeg", "quality": 40 })).unwrap();
        let b = job.queue_json(&base, &serde_json::json!({ "path": "b.png" })).unwrap();
        assert!(job.queue_json(&base, &serde_json::json!({ "format": "png" })).is_err());
        assert_eq!(job.list()[0].options.format, ExportFormat::Jpeg);
        assert_eq!(job.list_json()["items"].as_array().unwrap().len(), 2);
        let out = job.delete_json(&serde_json::json!({ "id": a }));
        assert_eq!(out["deleted"], serde_json::json!([a]));
        assert_eq!(job.list().len(), 1);
        assert_eq!(job.list()[0].id, b);
        assert!(!job.delete(a));
    }
}
