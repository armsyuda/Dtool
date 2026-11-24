use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    PassThrough,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [BlendMode; 17] = [
        BlendMode::Normal,
        BlendMode::PassThrough,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    pub fn is_normal(self) -> bool {
        matches!(self, BlendMode::Normal | BlendMode::PassThrough)
    }
}

fn separable(mode: BlendMode, b: f32, s: f32) -> f32 {
    match mode {
        BlendMode::Multiply => b * s,
        BlendMode::Screen => b + s - b * s,
        BlendMode::Overlay => separable(BlendMode::HardLight, s, b),
        BlendMode::Darken => b.min(s),
        BlendMode::Lighten => b.max(s),
        BlendMode::ColorDodge => {
            if b <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }
        BlendMode::HardLight => {
            if s <= 0.5 {
                b * 2.0 * s
            } else {
                let t = 2.0 * s - 1.0;
                b + t - b * t
            }
        }
        BlendMode::SoftLight => {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }
        BlendMode::Difference => (b - s).abs(),
        BlendMode::Exclusion => b + s - 2.0 * b * s,
        _ => s,
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_colour(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 && l - n > 0.0 {
        out = out.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 && x - l > 0.0 {
        out = out.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_colour(c.map(|v| v + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|a, b| c[*a].total_cmp(&c[*b]));
    let (min, mid, max) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0; 3];
    if c[max] > c[min] {
        out[mid] = (c[mid] - c[min]) * s / (c[max] - c[min]);
        out[max] = s;
    }
    out
}

fn blend_rgb(mode: BlendMode, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    match mode {
        BlendMode::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        BlendMode::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        BlendMode::Color => set_lum(s, lum(b)),
        BlendMode::Luminosity => set_lum(b, lum(s)),
        _ => [separable(mode, b[0], s[0]), separable(mode, b[1], s[1]), separable(mode, b[2], s[2])],
    }
}

pub fn blend(dst: [f32; 4], src: [f32; 4], mode: BlendMode) -> [f32; 4] {
    let (ab, a_s) = (dst[3].clamp(0.0, 1.0), src[3].clamp(0.0, 1.0));
    let ao = a_s + ab * (1.0 - a_s);
    if ao <= 0.0 {
        return [0.0; 4];
    }
    let cb = [dst[0], dst[1], dst[2]].map(|v| v.clamp(0.0, 1.0));
    let cs = [src[0], src[1], src[2]].map(|v| v.clamp(0.0, 1.0));
    let mixed = if mode.is_normal() { cs } else { blend_rgb(mode, cb, cs) };
    let mut out = [0.0; 4];
    for i in 0..3 {
        let cs_prime = (1.0 - ab) * cs[i] + ab * mixed[i];
        out[i] = ((a_s * cs_prime + (1.0 - a_s) * ab * cb[i]) / ao).clamp(0.0, 1.0);
    }
    out[3] = ao;
    out
}

pub fn blend_pixel(pixels: &mut [u8], offset: usize, colour: [u8; 4], opacity: f32, mode: BlendMode) {
    let alpha = f32::from(colour[3]) / 255.0 * opacity;
    if alpha <= 0.0 || !alpha.is_finite() {
        return;
    }
    let old_alpha = f32::from(pixels[offset + 3]) / 255.0;
    let out_alpha = alpha + old_alpha * (1.0 - alpha);
    if out_alpha <= 0.0 {
        return;
    }
    if mode.is_normal() {
        for channel in 0..3 {
            let value = (f32::from(colour[channel]) * alpha + f32::from(pixels[offset + channel]) * old_alpha * (1.0 - alpha)) / out_alpha;
            pixels[offset + channel] = value.round().clamp(0.0, 255.0) as u8;
        }
    } else {
        let dst = [0, 1, 2].map(|i| f32::from(pixels[offset + i]) / 255.0);
        let src = [0, 1, 2].map(|i| f32::from(colour[i]) / 255.0);
        let out = blend([dst[0], dst[1], dst[2], old_alpha], [src[0], src[1], src[2], alpha.min(1.0)], mode);
        for channel in 0..3 {
            pixels[offset + channel] = (out[channel] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    pixels[offset + 3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
}

pub fn blend_pixel_normal(pixels: &mut [u8], offset: usize, colour: [u8; 4], opacity: f32) {
    blend_pixel(pixels, offset, colour, opacity, BlendMode::default());
}
