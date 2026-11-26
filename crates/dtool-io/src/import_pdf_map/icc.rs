use std::borrow::Cow;
use std::collections::HashMap;

use dtool_core::paint::GradientStop;
use lcms2::{ColorSpaceSignature, Intent, PixelFormat, Profile, Transform};

use super::{Colour, PdfDocument, PdfId, PdfObject, Space, Stream, deref, dict_of, num};

const MAX_PROFILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PROFILES: usize = 32;

#[derive(Default)]
pub(super) struct Profiles {
    ids: HashMap<(PdfId, usize), Option<Space>>,
    transforms: Vec<Conversion>,
}

pub(super) struct Conversion {
    transform: ToRgb,
    range: [[f32; 2]; 4],
}

enum ToRgb {
    Gray(Transform<[f32; 1], [f32; 3]>),
    Rgb(Transform<[f32; 3], [f32; 3]>),
    Cmyk(Transform<[f32; 4], [f32; 3]>),
}

impl Profiles {
    pub(super) fn load(
        &mut self,
        pdf: &PdfDocument,
        object: &PdfObject,
        channels: usize,
    ) -> Option<Space> {
        let id = object.as_reference().ok()?;
        let key = (id, channels);
        if let Some(cached) = self.ids.get(&key) {
            return *cached;
        }
        if self.ids.len() >= MAX_PROFILES {
            return None;
        }
        let colour_space = self.load_stream(pdf, object, channels);
        self.ids.insert(key, colour_space);
        colour_space
    }

    fn load_stream(
        &mut self,
        pdf: &PdfDocument,
        object: &PdfObject,
        channels: usize,
    ) -> Option<Space> {
        let stream = deref(pdf, object).as_stream().ok()?;
        let data = profile_bytes(stream)?;
        let input = Profile::new_icc(&data).ok()?;
        let output = Profile::new_srgb();
        let intent = Intent::RelativeColorimetric;
        let transform = match (input.color_space(), channels) {
            (ColorSpaceSignature::GrayData, 1) => ToRgb::Gray(
                Transform::new(
                    &input,
                    PixelFormat::GRAY_FLT,
                    &output,
                    PixelFormat::RGB_FLT,
                    intent,
                )
                .ok()?,
            ),
            (ColorSpaceSignature::RgbData, 3) => ToRgb::Rgb(
                Transform::new(
                    &input,
                    PixelFormat::RGB_FLT,
                    &output,
                    PixelFormat::RGB_FLT,
                    intent,
                )
                .ok()?,
            ),
            (ColorSpaceSignature::CmykData, 4) => ToRgb::Cmyk(
                Transform::new(
                    &input,
                    PixelFormat::CMYK_FLT,
                    &output,
                    PixelFormat::RGB_FLT,
                    intent,
                )
                .ok()?,
            ),
            _ => return None,
        };
        let mut range = [[0.0, 1.0]; 4];
        if let Ok(values) = stream.dict.get(b"Range") {
            let values = deref(pdf, values).as_array().ok()?;
            if values.len() != channels * 2 {
                return None;
            }
            for (bounds, pair) in range.iter_mut().zip(values.chunks_exact(2)) {
                let lo = num(deref(pdf, &pair[0]))? as f32;
                let hi = num(deref(pdf, &pair[1]))? as f32;
                if lo > hi {
                    return None;
                }
                *bounds = [lo.clamp(0.0, 1.0), hi.clamp(0.0, 1.0)];
            }
        }
        let index = self.transforms.len();
        self.transforms.push(Conversion { transform, range });
        Some(Space::Icc { index, channels })
    }

    pub(super) fn conversion(&self, space: Space) -> Option<&Conversion> {
        let Space::Icc { index, .. } = space else {
            return None;
        };
        self.transforms.get(index)
    }
}

impl Conversion {
    pub(super) fn input_range(&self, channels: usize) -> Vec<f64> {
        self.range[..channels]
            .iter()
            .flatten()
            .map(|&v| f64::from(v))
            .collect()
    }

    pub(super) fn append_image(
        &self,
        count: usize,
        sample: impl Fn(usize, usize) -> f64,
        rgba: &mut Vec<u8>,
    ) {
        match &self.transform {
            ToRgb::Gray(transform) => {
                convert_image(transform, &self.range, count, sample, 1.0, rgba)
            }
            ToRgb::Rgb(transform) => {
                convert_image(transform, &self.range, count, sample, 1.0, rgba)
            }
            ToRgb::Cmyk(transform) => {
                convert_image(transform, &self.range, count, sample, 100.0, rgba)
            }
        }
    }

    pub(super) fn rgb(&self, values: &[f64]) -> [f32; 3] {
        let sample = |i: usize| {
            let [lo, hi] = self.range[i];
            (values.get(i).copied().unwrap_or(0.0) as f32).clamp(lo, hi)
        };
        let mut output = [[0.0; 3]];
        match &self.transform {
            ToRgb::Gray(transform) => transform.transform_pixels(&[[sample(0)]], &mut output),
            ToRgb::Rgb(transform) => {
                transform.transform_pixels(&[std::array::from_fn(sample)], &mut output)
            }
            ToRgb::Cmyk(transform) => transform
                .transform_pixels(&[std::array::from_fn(|i| sample(i) * 100.0)], &mut output),
        }
        output[0].map(|channel| channel.clamp(0.0, 1.0))
    }
}

fn convert_image<const N: usize>(
    transform: &Transform<[f32; N], [f32; 3]>,
    range: &[[f32; 2]; 4],
    count: usize,
    sample: impl Fn(usize, usize) -> f64,
    scale: f32,
    rgba: &mut Vec<u8>,
) where
    [f32; N]: lcms2::Pod,
{
    let mut input = Vec::with_capacity(1024);
    let mut output = [[0.0; 3]; 1024];
    for start in (0..count).step_by(1024) {
        input.clear();
        input.extend((start..(start + 1024).min(count)).map(|pixel| {
            std::array::from_fn(|i| {
                (sample(pixel, i) as f32).clamp(range[i][0], range[i][1]) * scale
            })
        }));
        let output = &mut output[..input.len()];
        transform.transform_pixels(&input, output);
        for &rgb in output.iter() {
            let c = colour(rgb);
            rgba.extend([c.r, c.g, c.b, 255]);
        }
    }
}

pub(super) fn colour(rgb: [f32; 3]) -> Colour {
    let [r, g, b] = rgb.map(|value| (value * 255.0).round() as u8);
    Colour::new(r, g, b, 255)
}

pub(super) fn gradient_stops(
    conversion: &Conversion,
    source: &[(f32, [f64; 4])],
) -> Option<(Vec<GradientStop>, bool)> {
    let &(offset, values) = source.first()?;
    let mut start = conversion.rgb(&values);
    let mut stops = vec![GradientStop {
        offset,
        colour: colour(start),
    }];
    let mut extra = 1024usize.saturating_sub(source.len());
    let mut limited = false;
    for pair in source.windows(2) {
        let [(left, a), (right, b)] = [pair[0], pair[1]];
        let end = conversion.rgb(&b);
        // Transform the original component interpolation. Mixing the converted
        // endpoints would skip the nonlinear tone curves and profile lookup tables.
        let evaluate = |t: f32| {
            let t = f64::from(t);
            let values: [f64; 4] = std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t);
            conversion.rgb(&values)
        };
        let mut stack = vec![([0.0f32, 1.0], [start, end], 0)];
        while let Some(([lo, hi], [c0, c1], depth)) = stack.pop() {
            let offset_at = |t: f32| left + (right - left) * t;
            let split = left < right
                && [0.25, 0.5, 0.75].into_iter().any(|fraction| {
                    let rgb = evaluate(lo + (hi - lo) * fraction);
                    (0..3).any(|i| {
                        (rgb[i] - (c0[i] + (c1[i] - c0[i]) * fraction)).abs() > 0.75 / 255.0
                    })
                });
            let mid = lo.midpoint(hi);
            let middle_offset = offset_at(mid);
            if split
                && extra > 0
                && depth < 12
                && middle_offset > offset_at(lo)
                && middle_offset < offset_at(hi)
            {
                let c_mid = evaluate(mid);
                extra -= 1;
                stack.push(([mid, hi], [c_mid, c1], depth + 1));
                stack.push(([lo, mid], [c0, c_mid], depth + 1));
            } else {
                limited |= split;
                let offset = if hi == 1.0 { right } else { offset_at(hi) };
                stops.push(GradientStop {
                    offset,
                    colour: colour(c1),
                });
            }
        }
        start = end;
    }
    Some((stops, limited))
}

fn profile_bytes(stream: &Stream) -> Option<Cow<'_, [u8]>> {
    let data = if stream.dict.has(b"Filter") {
        Cow::Owned(
            stream
                .decompressed_content_with_limit(MAX_PROFILE_BYTES)
                .ok()?,
        )
    } else {
        Cow::Borrowed(stream.content.as_slice())
    };
    (data.len() <= MAX_PROFILE_BYTES).then_some(data)
}

pub(super) fn output_intent(pdf: &PdfDocument, profiles: &mut Profiles) -> (Option<Space>, bool) {
    let Some(entry) = pdf
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|root| dict_of(pdf, root))
        .and_then(|catalog| catalog.get(b"OutputIntents").ok())
    else {
        return (None, false);
    };
    let profile = deref(pdf, entry).as_array().ok().and_then(|intents| {
        intents.iter().take(MAX_PROFILES).find_map(|object| {
            let intent = dict_of(pdf, object)?;
            if deref(pdf, intent.get(b"S").ok()?).as_name().ok()? != b"GTS_PDFX" {
                return None;
            }
            profiles.load(pdf, intent.get(b"DestOutputProfile").ok()?, 4)
        })
    });
    (profile, true)
}
