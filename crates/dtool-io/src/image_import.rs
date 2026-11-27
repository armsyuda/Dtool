//! File decoding for the existing image.place transaction. Original bytes stay in
//! ImageResource; decoded RGBA goes to the canonical pixel ledger, not vice versa.
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::{OnceLock, RwLock};

use dtool_core::commands::image_place::{DecodedImage, ImageDecoder, register_decoder};
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_core::CommandError;
use image::{DynamicImage, ImageDecoder as _, ImageReader};
use lcms2::{ColorSpaceSignature, Flags, Intent, PixelFormat, Profile, Transform};

const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_ICC_BYTES: u64 = 16 * 1024 * 1024;
static IMPORTERS: OnceLock<RwLock<HashMap<String, ImageDecoder>>> = OnceLock::new();
const ICC_INTENT: Intent = Intent::RelativeColorimetric;

pub fn install() -> bool {
    #[cfg(feature = "jxl")]
    let _ = register_place_importer("jxl", decode_jxl_path);
    #[cfg(feature = "heif")]
    {
        let _ = register_place_importer("heif", decode_heif_path);
        let _ = register_place_importer("heic", decode_heif_path);
        let _ = register_place_importer("hif", decode_heif_path);
    }
    register_decoder(decode_image)
}

fn extra_image_format(extension: &str) -> Option<image::ImageFormat> {
    match extension {
        "bmp" => Some(image::ImageFormat::Bmp),
        "tga" => Some(image::ImageFormat::Tga),
        "hdr" => Some(image::ImageFormat::Hdr),
        "exr" => Some(image::ImageFormat::OpenExr),
        _ => None,
    }
}

fn decode_configured(path: &Path, extension: &str, bytes: &[u8]) -> Result<DecodedImage, CommandError> {
    #[cfg(feature = "jxl")]
    if extension == "jxl" {
        return decode_jxl(path, bytes);
    }
    #[cfg(feature = "heif")]
    if matches!(extension, "heif" | "heic" | "hif") {
        return decode_heif(path, bytes);
    }
    let _ = (path, bytes, extension);
    Err(CommandError::new("UNSUPPORTED_PROFILE"))
}

/// Additional approved format adapters (e.g. HEIF/JXL) can be installed without
/// treating an unsupported extension as a successfully decoded image.
pub fn register_place_importer(extension: &str, decoder: ImageDecoder) -> Result<(), CommandError> {
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    if extension.is_empty() || !extension.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(CommandError::invalid_params());
    }
    IMPORTERS.get_or_init(Default::default).write().map_err(|_| CommandError::new("NOT_AVAILABLE"))?.insert(extension, decoder);
    Ok(())
}

fn image_error(error: image::ImageError) -> CommandError {
    match error {
        image::ImageError::Unsupported(_) => CommandError::new("UNSUPPORTED_PROFILE"),
        image::ImageError::Limits(_) => CommandError::new("REQUEST_TOO_LARGE"),
        _ => CommandError::new("INVALID_FORMAT"),
    }
}

fn png_dpi(bytes: &[u8]) -> Option<f32> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") { return None; }
    let mut offset = 8usize;
    while offset.checked_add(12)? <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        let end = offset.checked_add(12)?.checked_add(length)?;
        if end > bytes.len() { return None; }
        let kind = &bytes[offset + 4..offset + 8];
        if kind == b"pHYs" && length == 9 && bytes[offset + 16] == 1 {
            let x = u32::from_be_bytes(bytes[offset + 8..offset + 12].try_into().ok()?);
            let y = u32::from_be_bytes(bytes[offset + 12..offset + 16].try_into().ok()?);
            if x > 0 && y > 0 { return Some(x as f32 * 0.0254); }
        }
        if kind == b"IDAT" || kind == b"IEND" { break; }
        offset = end;
    }
    None
}

fn rgba_in_srgb(image: DynamicImage, source: Option<&Profile>, srgb: &Profile) -> Vec<u8> {
    let input_colour = image.color();
    let mut rgba = image.into_rgba8().into_raw();
    let Some(source) = source else { return rgba; };
    let flags = Flags::BLACKPOINT_COMPENSATION | Flags::COPY_ALPHA;
    match source.color_space() {
        ColorSpaceSignature::RgbData if input_colour.has_color() => {
            if let Ok(transform) = Transform::new_flags(source, PixelFormat::RGBA_8, srgb, PixelFormat::RGBA_8, ICC_INTENT, flags) {
                transform.transform_in_place(rgba.as_chunks_mut::<4>().0);
            }
        }
        ColorSpaceSignature::GrayData if !input_colour.has_color() => {
            if let Ok(transform) = Transform::new_flags(source, PixelFormat::GRAYA_8, srgb, PixelFormat::RGBA_8, ICC_INTENT, flags) {
                let mut gray_alpha = [[0; 2]; 1024];
                for pixels in rgba.as_chunks_mut::<4>().0.chunks_mut(gray_alpha.len()) {
                    for (sample, pixel) in gray_alpha.iter_mut().zip(pixels.iter()) {
                        *sample = [pixel[0], pixel[3]];
                    }
                    transform.transform_pixels(&gray_alpha[..pixels.len()], pixels);
                }
            }
        }
        _ => {}
    }
    rgba
}

fn tiff_icc(decoder: &mut tiff::decoder::Decoder<Cursor<&[u8]>>) -> Option<Profile> {
    use tiff::tags::{Tag, Type};
    let mut ifd = decoder.image_ifd();
    let entry = ifd.find_entry(Tag::IccProfile)?;
    if !matches!(entry.field_type(), Type::BYTE | Type::UNDEFINED) || entry.count() > MAX_ICC_BYTES {
        return None;
    }
    let mut data = vec![0; entry.count() as usize];
    if ifd.find_tag_bytes(Tag::IccProfile, &mut data, 0).ok()?? != data.len() {
        return None;
    }
    Profile::new_icc(&data).ok()
}

fn cmyk8_in_srgb(samples: &[[u8; 4]], source: &Profile, srgb: &Profile) -> Option<Vec<u8>> {
    let transform = Transform::new_flags(
        source, PixelFormat::CMYK_8, srgb, PixelFormat::RGBA_8, ICC_INTENT, Flags::BLACKPOINT_COMPENSATION,
    ).ok()?;
    let mut rgba = vec![255; samples.len() * 4];
    transform.transform_pixels(samples, rgba.as_chunks_mut::<4>().0);
    Some(rgba)
}

fn jpeg_has_adobe_marker(bytes: &[u8]) -> Option<bool> {
    let mut rest = bytes.strip_prefix(&[0xff, 0xd8])?;
    while let Some((&0xff, suffix)) = rest.split_first() {
        let marker_at = suffix.iter().position(|&byte| byte != 0xff)?;
        let (&marker, segment) = suffix[marker_at..].split_first()?;
        if matches!(marker, 0xda | 0xd9) {
            return Some(false);
        }
        if matches!(marker, 0x01 | 0xd0..=0xd7) {
            rest = segment;
            continue;
        }
        let length = usize::from(u16::from_be_bytes(segment.get(..2)?.try_into().ok()?));
        let data = segment.get(2..length)?;
        if marker == 0xee && data.len() >= 12 && data.starts_with(b"Adobe") {
            return Some(true);
        }
        rest = &segment[length..];
    }
    None
}

fn cmyk_jpeg_in_srgb(bytes: &[u8], source: &Profile, srgb: &Profile, dimensions: [u32; 2]) -> Option<Vec<u8>> {
    use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    use zune_jpeg::JpegDecoder;
    let options = DecoderOptions::default().set_strict_mode(false).set_max_width(32768).set_max_height(32768);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().ok()?;
    if decoder.dimensions()? != (dimensions[0] as usize, dimensions[1] as usize) {
        return None;
    }
    let encoding = decoder.input_colorspace()?;
    if !matches!(encoding, ColorSpace::CMYK | ColorSpace::YCCK) {
        return None;
    }
    decoder.set_options(decoder.options().jpeg_set_out_colorspace(encoding));
    let mut bytes_out = decoder.decode().ok()?;
    let (samples, tail) = bytes_out.as_chunks_mut::<4>();
    if !tail.is_empty() || samples.len() != dimensions[0] as usize * dimensions[1] as usize {
        return None;
    }
    if encoding == ColorSpace::YCCK {
        // YCbCr encodes the CMY signals, and the JPEG's black sample is inverted.
        for [cyan, magenta, yellow, black] in samples.iter_mut() {
            let (luma, cb, cr) = (f32::from(*cyan), f32::from(*magenta) - 128.0, f32::from(*yellow) - 128.0);
            let quantize = |value: f32| value.round().clamp(0.0, 255.0) as u8;
            *cyan = quantize(luma + 1.402 * cr);
            *magenta = quantize(luma - 0.344136 * cb - 0.714136 * cr);
            *yellow = quantize(luma + 1.772 * cb);
            *black = 255 - *black;
        }
    } else if jpeg_has_adobe_marker(bytes)? {
        for sample in samples.iter_mut().flatten() {
            *sample = 255 - *sample;
        }
    }
    cmyk8_in_srgb(samples, source, srgb)
}

// Keep CMYK separations available until the input ICC transform has run.
// The general image decoder's TIFF path produces RGB before exposing pixels.
fn cmyk_tiff_in_srgb(decoder: &mut tiff::decoder::Decoder<Cursor<&[u8]>>, source: &Profile, srgb: &Profile, dimensions: [u32; 2]) -> Option<Vec<u8>> {
    use tiff::decoder::DecodingResult;
    if decoder.dimensions().ok()? != (dimensions[0], dimensions[1])
        || !matches!(decoder.colortype().ok()?, tiff::ColorType::CMYK(8 | 16)) {
        return None;
    }
    let count = dimensions[0] as usize * dimensions[1] as usize;
    match decoder.read_image().ok()? {
        DecodingResult::U8(data) if data.len() == count * 4 => cmyk8_in_srgb(data.as_chunks::<4>().0, source, srgb),
        DecodingResult::U16(data) if data.len() == count * 4 => {
            let transform = Transform::new_flags(
                source, PixelFormat::CMYK_16, srgb, PixelFormat::RGBA_8, ICC_INTENT, Flags::BLACKPOINT_COMPENSATION,
            ).ok()?;
            let mut rgba = vec![255; count * 4];
            transform.transform_pixels(data.as_chunks::<4>().0, rgba.as_chunks_mut::<4>().0);
            Some(rgba)
        }
        _ => None,
    }
}

/// Preview embedded resources that do not already have a decoded placement.
pub fn embedded_thumbnail(bytes: &[u8], side: u32) -> Option<image::RgbaImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(24 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > 6_000_000 { return None; }
    let source = decoder.icc_profile().ok().flatten()
        .filter(|bytes| bytes.len() as u64 <= MAX_ICC_BYTES)
        .and_then(|bytes| Profile::new_icc(&bytes).ok());
    let image = DynamicImage::from_decoder(decoder).ok()?.thumbnail(side, side);
    let size = (image.width(), image.height());
    let rgba = rgba_in_srgb(image, source.as_ref(), &Profile::new_srgb());
    image::RgbaImage::from_raw(size.0, size.1, rgba)
}

pub fn decode_image(path: &Path) -> Result<DecodedImage, CommandError> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let importer = IMPORTERS.get().and_then(|registry| registry.read().ok().and_then(|registry| registry.get(&extension).copied()));
    if let Some(importer) = importer { return importer(path); }
    let file = std::fs::File::open(path).map_err(|error| dtool_core::import_hook::read_error(&error))?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes).map_err(|error| dtool_core::import_hook::read_error(&error))?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format().map_err(|_| CommandError::new("INVALID_FORMAT"))?;
    if reader.format().is_none() {
        if let Some(format) = extra_image_format(&extension) {
            reader.set_format(format);
        }
    }
    let format = match reader.format() {
        Some(image::ImageFormat::Png) => ImageFormat::Png,
        Some(image::ImageFormat::Jpeg) => ImageFormat::Jpeg,
        Some(image::ImageFormat::Gif) => ImageFormat::Gif,
        Some(image::ImageFormat::Tiff) => ImageFormat::Tiff,
        Some(image::ImageFormat::WebP) => ImageFormat::Webp,
        Some(image::ImageFormat::Bmp) => ImageFormat::Bmp,
        Some(image::ImageFormat::Tga) => ImageFormat::Tga,
        Some(image::ImageFormat::Hdr) => ImageFormat::Hdr,
        Some(image::ImageFormat::OpenExr) => ImageFormat::Exr,
        _ => return decode_configured(path, &extension, &bytes),
    };
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 4);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(image_error)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    let mut tiff = if format == ImageFormat::Tiff { tiff::decoder::Decoder::new(Cursor::new(bytes.as_slice())).ok() } else { None };
    let source = if let Some(tiff) = tiff.as_mut() {
        tiff_icc(tiff)
    } else {
        decoder.icc_profile().ok().flatten().filter(|bytes| bytes.len() as u64 <= MAX_ICC_BYTES)
            .and_then(|bytes| Profile::new_icc(&bytes).ok())
    };
    let srgb = Profile::new_srgb();
    let cmyk_pixels = source.as_ref()
        .filter(|profile| profile.color_space() == ColorSpaceSignature::CmykData)
        .and_then(|profile| match format {
            ImageFormat::Tiff => cmyk_tiff_in_srgb(tiff.as_mut()?, profile, &srgb, [width, height]),
            ImageFormat::Jpeg => cmyk_jpeg_in_srgb(&bytes, profile, &srgb, [width, height]),
            _ => None,
        });
    drop(tiff);
    let rgba = match cmyk_pixels {
        Some(pixels) => {
            drop(decoder);
            pixels
        }
        None => rgba_in_srgb(DynamicImage::from_decoder(decoder).map_err(image_error)?, source.as_ref(), &srgb),
    };
    let dpi = png_dpi(&bytes).unwrap_or(72.0);
    let mut resource = ImageResource::new_embedded(format, [width, height], dpi, bytes);
    resource.name = path.file_name().and_then(|s| s.to_str()).unwrap_or("Image").to_string();
    resource.path = Some(path.to_path_buf());
    Ok(DecodedImage { resource, rgba })
}


#[cfg(any(feature = "jxl", feature = "heif"))]
fn finish_decoded(path: &Path, format: ImageFormat, width: u32, height: u32, rgba: Vec<u8>, bytes: &[u8]) -> Result<DecodedImage, CommandError> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    let expected = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4));
    if expected != Some(rgba.len()) {
        return Err(CommandError::new("INVALID_FORMAT"));
    }
    let mut resource = ImageResource::new_embedded(format, [width, height], 72.0, bytes.to_vec());
    resource.name = path.file_name().and_then(|s| s.to_str()).unwrap_or("Image").to_string();
    resource.path = Some(path.to_path_buf());
    Ok(DecodedImage { resource, rgba })
}

#[cfg(feature = "jxl")]
fn decode_jxl_path(path: &Path) -> Result<DecodedImage, CommandError> {
    let bytes = std::fs::read(path).map_err(|error| dtool_core::import_hook::read_error(&error))?;
    decode_jxl(path, &bytes)
}

#[cfg(feature = "jxl")]
fn decode_jxl(path: &Path, bytes: &[u8]) -> Result<DecodedImage, CommandError> {
    use std::io::Cursor;
    let image = jxl_oxide::JxlImage::builder().read(Cursor::new(bytes)).map_err(|_| CommandError::new("INVALID_FORMAT"))?;
    if image.num_loaded_keyframes() == 0 {
        return Err(CommandError::new("INVALID_FORMAT"));
    }
    let render = image.render_frame(0).map_err(|_| CommandError::new("INVALID_FORMAT"))?;
    let mut stream = render.stream();
    let width = stream.width();
    let height = stream.height();
    let channels = stream.channels();
    if width == 0 || height == 0 || channels == 0 {
        return Err(CommandError::new("INVALID_FORMAT"));
    }
    let mut samples = vec![0u8; width as usize * height as usize * channels as usize];
    stream.write_to_buffer(&mut samples);
    let step = channels as usize;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in samples.chunks_exact(step) {
        match pixel {
            [g] => rgba.extend_from_slice(&[*g, *g, *g, 255]),
            [g, a] => rgba.extend_from_slice(&[*g, *g, *g, *a]),
            [r, g, b] => rgba.extend_from_slice(&[*r, *g, *b, 255]),
            rest => rgba.extend_from_slice(&[rest[0], rest[1], rest[2], rest.get(3).copied().unwrap_or(255)]),
        }
    }
    finish_decoded(path, ImageFormat::JpegXl, width, height, rgba, bytes)
}

#[cfg(feature = "heif")]
fn decode_heif_path(path: &Path) -> Result<DecodedImage, CommandError> {
    let bytes = std::fs::read(path).map_err(|error| dtool_core::import_hook::read_error(&error))?;
    decode_heif(path, &bytes)
}

#[cfg(feature = "heif")]
fn decode_heif(path: &Path, bytes: &[u8]) -> Result<DecodedImage, CommandError> {
    let options = heic_rs::DecodeOptions::default().with_layout(heic_rs::PixelLayout::Rgba8);
    let image = heic_rs::decode(bytes, &options).map_err(|_| CommandError::new("INVALID_FORMAT"))?;
    if image.layout != heic_rs::PixelLayout::Rgba8 {
        return Err(CommandError::new("INVALID_FORMAT"));
    }
    finish_decoded(path, ImageFormat::Heif, image.width, image.height, image.data, bytes)
}
