use lopdf::Stream;
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

use super::{Dictionary, MAX_BYTES, PdfDocument, PdfObject, deref, num, stream_bytes};

pub(super) fn dimensions(pdf: &PdfDocument, dict: &Dictionary) -> Option<[u32; 2]> {
    let dim = |key: &[u8]| {
        let value = num(deref(pdf, dict.get(key).ok()?))?;
        (value > 0.0 && value <= i32::MAX as f64 && value.fract() == 0.0).then_some(value as u32)
    };
    Some([dim(b"Width")?, dim(b"Height")?])
}

pub(super) fn samples(
    pdf: &PdfDocument,
    stream: &Stream,
    size: [u32; 2],
    channels: usize,
) -> Option<Vec<u8>> {
    if stream
        .filters()
        .ok()
        .is_some_and(|filters| filters == [b"DCTDecode".as_slice()])
    {
        let [width, height] = size.map(|n| n as usize);
        if stream.content.len() > MAX_BYTES {
            return None;
        }
        let options = DecoderOptions::default()
            .set_max_width(width)
            .set_max_height(height);
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(&stream.content), options);
        decoder.decode_headers().ok()?;
        if decoder.dimensions()? != (width, height) {
            return None;
        }
        let transform = stream
            .dict
            .get(b"DecodeParms")
            .ok()
            .and_then(|o| deref(pdf, o).as_dict().ok())
            .and_then(|d| d.get(b"ColorTransform").ok())
            .and_then(|o| deref(pdf, o).as_i64().ok());
        let input = decoder.input_colorspace()?;
        let output = match (channels, input, transform) {
            (1, ColorSpace::Luma, _) => ColorSpace::Luma,
            (3, ColorSpace::YCbCr, Some(0)) => ColorSpace::YCbCr,
            (3, ColorSpace::YCbCr | ColorSpace::RGB, _) => ColorSpace::RGB,
            (4, ColorSpace::CMYK, None | Some(0)) => ColorSpace::CMYK,
            _ => return None,
        };
        decoder.set_options(decoder.options().jpeg_set_out_colorspace(output));
        decoder.decode().ok()
    } else {
        stream_bytes(stream)
    }
}

pub(super) struct Mask {
    pub alpha: Vec<f32>,
    pub matte: Option<Vec<f64>>,
}

pub(super) fn mask(
    pdf: &PdfDocument,
    dict: &Dictionary,
    size: [u32; 2],
    image: &[u8],
    channels: usize,
) -> Option<Mask> {
    let get = |key: &[u8]| {
        dict.get(key).ok().map(|o| deref(pdf, o)).filter(|o| {
            !matches!(o, PdfObject::Null) && !matches!(o, PdfObject::Name(n) if n == b"None")
        })
    };
    if let Some(object) = get(b"SMask") {
        return mask_stream(pdf, object.as_stream().ok()?, size, channels, true);
    }
    match get(b"Mask")? {
        PdfObject::Stream(stream) => mask_stream(pdf, stream, size, channels, false),
        PdfObject::Array(keys) if keys.len() == channels * 2 => {
            let keys: Vec<u8> = keys
                .iter()
                .map(|o| u8::try_from(deref(pdf, o).as_i64().ok()?).ok())
                .collect::<Option<_>>()?;
            if keys.chunks_exact(2).any(|pair| pair[0] > pair[1]) {
                return None;
            }
            let alpha = image
                .chunks_exact(channels)
                .map(|pixel| {
                    let transparent = pixel
                        .iter()
                        .zip(keys.chunks_exact(2))
                        .all(|(sample, pair)| (pair[0]..=pair[1]).contains(sample));
                    if transparent { 0.0 } else { 1.0 }
                })
                .collect();
            Some(Mask { alpha, matte: None })
        }
        _ => None,
    }
}

fn mask_stream(
    pdf: &PdfDocument,
    stream: &Stream,
    size: [u32; 2],
    channels: usize,
    soft: bool,
) -> Option<Mask> {
    let dict = &stream.dict;
    let get = |key: &[u8]| dict.get(key).ok().map(|o| deref(pdf, o));
    if get(b"Subtype")?.as_name().ok()? != b"Image" || dict.has(b"SMask") || dict.has(b"Mask") {
        return None;
    }
    let stencil = get(b"ImageMask")
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);
    if soft == stencil || (soft && get(b"ColorSpace")?.as_name().ok()? != b"DeviceGray") {
        return None;
    }
    let [width, height] = dimensions(pdf, dict)?.map(|n| n as usize);
    if width.checked_mul(height)?.checked_mul(4)? > MAX_BYTES {
        return None;
    }
    let bits = if stencil {
        let bits = get(b"BitsPerComponent").map_or(Some(1), |o| o.as_i64().ok())?;
        if bits != 1 {
            return None;
        }
        1
    } else {
        let bits = usize::try_from(get(b"BitsPerComponent")?.as_i64().ok()?).ok()?;
        if !matches!(bits, 1 | 2 | 4 | 8 | 16) {
            return None;
        }
        bits
    };
    let [lo, hi] = if let Some(value) = get(b"Decode") {
        let [lo, hi] = value.as_array().ok()?.as_slice() else {
            return None;
        };
        [num(deref(pdf, lo))?, num(deref(pdf, hi))?]
    } else {
        [0.0, 1.0]
    };
    let matte = if soft {
        if let Some(value) = get(b"Matte") {
            let values = value.as_array().ok()?;
            if values.len() != channels || [width, height] != size.map(|n| n as usize) {
                return None;
            }
            Some(
                values
                    .iter()
                    .map(|o| num(deref(pdf, o)))
                    .collect::<Option<Vec<_>>>()?,
            )
        } else {
            None
        }
    } else {
        None
    };
    let stride = (width * bits).div_ceil(8);
    let data = if bits == 8 {
        samples(pdf, stream, [width as u32, height as u32], 1)?
    } else {
        stream_bytes(stream)?
    };
    if data.len() != stride.checked_mul(height)? {
        return None;
    }
    let limit = (1u32 << bits) - 1;
    let alpha_at = |x: usize, y: usize| {
        let row = y * stride;
        let sample = if bits == 16 {
            u32::from(u16::from_be_bytes([
                data[row + x * 2],
                data[row + x * 2 + 1],
            ]))
        } else {
            let bit = x * bits;
            (u32::from(data[row + bit / 8]) >> (8 - bits - bit % 8)) & limit
        };
        let t = f64::from(sample) / f64::from(limit);
        let alpha = (lo + (hi - lo) * t).clamp(0.0, 1.0) as f32;
        if soft { alpha } else { 1.0 - alpha }
    };
    let [w, h] = size.map(|n| n as usize);
    let interpolate = soft
        && get(b"Interpolate")
            .and_then(|o| o.as_bool().ok())
            .unwrap_or(false);
    let mut alpha = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let scaled_x = (x as f64 + 0.5) * width as f64 / w as f64;
            let scaled_y = (y as f64 + 0.5) * height as f64 / h as f64;
            let value = if [w, h] == [width, height] {
                alpha_at(x, y)
            } else if interpolate {
                let sx = (scaled_x - 0.5).clamp(0.0, (width - 1) as f64);
                let sy = (scaled_y - 0.5).clamp(0.0, (height - 1) as f64);
                let (x0, y0) = (sx as usize, sy as usize);
                let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
                let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
                let fx = sx.fract() as f32;
                let top = mix(alpha_at(x0, y0), alpha_at(x1, y0), fx);
                let bottom = mix(alpha_at(x0, y1), alpha_at(x1, y1), fx);
                mix(top, bottom, sy.fract() as f32)
            } else {
                alpha_at(
                    (scaled_x as usize).min(width - 1),
                    (scaled_y as usize).min(height - 1),
                )
            };
            alpha.push(value);
        }
    }
    Some(Mask { alpha, matte })
}
