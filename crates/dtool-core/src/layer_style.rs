pub mod raster;

use crate::blend::BlendMode;
use crate::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlendRanges {
    pub source: Vec<[f32; 2]>,
    pub underlying: Vec<[f32; 2]>,
    #[serde(default = "unit_gamma")]
    pub gamma: f32,
    #[serde(default)]
    pub antialias: bool,
}

fn unit_gamma() -> f32 {
    1.0
}

impl BlendRanges {
    pub fn is_valid(&self) -> bool {
        let curve = |points: &Vec<[f32; 2]>| {
            points.len() >= 2
                && points.iter().all(|[x, y]| x.is_finite() && y.is_finite() && (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y))
                && points.windows(2).all(|pair| pair[0][0] <= pair[1][0])
        };
        curve(&self.source) && curve(&self.underlying) && self.gamma.is_finite() && self.gamma > 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    GaussianBlur,
    OuterShadow,
    InnerShadow,
    OuterGlow,
    InnerGlow,
    Outline,
    ThreeD,
    BevelEmboss,
    ColourOverlay,
    GradientOverlay,
}

impl EffectKind {
    pub const ALL: [EffectKind; 10] = [
        EffectKind::GaussianBlur,
        EffectKind::OuterShadow,
        EffectKind::InnerShadow,
        EffectKind::OuterGlow,
        EffectKind::InnerGlow,
        EffectKind::Outline,
        EffectKind::ThreeD,
        EffectKind::BevelEmboss,
        EffectKind::ColourOverlay,
        EffectKind::GradientOverlay,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutlineAlign {
    Outside,
    Centre,
    Inside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BevelType {
    Inner,
    Outer,
    Emboss,
    Pillow,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BandPass {
    pub hue: [f32; 2],
    pub intensity: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EffectParams {
    Blur {
        radius: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius_y: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<f32>,
        preserve_alpha: bool,
    },
    Shadow {
        radius: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius_y: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blur_angle: Option<f32>,
        offset: f32,
        angle: f32,
        intensity: f32,
        colour: Colour,
        knock_out: bool,
    },
    Glow { radius: f32, intensity: f32, colour: Colour, from_centre: bool },
    Outline { width: f32, align: OutlineAlign, fill: Paint },
    Light { radius: f32, depth: f32, soften: f32, azimuth: f32, elevation: f32, bevel: BevelType, highlight: Colour, shadow: Colour },
    Overlay { fill: Paint },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerEffect {
    pub kind: EffectKind,
    pub enabled: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    pub params: EffectParams,
    #[serde(default)]
    pub band_pass: Option<BandPass>,
}

pub fn overlay_gradient(size: [f32; 2]) -> Paint {
    let [w, h] = size.map(|v| if v.is_finite() && v.abs() > 0.0 { f64::from(v.abs()) } else { 1.0 });
    let stops = vec![GradientStop { offset: 0.0, colour: black() }, GradientStop { offset: 1.0, colour: white() }];
    Paint::Gradient { kind: GradientKind::Linear, stops, geom: GradientGeom { start: [w * 0.5, h], end: [w * 0.5, 0.0], ratio: 1.0, ..GradientGeom::default() } }
}

pub fn default_effect_for(kind: EffectKind, size: [f32; 2]) -> LayerEffect {
    let mut effect = default_effect(kind);
    if kind == EffectKind::GradientOverlay {
        effect.params = EffectParams::Overlay { fill: overlay_gradient(size) };
    }
    effect
}

fn black() -> Colour {
    Colour::new(0, 0, 0, 255)
}

fn white() -> Colour {
    Colour::new(255, 255, 255, 255)
}

pub fn default_effect(kind: EffectKind) -> LayerEffect {
    let (blend, params) = match kind {
        EffectKind::GaussianBlur => (BlendMode::Normal, EffectParams::Blur { radius: 4.0, radius_y: None, angle: None, preserve_alpha: false }),
        EffectKind::OuterShadow => (BlendMode::Multiply, EffectParams::Shadow { radius: 4.0, radius_y: None, blur_angle: None, offset: 4.0, angle: 135.0, intensity: 0.0, colour: black(), knock_out: true }),
        EffectKind::InnerShadow => (BlendMode::Multiply, EffectParams::Shadow { radius: 4.0, radius_y: None, blur_angle: None, offset: 4.0, angle: 135.0, intensity: 0.0, colour: black(), knock_out: false }),
        EffectKind::OuterGlow => (BlendMode::Screen, EffectParams::Glow { radius: 8.0, intensity: 0.0, colour: white(), from_centre: false }),
        EffectKind::InnerGlow => (BlendMode::Screen, EffectParams::Glow { radius: 8.0, intensity: 0.0, colour: white(), from_centre: false }),
        EffectKind::Outline => (BlendMode::Normal, EffectParams::Outline { width: 1.0, align: OutlineAlign::Outside, fill: Paint::Solid { colour: black() } }),
        EffectKind::ThreeD => (BlendMode::Normal, EffectParams::Light { radius: 4.0, depth: 1.0, soften: 0.0, azimuth: 135.0, elevation: 45.0, bevel: BevelType::Inner, highlight: white(), shadow: black() }),
        EffectKind::BevelEmboss => (BlendMode::Normal, EffectParams::Light { radius: 4.0, depth: 1.0, soften: 0.0, azimuth: 135.0, elevation: 45.0, bevel: BevelType::Inner, highlight: white(), shadow: black() }),
        EffectKind::ColourOverlay => (BlendMode::Normal, EffectParams::Overlay { fill: Paint::Solid { colour: black() } }),
        EffectKind::GradientOverlay => (BlendMode::Normal, EffectParams::Overlay { fill: overlay_gradient([1.0, 1.0]) }),
    };
    LayerEffect { kind, enabled: true, opacity: if kind == EffectKind::OuterShadow || kind == EffectKind::InnerShadow { 0.75 } else { 1.0 }, blend, params, band_pass: None }
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn unit(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn pair(range: [f32; 2]) -> bool {
    finite(&range) && range[0] <= range[1]
}

impl LayerEffect {
    pub fn is_valid(&self) -> bool {
        if !unit(self.opacity) || self.blend == BlendMode::PassThrough {
            return false;
        }
        if let Some(band) = self.band_pass {
            if !pair(band.hue) || !pair(band.intensity) {
                return false;
            }
        }
        match (&self.params, self.kind) {
            (EffectParams::Blur { radius, radius_y, angle, .. }, EffectKind::GaussianBlur) => {
                [*radius, radius_y.unwrap_or(*radius)].into_iter().all(|value| value.is_finite() && value >= 0.0)
                    && angle.is_none_or(f32::is_finite)
            }
            (EffectParams::Shadow { radius, radius_y, blur_angle, offset, angle, intensity, .. }, EffectKind::OuterShadow | EffectKind::InnerShadow) =>
                finite(&[*offset, *angle, blur_angle.unwrap_or_default()])
                    && [*radius, radius_y.unwrap_or(*radius)].into_iter().all(|r| r.is_finite() && r >= 0.0)
                    && unit(*intensity),
            (EffectParams::Glow { radius, intensity, .. }, EffectKind::OuterGlow | EffectKind::InnerGlow) => radius.is_finite() && *radius >= 0.0 && unit(*intensity),
            (EffectParams::Outline { width, .. }, EffectKind::Outline) => width.is_finite() && *width >= 0.0,
            (EffectParams::Light { radius, depth, soften, azimuth, elevation, .. }, EffectKind::ThreeD | EffectKind::BevelEmboss) => finite(&[*depth, *soften, *azimuth, *elevation]) && radius.is_finite() && *radius >= 0.0 && *soften >= 0.0,
            (EffectParams::Overlay { .. }, EffectKind::ColourOverlay | EffectKind::GradientOverlay) => true,
            _ => false,
        }
    }
}

pub fn is_normal(mode: &BlendMode) -> bool {
    *mode == BlendMode::Normal
}
