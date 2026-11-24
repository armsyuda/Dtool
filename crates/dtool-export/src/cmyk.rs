use std::io::Write;
use std::path::PathBuf;
use dtool_protocol::{CommandError, CommandResult};
use crate::{DocumentExportExt, ExportRequest, ExportReceipt, ExportControl};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmykOptions {
    pub profile: Option<String>,
    pub intent: String,
    pub bit_depth: u8,
}

impl Default for CmykOptions {
    fn default() -> Self {
        Self {
            profile: None,
            intent: "relative_colorimetric".to_string(),
            bit_depth: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmykProfileEntry {
    pub id: &'static str,
    pub icc_name: &'static str,
    pub label_key: &'static str,
    pub licence_key: &'static str,
    pub icc_file: &'static str,
    pub licence_file: &'static str,
    pub vendor_supplied: bool,
}

pub const ICC_ASSET_DIR: &str = "assets/icc";
pub const DEFAULT_CMYK_PROFILE_ID: &str = "fogra39l_coated";

pub const CMYK_PROFILES: [CmykProfileEntry; 2] = [
    CmykProfileEntry {
        id: "fogra39l_coated",
        icc_name: "FOGRA39L Coated",
        label_key: "colour.cmyk_profile.fogra39l",
        licence_key: "colour.cmyk_profile.licence.cc0",
        icc_file: "FOGRA39L_coated.icc",
        licence_file: "FOGRA39L_coated.LICENSE.txt",
        vendor_supplied: false,
    },
    CmykProfileEntry {
        id: "us_web_coated_swop_v2",
        icc_name: "U.S. Web Coated (SWOP) v2",
        label_key: "colour.cmyk_profile.swop_v2",
        licence_key: "colour.cmyk_profile.licence.adobe",
        icc_file: "USWebCoatedSWOP.icc",
        licence_file: "USWebCoatedSWOP.LICENSE.txt",
        vendor_supplied: true,
    },
];

pub const CMYK_PROFILE_NAMES: [&str; 2] = [CMYK_PROFILES[0].icc_name, CMYK_PROFILES[1].icc_name];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmykProfileStatus {
    pub entry: CmykProfileEntry,
    pub icc_path: PathBuf,
    pub licence_path: PathBuf,
    pub icc_present: bool,
    pub licence_present: bool,
    pub is_default: bool,
}

pub fn cmyk_profiles() -> &'static [CmykProfileEntry] {
    &CMYK_PROFILES
}

pub fn default_cmyk_profile() -> &'static CmykProfileEntry {
    find_cmyk_profile(DEFAULT_CMYK_PROFILE_ID).unwrap_or(&CMYK_PROFILES[0])
}

pub fn find_cmyk_profile(selection: &str) -> Option<&'static CmykProfileEntry> {
    let wanted = selection.trim();
    CMYK_PROFILES.iter().find(|entry| entry.id == wanted || entry.icc_name == wanted || entry.icc_file.eq_ignore_ascii_case(wanted))
}

pub fn icc_asset_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("DTOOL_ICC_DIR").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(std::path::Path::to_path_buf)) {
        dirs.push(exe_dir.join(ICC_ASSET_DIR));
    }
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("dtool-core").join(ICC_ASSET_DIR));
    dirs
}

fn locate_asset(file: &str) -> (PathBuf, bool) {
    let dirs = icc_asset_dirs();
    for dir in &dirs {
        let candidate = dir.join(file);
        if candidate.is_file() {
            return (candidate, true);
        }
    }
    let fallback = dirs.last().cloned().unwrap_or_else(|| PathBuf::from(ICC_ASSET_DIR));
    (fallback.join(file), false)
}

pub fn cmyk_profile_status(entry: &CmykProfileEntry) -> CmykProfileStatus {
    let (icc_path, icc_present) = locate_asset(entry.icc_file);
    let (licence_path, licence_present) = locate_asset(entry.licence_file);
    CmykProfileStatus {
        entry: *entry,
        icc_path,
        licence_path,
        icc_present,
        licence_present,
        is_default: entry.id == DEFAULT_CMYK_PROFILE_ID,
    }
}

pub fn cmyk_profile_list() -> Vec<CmykProfileStatus> {
    CMYK_PROFILES.iter().map(cmyk_profile_status).collect()
}

pub fn cmyk_profile_licence_path(selection: &str) -> Option<PathBuf> {
    let status = cmyk_profile_status(find_cmyk_profile(selection)?);
    status.licence_present.then_some(status.licence_path)
}

pub fn cmyk_profile_list_value() -> serde_json::Value {
    serde_json::Value::Array(cmyk_profile_list().into_iter().map(|status| serde_json::json!({
        "id": status.entry.id,
        "name": status.entry.icc_name,
        "label_key": status.entry.label_key,
        "licence_key": status.entry.licence_key,
        "icc_file": status.entry.icc_file,
        "licence_file": status.entry.licence_file,
        "available": status.icc_present && status.licence_present,
        "icc_present": status.icc_present,
        "licence_present": status.licence_present,
        "default": status.is_default,
    })).collect())
}

pub fn resolve_cmyk_profile(selection: Option<&str>) -> Result<PathBuf, CommandError> {
    let entry = match selection.map(str::trim).filter(|value| !value.is_empty()) {
        None => default_cmyk_profile(),
        Some(value) => match find_cmyk_profile(value) {
            Some(entry) => entry,
            None => {
                let path = PathBuf::from(value);
                return if path.is_file() { Ok(path) } else { Err(crate::export_error("NOT_FOUND", "error.io.not_found")) };
            }
        },
    };
    let status = cmyk_profile_status(entry);
    if status.icc_present && status.licence_present {
        Ok(status.icc_path)
    } else {
        Err(crate::export_error("NOT_FOUND", "error.colour.cmyk_profile_missing"))
    }
}
struct CmykRaster {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    icc: Vec<u8>,
    warnings: Vec<String>,
}

fn rendering_intent(name: &str) -> Result<lcms2::Intent, CommandError> {
    match name {
        "perceptual" => Ok(lcms2::Intent::Perceptual),
        "relative_colorimetric" => Ok(lcms2::Intent::RelativeColorimetric),
        "absolute_colorimetric" => Ok(lcms2::Intent::AbsoluteColorimetric),
        "saturation" => Ok(lcms2::Intent::Saturation),
        _ => Err(crate::export_error("INVALID_PARAMS", "error.invalid_params")),
    }
}

fn render_cmyk(doc: &dtool_core::Document, opts: &CmykOptions) -> Result<CmykRaster, CommandError> {
    let profile_path = resolve_cmyk_profile(opts.profile.as_deref())?;

    let intent = rendering_intent(opts.intent.as_str())?;

    let src_profile = lcms2::Profile::new_srgb();

    let dst_profile = lcms2::Profile::new_file(&profile_path)
        .map_err(|_| crate::export_error("NOT_FOUND", "error.io.not_found"))?;

    let transform: lcms2::Transform<[u8; 3], [u8; 4]> = lcms2::Transform::new(
        &src_profile,
        lcms2::PixelFormat::RGB_8,
        &dst_profile,
        lcms2::PixelFormat::CMYK_8,
        intent,
    )
    .map_err(|_| crate::export_error("INVALID_PARAMS", "error.io.invalid_format"))?;

    let width = doc.width().round().max(0.0) as usize;
    let height = doc.height().round().max(0.0) as usize;
    if width == 0 || height == 0 {
        return Err(CommandError::no_document());
    }
    let src_data = doc.render_rgb(width, height);
    
    if src_data.is_empty() {
        return Err(CommandError::no_document());
    }

    let rgb_image: image::RgbImage = image::ImageBuffer::from_raw(width as u32, height as u32, src_data)
        .ok_or_else(|| CommandError::validation("INVALID_PARAMS"))?;
    let src_pixels: Vec<[u8; 3]> = rgb_image.pixels().map(|pixel| pixel.0).collect();

    let mut dst_pixels = vec![[0u8; 4]; src_pixels.len()];
    transform.transform_pixels(&src_pixels, &mut dst_pixels);

    let mut warnings = Vec::new();
    if opts.bit_depth != 8 {
        warnings.push("warning.cmyk.bit_depth_8".to_string());
    }

    Ok(CmykRaster {
        width,
        height,
        pixels: dst_pixels.into_iter().flatten().collect(),
        icc: dst_profile.icc().unwrap_or_default(),
        warnings,
    })
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn tiff_entry(out: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
    push_u16(out, tag);
    push_u16(out, kind);
    push_u32(out, count);
    if kind == 3 && count == 1 {
        push_u16(out, value as u16);
        push_u16(out, 0);
    } else {
        push_u32(out, value);
    }
}

fn encode_cmyk_tiff(raster: &CmykRaster, dpi: f32) -> Result<Vec<u8>, CommandError> {
    let image_len = raster.pixels.len();
    let width = u32::try_from(raster.width).map_err(|_| CommandError::new("REQUEST_TOO_LARGE"))?;
    let height = u32::try_from(raster.height).map_err(|_| CommandError::new("REQUEST_TOO_LARGE"))?;
    let image_len_u32 = u32::try_from(image_len).map_err(|_| CommandError::new("REQUEST_TOO_LARGE"))?;
    let has_icc = !raster.icc.is_empty();
    let entry_count: u16 = if has_icc { 15 } else { 14 };
    let ifd_offset = 8u32;
    let ifd_len = 2 + u32::from(entry_count) * 12 + 4;
    let bits_offset = ifd_offset + ifd_len;
    let xres_offset = bits_offset + 8;
    let yres_offset = xres_offset + 8;
    let icc_offset = yres_offset + 8;
    let icc_len = raster.icc.len() as u32;
    let icc_padded = icc_len + (icc_len % 2);
    let data_offset = icc_offset + icc_padded;
    let resolution = if dpi.is_finite() && dpi > 0.0 { dpi.round() as u32 } else { 300 };

    let mut out = Vec::with_capacity(data_offset as usize + image_len);
    out.extend_from_slice(b"II");
    push_u16(&mut out, 42);
    push_u32(&mut out, ifd_offset);

    push_u16(&mut out, entry_count);
    tiff_entry(&mut out, 256, 4, 1, width);
    tiff_entry(&mut out, 257, 4, 1, height);
    tiff_entry(&mut out, 258, 3, 4, bits_offset);
    tiff_entry(&mut out, 259, 3, 1, 1);
    tiff_entry(&mut out, 262, 3, 1, 5);
    tiff_entry(&mut out, 273, 4, 1, data_offset);
    tiff_entry(&mut out, 277, 3, 1, 4);
    tiff_entry(&mut out, 278, 4, 1, height);
    tiff_entry(&mut out, 279, 4, 1, image_len_u32);
    tiff_entry(&mut out, 282, 5, 1, xres_offset);
    tiff_entry(&mut out, 283, 5, 1, yres_offset);
    tiff_entry(&mut out, 284, 3, 1, 1);
    tiff_entry(&mut out, 296, 3, 1, 2);
    tiff_entry(&mut out, 332, 3, 1, 1);
    if has_icc {
        tiff_entry(&mut out, 34675, 7, icc_len, icc_offset);
    }
    push_u32(&mut out, 0);

    for _ in 0..4 {
        push_u16(&mut out, 8);
    }
    push_u32(&mut out, resolution);
    push_u32(&mut out, 1);
    push_u32(&mut out, resolution);
    push_u32(&mut out, 1);
    out.extend_from_slice(&raster.icc);
    if icc_len % 2 == 1 {
        out.push(0);
    }
    out.extend_from_slice(&raster.pixels);
    Ok(out)
}

pub fn export_tiff_cmyk(
    doc: &dtool_core::Document,
    req: &ExportRequest,
    opts: &CmykOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }

    let raster = render_cmyk(doc, opts)?;

    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }

    let encoded = encode_cmyk_tiff(&raster, doc.dpi)?;

    let tmp_path = req.path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp_path).map_err(crate::export_io_error)?;
    if let Err(e) = file.write_all(&encoded).and_then(|_| file.flush()) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(crate::export_io_error(e));
    }
    drop(file);
    if let Err(e) = std::fs::rename(&tmp_path, &req.path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(crate::export_io_error(e));
    }

    control.set_progress(1.0);

    let bytes = std::fs::metadata(&req.path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(ExportReceipt {
        path: req.path.clone(),
        bytes,
        warnings: raster.warnings,
    })
}

pub fn export_jpeg_cmyk(
    doc: &dtool_core::Document,
    req: &ExportRequest,
    opts: &CmykOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }

    let raster = render_cmyk(doc, opts)?;

    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }

    let width = u16::try_from(raster.width).map_err(|_| CommandError::new("REQUEST_TOO_LARGE"))?;
    let height = u16::try_from(raster.height).map_err(|_| CommandError::new("REQUEST_TOO_LARGE"))?;

    let quality = req
        .options
        .get("quality")
        .and_then(|v| v.as_u64())
        .map(|v| v.clamp(1, 100) as u8)
        .unwrap_or(90);

    let mut encoder = jpeg_encoder::Encoder::new_file(&req.path, quality)
        .map_err(crate::export_io_error)?;
    if !raster.icc.is_empty() {
        encoder
            .add_icc_profile(&raster.icc)
            .map_err(|_| crate::export_error("INVALID_PARAMS", "error.io.invalid_format"))?;
    }
    encoder
        .encode(&raster.pixels, width, height, jpeg_encoder::ColorType::Cmyk)
        .map_err(crate::export_io_error)?;

    control.set_progress(1.0);

    let bytes = std::fs::metadata(&req.path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(ExportReceipt {
        path: req.path.clone(),
        bytes,
        warnings: raster.warnings,
    })
}

pub fn export_cmyk_value(
    doc: &dtool_core::Document,
    req: &ExportRequest,
    opts: &CmykOptions,
    control: &ExportControl,
) -> CommandResult {
    let receipt = match req.format {
        crate::ExportFormat::Jpeg => export_jpeg_cmyk(doc, req, opts, control)?,
        _ => export_tiff_cmyk(doc, req, opts, control)?,
    };
    Ok(serde_json::json!({
        "path": receipt.path.to_string_lossy(),
        "bytes": receipt.bytes,
        "warnings": receipt.warnings,
    }))
}
