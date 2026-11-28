use crate::raster_view::{RasterView, RasterViewMut};
use serde::{Deserialize, Serialize};

#[path = "brush_filters.rs"]
mod filters;
pub(crate) use filters::{apply as filter_dabs, filter_padding};

#[path = "brush_heal.rs"]
mod heal;
pub(crate) use heal::{heal_dabs, heal_region};

#[path = "brush_image.rs"]
mod brush_image;
pub use brush_image::BrushImage;
pub(crate) use brush_image::fit_stamp_size;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub has_pressure: bool,
    pub tilt: [f32; 2],
    pub velocity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrushOp {
    Paint,
    Erase,
    Pixel,
    BackgroundErase,
    Mixer,
    ColourReplace,
    Clone,
    Heal,
    Undo,
    Sponge,
    Tone,
    Dodge,
    Burn,
    Blur,
    Sharpen,
    Smudge,
    Median,
}

impl BrushOp {
    pub fn uses_sample_source(self) -> bool {
        matches!(self, Self::Clone | Self::Heal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TipKind {
    Round,
    Square,
    Image,
    Intensity,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushTextureMode {
    #[default]
    Nozzle,
    Final,
}

impl BrushTextureMode {
    fn is_nozzle(&self) -> bool { *self == Self::Nozzle }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushTexture {
    pub image: BrushImage,
    pub source_size: [u32; 2],
    pub scale: f32,
    pub enabled: bool,
    pub inverted: bool,
    #[serde(default, skip_serializing_if = "BrushTextureMode::is_nozzle")]
    pub mode: BrushTextureMode,
}

impl BrushTexture {
    pub(crate) fn valid(&self) -> bool {
        self.image.valid()
            && self.source_size.into_iter().zip(self.image.size)
                .all(|(source, sample)| (sample..=4096).contains(&source))
            && (0.01..=10.0).contains(&self.scale)
    }

    fn coverage(&self, pos: [f32; 2]) -> f32 {
        let uv = std::array::from_fn(|axis| {
            (pos[axis] * self.scale / self.source_size[axis] as f32).rem_euclid(1.0)
        });
        let alpha = self.image.tiled_alpha(uv);
        if self.inverted { 1.0 - alpha } else { alpha }
    }

    fn coverage_at(&self, x: u32, y: u32, origin: [u32; 2]) -> f32 {
        self.coverage([origin[0] as f32 + x as f32 + 0.5, origin[1] as f32 + y as f32 + 0.5])
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushWetEdges {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<WetEdgeCurve>,
    #[serde(default, skip_serializing_if = "WetEdgeScope::is_default")]
    pub scope: WetEdgeScope,
}

impl BrushWetEdges {
    pub fn is_default(&self) -> bool { self.enabled.is_none() && self.curve.is_none() && self.scope.is_default() }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WetEdgeScope {
    #[default]
    PerTip,
    WholeStroke,
}

impl WetEdgeScope {
    fn is_default(&self) -> bool { *self == Self::PerTip }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WetEdgeCurve {
    pub points: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub second_derivatives: Vec<f32>,
}

impl WetEdgeCurve {
    pub fn valid(&self) -> bool {
        (2..=64).contains(&self.points.len())
            && self.points.iter().flatten().all(|value| (0.0..=1.0).contains(value))
            && self.points.windows(2).all(|pair| pair[0][0] < pair[1][0])
            && (self.second_derivatives.is_empty()
                || self.second_derivatives.len() == self.points.len()
                    && self.second_derivatives.iter().all(|value| value.is_finite()))
    }

    pub fn sample(&self, alpha: f32) -> f32 {
        let right = self.points.partition_point(|point| point[0] < alpha);
        let value = if right == 0 {
            self.points[0][1]
        } else if right == self.points.len() {
            self.points[right - 1][1]
        } else {
            let [x0, y0] = self.points[right - 1];
            let [x1, y1] = self.points[right];
            let width = x1 - x0;
            let b = (alpha - x0) / width;
            let a = 1.0 - b;
            let linear = a * y0 + b * y1;
            if self.second_derivatives.is_empty() {
                linear
            } else {
                let m0 = self.second_derivatives[right - 1];
                let m1 = self.second_derivatives[right];
                linear + ((a * a * a - a) * m0 + (b * b * b - b) * m1) * width * width / 6.0
            }
        };
        value.clamp(0.0, 1.0)
    }

    fn lookup(&self) -> [u8; 256] {
        std::array::from_fn(|index| (self.sample(index as f32 / 255.0) * 255.0).round() as u8)
    }

    pub fn set_smooth(&mut self, smooth: bool) {
        self.second_derivatives.clear();
        if !smooth || !self.valid() {
            return;
        }

        // Natural spline with zero curvature at the two ends. Eliminate the
        // lower diagonal first, then substitute back from the right.
        let count = self.points.len();
        let mut upper = vec![0.0; count];
        let mut values = vec![0.0; count];
        for index in 1..count - 1 {
            let [[x0, y0], [x1, y1], [x2, y2]] = self.points[index - 1..=index + 1] else { unreachable!() };
            let [x0, y0, x1, y1, x2, y2] = [x0, y0, x1, y1, x2, y2].map(f64::from);
            let left = x1 - x0;
            let right = x2 - x1;
            let diagonal = 2.0 * (left + right) + left * upper[index - 1];
            let slopes = (y2 - y1) / right - (y1 - y0) / left;
            upper[index] = -right / diagonal;
            values[index] = (6.0 * slopes - left * values[index - 1]) / diagonal;
        }
        for index in (1..count - 1).rev() {
            values[index] += upper[index] * values[index + 1];
        }
        self.second_derivatives = values.into_iter().map(|value| value as f32).collect();
    }
}

impl Default for WetEdgeCurve {
    fn default() -> Self {
        Self { points: vec![[0.0, 0.0], [1.0, 1.0]], second_derivatives: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushTip {
    pub id: String,
    pub name: String,
    pub category: String,
    pub kind: TipKind,
    pub spacing: f32,
    pub size_px: f32,
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accumulation: Option<f32>,
    pub flow: f32,
    pub hardness: f32,
    #[serde(default = "full_roundness", skip_serializing_if = "is_full_roundness")]
    pub roundness: f32,
    pub image: Option<BrushImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_images: Vec<BrushImage>,
    #[serde(default, skip_serializing_if = "BrushWetEdges::is_default")]
    pub wet_edges: BrushWetEdges,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<BrushTexture>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_brushes: Vec<SubBrush>,
    #[serde(default, skip_serializing_if = "zero")]
    pub rotation: f32,
    #[serde(default, skip_serializing_if = "BrushDynamics::is_default")]
    pub dynamics: BrushDynamics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubBrush {
    pub tip: BrushTip,
    pub enabled: bool,
    pub sync_size: bool,
    pub sync_spacing: bool,
    #[serde(default, skip_serializing_if = "SubBrushPlacement::is_default")]
    pub placement: SubBrushPlacement,
    #[serde(default, skip_serializing_if = "SubBrushBlend::is_default")]
    pub blending: SubBrushBlend,
    #[serde(default, skip_serializing_if = "SubBrushColorBlend::is_default")]
    pub color_blend: SubBrushColorBlend,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubBrushColorBlend {
    #[default]
    Normal,
    Average,
}

impl SubBrushColorBlend {
    fn is_default(&self) -> bool { *self == Self::Normal }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubBrushPlacement {
    #[default]
    AtNozzle,
    BehindNozzle,
    StrokeEnd,
}

impl SubBrushPlacement {
    fn is_default(&self) -> bool { *self == Self::AtNozzle }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubBrushBlend {
    #[default]
    Blend,
    Erase,
}

impl SubBrushBlend {
    fn is_default(&self) -> bool { *self == Self::Blend }

    fn operation(self, parent: BrushOp) -> BrushOp {
        match self {
            Self::Blend => parent,
            Self::Erase => BrushOp::Erase,
        }
    }
}

pub const MAX_SUB_BRUSHES: usize = 8;

fn zero(value: &f32) -> bool { *value == 0.0 }
fn full_roundness() -> f32 { 1.0 }
fn is_full_roundness(value: &f32) -> bool { *value == full_roundness() }

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControllerKind {
    #[default]
    None,
    Pressure,
    Tilt,
    Velocity,
    Random,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub kind: ControllerKind,
    pub curve: Vec<[f32; 2]>,
    pub jitter: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub requires_pressure: bool,
}

impl Default for Controller {
    fn default() -> Self {
        Self { kind: ControllerKind::None, curve: vec![[0.0, 0.0], [1.0, 1.0]], jitter: 0.0, requires_pressure: false }
    }
}

impl Controller {
    fn valid(&self) -> bool {
        (0.0..=1.0).contains(&self.jitter) && (2..=16).contains(&self.curve.len())
            && self.curve.first().is_some_and(|point| point[0] == 0.0)
            && self.curve.last().is_some_and(|point| point[0] == 1.0)
            && self.curve.iter().flatten().all(|value| (0.0..=1.0).contains(value))
            && self.curve.windows(2).all(|pair| pair[0][0] < pair[1][0])
    }

    fn value(&self, sample: StrokePoint, index: usize, channel: u32) -> Option<f32> {
        let input = match self.kind {
            ControllerKind::None => return None,
            ControllerKind::Pressure if self.requires_pressure && !sample.has_pressure => return None,
            ControllerKind::Pressure => sample.pressure,
            ControllerKind::Tilt => sample.tilt[0].hypot(sample.tilt[1]),
            ControllerKind::Velocity => sample.velocity,
            ControllerKind::Random => noise(index, channel),
        }.clamp(0.0, 1.0);
        let mapped = self.curve.windows(2).find_map(|pair| {
            let [[x0, y0], [x1, y1]] = [pair[0], pair[1]];
            (x0 <= input && input <= x1 && x1 > x0).then(|| y0 + (y1 - y0) * (input - x0) / (x1 - x0))
        }).unwrap_or(input);
        Some((mapped + (2.0 * noise(index, channel + 5) - 1.0) * self.jitter).clamp(0.0, 1.0))
    }
}

fn noise(index: usize, channel: u32) -> f32 {
    let mut bits = (index as u32).wrapping_add(channel.wrapping_mul(0x9e37_79b9));
    bits = (bits ^ (bits >> 16)).wrapping_mul(0x7feb_352d);
    bits = (bits ^ (bits >> 15)).wrapping_mul(0x846c_a68b);
    ((bits ^ (bits >> 16)) >> 8) as f32 / 16_777_215.0
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrushDynamics {
    pub size: Controller,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilt_scale: Option<f32>,
    pub opacity: Controller,
    pub flow: Controller,
    pub hardness: Controller,
    pub rotation: Controller,
    #[serde(skip_serializing_if = "zero")]
    pub direction_rotation: f32,
    #[serde(skip_serializing_if = "zero")]
    pub rotation_jitter: f32,
    #[serde(skip_serializing_if = "ShapeJitter::is_default")]
    pub size_jitter: ShapeJitter,
    #[serde(skip_serializing_if = "ShapeJitter::is_default")]
    pub roundness_jitter: ShapeJitter,
    #[serde(skip_serializing_if = "ScatterAxis::pair_is_default")]
    pub scatter: [ScatterAxis; 2],
    #[serde(skip_serializing_if = "PathScatter::is_default")]
    pub path_scatter: PathScatter,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PathScatter {
    pub amount: f32,
    pub both_axes: bool,
    pub count: u8,
    pub count_jitter: f32,
    pub pressure: bool,
    pub count_pressure: bool,
}

impl Default for PathScatter {
    fn default() -> Self {
        Self { amount: 0.0, both_axes: false, count: 1, count_jitter: 0.0, pressure: false, count_pressure: false }
    }
}

impl PathScatter {
    pub const MAX_COUNT: u8 = 16;

    pub fn is_default(&self) -> bool { *self == Self::default() }

    fn valid(self) -> bool {
        (0.0..=ScatterAxis::MAX_AMOUNT).contains(&self.amount)
            && (1..=Self::MAX_COUNT).contains(&self.count)
            && (0.0..=1.0).contains(&self.count_jitter)
    }

    fn count_at(self, sample: StrokePoint, station: usize) -> u8 {
        if self.count <= 1 { return 1; }
        let pressure = if self.count_pressure && sample.has_pressure { sample.pressure } else { 1.0 };
        let jitter = 1.0 - noise(station, 25) * self.count_jitter;
        1 + ((f32::from(self.count - 1) * pressure * jitter).round() as u8)
    }

    fn offset(self, sample: StrokePoint, index: usize, size: f32, direction: f32) -> [f32; 2] {
        if self.amount == 0.0 { return [0.0; 2]; }
        let strength = if self.pressure && sample.has_pressure { sample.pressure } else { 1.0 };
        let reach = size * self.amount * strength;
        if self.both_axes {
            let (y, x) = (std::f32::consts::TAU * noise(index, 23)).sin_cos();
            let length = reach * noise(index, 24).sqrt();
            [x * length, y * length]
        } else {
            let (sin, cos) = direction.sin_cos();
            let side = reach * (2.0 * noise(index, 24) - 1.0);
            [-sin * side, cos * side]
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShapeJitter {
    pub amount: f32,
    pub minimum: f32,
}

impl ShapeJitter {
    pub fn is_default(&self) -> bool { *self == Self::default() }

    fn valid(self) -> bool {
        (0.0..=1.0).contains(&self.amount) && (0.0..=1.0).contains(&self.minimum)
    }

    fn apply(self, base: f32, random: f32) -> f32 {
        if self.amount == 0.0 { return base; }
        let base = base.max(self.minimum);
        base - (base - self.minimum) * self.amount * random
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScatterAxis {
    pub amount: f32,
    pub pressure: bool,
}

impl ScatterAxis {
    pub const MAX_AMOUNT: f32 = 10.0;

    fn pair_is_default(axes: &[Self; 2]) -> bool { *axes == [Self::default(); 2] }

    fn offset(self, pressure: f32, index: usize, channel: u32) -> f32 {
        if self.amount == 0.0 { return 0.0; }
        let strength = if self.pressure { 1.0 - pressure.clamp(0.0, 1.0) } else { 1.0 };
        (2.0 * noise(index, channel) - 1.0) * self.amount * strength
    }
}

impl BrushDynamics {
    pub fn is_default(&self) -> bool {
        self.controllers().into_iter().all(|controller| controller.kind == ControllerKind::None
            && controller.jitter == 0.0 && controller.curve == [[0.0, 0.0], [1.0, 1.0]])
            && self.tilt_scale.is_none() && self.direction_rotation == 0.0 && self.rotation_jitter == 0.0
            && self.size_jitter.is_default() && self.roundness_jitter.is_default()
            && ScatterAxis::pair_is_default(&self.scatter) && self.path_scatter.is_default()
    }

    pub(crate) fn valid(&self) -> bool {
        self.controllers().into_iter().all(Controller::valid)
            && self.tilt_scale.is_none_or(|scale| (0.0..=2.0).contains(&scale))
            && (0.0..=1.0).contains(&self.direction_rotation)
            && (0.0..=1.0).contains(&self.rotation_jitter)
            && self.size_jitter.valid() && self.roundness_jitter.valid()
            && self.scatter.iter().all(|axis| (0.0..=ScatterAxis::MAX_AMOUNT).contains(&axis.amount))
            && self.path_scatter.valid()
    }

    fn controllers(&self) -> [&Controller; 5] {
        [&self.size, &self.opacity, &self.flow, &self.hardness, &self.rotation]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TipTransform {
    pub radians: f32,
    pub reflect: bool,
}

impl TipTransform {
    fn angle(self, rotation: f32) -> f32 {
        self.radians + if self.reflect { -rotation } else { rotation }
    }

    fn vector(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let x = if self.reflect { -x } else { x };
        let (sin, cos) = self.radians.sin_cos();
        [x * cos - y * sin, x * sin + y * cos]
    }

    fn source_angle(self, dx: f32, dy: f32) -> f32 {
        let (sin, cos) = self.radians.sin_cos();
        let x = dx * cos + dy * sin;
        let y = dy * cos - dx * sin;
        y.atan2(if self.reflect { -x } else { x })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BrushSettings {
    pub size_px: f32,
    pub opacity: f32,
    pub accumulation: Option<f32>,
    pub flow: f32,
    pub hardness: f32,
    pub roundness: f32,
    pub spacing: f32,
    pub pressure_size: bool,
    pub stabilizer: bool,
    pub wet_edges: bool,
    pub wet_edge_curve: Option<WetEdgeCurve>,
    pub wet_edge_scope: WetEdgeScope,
    pub protect_alpha: bool,
    pub pixel_perfect: bool,
    pub colour: [u8; 4],
    pub tip: Option<String>,
    pub tip_kind: TipKind,
    pub stamp: Option<BrushImage>,
    pub additional_stamps: Vec<BrushImage>,
    pub texture: Option<BrushTexture>,
    pub sub_brushes: Vec<SubBrush>,
    pub sub_brush_color_blend: SubBrushColorBlend,
    pub stamp_colour: bool,
    pub rotation: f32,
    pub tip_transform: TipTransform,
    pub dynamics: BrushDynamics,
    pub symmetry: u32,
    pub mirror: bool,
    pub symmetry_lock: bool,
    pub symmetry_origin: [f32; 2],
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            size_px: 64.0,
            opacity: 1.0,
            accumulation: None,
            flow: 1.0,
            hardness: 0.8,
            roundness: 1.0,
            spacing: 0.1,
            pressure_size: false,
            stabilizer: false,
            wet_edges: false,
            wet_edge_curve: None,
            wet_edge_scope: WetEdgeScope::PerTip,
            protect_alpha: false,
            pixel_perfect: false,
            colour: [0, 0, 0, 255],
            tip: None,
            tip_kind: TipKind::Round,
            stamp: None,
            additional_stamps: Vec::new(),
            texture: None,
            sub_brushes: Vec::new(),
            sub_brush_color_blend: SubBrushColorBlend::Normal,
            stamp_colour: true,
            rotation: 0.0,
            tip_transform: TipTransform::default(),
            dynamics: BrushDynamics::default(),
            symmetry: 1,
            mirror: false,
            symmetry_lock: false,
            symmetry_origin: [0.5; 2],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TonalRange {
    Shadows,
    Midtones,
    Highlights,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpongeMode {
    Saturate,
    Desaturate,
    Vibrance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SharpenMode {
    Clarity,
    UnsharpMask,
    Harsh,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpParams {
    pub tonal_range: TonalRange,
    pub filter_tonal_range: Option<TonalRange>,
    pub protect_hue: bool,
    pub sponge_mode: SpongeMode,
    pub sharpen_mode: SharpenMode,
    pub strength: f32,
    pub tolerance: f32,
    pub contiguous: bool,
    pub sample_continuously: bool,
    pub load_brush: bool,
    pub clean_brush: bool,
    pub pigment: bool,
}

impl Default for OpParams {
    fn default() -> Self {
        Self {
            tonal_range: TonalRange::Midtones,
            filter_tonal_range: None,
            protect_hue: false,
            sponge_mode: SpongeMode::Saturate,
            sharpen_mode: SharpenMode::Clarity,
            strength: 1.0,
            tolerance: 0.0,
            contiguous: true,
            sample_continuously: false,
            load_brush: false,
            clean_brush: false,
            pigment: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MixerLoad {
    #[default]
    Fresh,
    Clean,
    Colour([u8; 4]),
}

impl MixerLoad {
    pub(crate) fn colour(self) -> Option<[u8; 4]> {
        match self {
            Self::Colour(colour) => Some(colour),
            Self::Fresh | Self::Clean => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dab {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub alpha: f32,
    pub hardness: f32,
    roundness: f32,
    height_scale: f32,
    basis: [f32; 2],
    reflect: bool,
    stamp_index: usize,
    distance: f32,
}

impl Dab {
    fn at(sample: StrokePoint, settings: &BrushSettings, index: usize, distance: f32, direction: f32) -> Self {
        let dynamics = &settings.dynamics;
        let tilt_size = dynamics.tilt_scale.filter(|_| dynamics.size.kind == ControllerKind::Tilt && !settings.pressure_size);
        let pen = sample.has_pressure || sample.tilt != [0.0; 2];
        let size = if settings.pressure_size {
            sample.pressure
        } else if tilt_size.is_some() && !pen {
            1.0
        } else {
            dynamics.size.value(sample, index, 1).unwrap_or(1.0)
        };
        let opacity = dynamics.opacity.value(sample, index, 2).unwrap_or(1.0);
        let flow = dynamics.flow.value(sample, index, 3).unwrap_or(1.0);
        let hardness = dynamics.hardness.value(sample, index, 4).unwrap_or(1.0);
        let rotation = settings.rotation + 360.0 * dynamics.rotation.value(sample, index, 5).unwrap_or(0.0)
            + 360.0 * (noise(index, 17) - 0.5) * dynamics.rotation_jitter;
        let angle = rotation.to_radians() + direction * dynamics.direction_rotation;
        let (sin, cos) = settings.tip_transform.angle(angle).sin_cos();
        let size = dynamics.size_jitter.apply(size, noise(index, 18));
        let size = settings.size_px.max(1.0) * size.clamp(0.01, 1.0);
        let roundness = dynamics.roundness_jitter.apply(settings.roundness, noise(index, 19)).clamp(0.01, 1.0);
        let tilt_height = tilt_size.filter(|scale| *scale > 0.0).unwrap_or(1.0);
        let tilt_amount = sample.tilt[0].hypot(sample.tilt[1]).min(1.0);
        let [along_x, along_y] = dynamics.path_scatter.offset(sample, index, size, direction);
        let [x, y] = settings.tip_transform.vector([
            along_x + size * 0.5 * dynamics.scatter[0].offset(sample.pressure, index, 11),
            along_y + size * 0.5 * dynamics.scatter[1].offset(sample.pressure, index, 12),
        ]);
        Self {
            x: sample.x + x,
            y: sample.y + y,
            size,
            alpha: settings.flow * flow * settings.accumulation.unwrap_or(settings.opacity) * opacity,
            hardness: settings.hardness * hardness,
            roundness,
            height_scale: 1.0 + (tilt_height - 1.0) * tilt_amount,
            basis: [cos, sin],
            reflect: settings.tip_transform.reflect,
            stamp_index: index % (1 + settings.additional_stamps.len()),
            distance,
        }
    }

    fn extent(&self, kind: TipKind) -> f32 {
        let radius = self.size * 0.5;
        let diameter_extent = if kind == TipKind::Round {
            radius + ROUND_EDGE_PADDING
        } else {
            radius * (self.basis[0].abs() + self.basis[1].abs())
        };
        diameter_extent * self.height_scale.max(1.0)
    }

    fn distance(&self, kind: TipKind, dx: f32, dy: f32) -> f32 {
        match kind {
            TipKind::Square | TipKind::Image | TipKind::Intensity => {
                let [cos, sin] = self.basis;
                let ratio = if kind == TipKind::Square { self.roundness } else { 1.0 };
                (dx * cos + dy * sin).abs().max((dy * cos - dx * sin).abs() / (ratio * self.height_scale))
            }
            TipKind::Round => dx.hypot(dy),
        }
    }
    fn ink(&self, settings: &BrushSettings, dx: f32, dy: f32) -> Option<([u8; 4], f32)> {
        let image = if self.stamp_index == 0 { settings.stamp.as_ref() }
            else { settings.additional_stamps.get(self.stamp_index - 1) };
        if let Some(image) = image {
            let radius = self.size * 0.5;
            let [cos, sin] = self.basis;
            let [mut w, mut h] = image.aspect().map(|side| side * radius);
            if self.roundness < 1.0 {
                if w < h { w *= self.roundness; } else { h *= self.roundness; }
            }
            h *= self.height_scale;
            let x = (dx * cos + dy * sin) * if self.reflect { -1.0 } else { 1.0 };
            let texel = image.sample([0.5 + x / (2.0 * w),
                0.5 + (dy * cos - dx * sin) / (2.0 * h)])?;
            let alpha = f32::from(texel[3]) / 255.0;
            let strength = if settings.tip_kind == TipKind::Intensity {
                alpha * luminance([texel[0], texel[1], texel[2]].map(|channel| f32::from(channel) / 255.0))
            } else {
                alpha
            };
            let colour = if settings.tip_kind == TipKind::Image && settings.stamp_colour {
                [texel[0], texel[1], texel[2], 255]
            } else {
                settings.colour
            };
            return Some((colour, strength));
        }

        let radius = self.size * 0.5;
        if settings.tip_kind == TipKind::Round {
            let distance = if self.roundness < 1.0 || self.height_scale != 1.0 {
                let [cos, sin] = self.basis;
                let minor = self.roundness.max(0.5 / radius).clamp(0.01, 1.0) * self.height_scale;
                (dx * cos + dy * sin).hypot((dy * cos - dx * sin) / minor)
            } else {
                dx.hypot(dy)
            };
            return round_coverage(radius, self.hardness, distance)
                .map(|strength| (settings.colour, strength));
        }
        let distance = self.distance(settings.tip_kind, dx, dy);
        if distance > radius { return None; }
        let core = radius * self.hardness.clamp(0.0, 1.0);
        let strength = if distance <= core { 1.0 } else { (radius - distance) / (radius - core).max(0.001) };
        Some((settings.colour, strength))
    }
}

const ROUND_EDGE_PADDING: f32 = 1.0;

fn round_coverage(radius: f32, hardness: f32, distance: f32) -> Option<f32> {
    if distance > radius + ROUND_EDGE_PADDING {
        return None;
    }
    // Hardness sets the solid core. A raster-sized transition keeps the
    // hardest tips antialiased, including when their centers are fractional.
    let width = (radius * (1.0 - hardness.clamp(0.0, 1.0))).max(radius.min(2.0));
    let t = ((distance - (radius - width + 0.5)) / width).max(0.0);
    Some(if t == 0.0 { 1.0 } else { (-6.0 * t * t).exp() })
}

fn round_core(radius: f32, hardness: f32) -> f32 {
    radius - (radius * (1.0 - hardness.clamp(0.0, 1.0))).max(radius.min(2.0)) + 0.5
}

impl BrushSettings {
    pub(crate) fn equivalent_tip_transform(&self, a: TipTransform, b: TipTransform) -> bool {
        if a == b { return true; }
        let equivalent = |kind: TipKind, roundness: f32, rotation: f32, dynamics: &BrushDynamics| {
            if dynamics.path_scatter.amount > 0.0 || dynamics.scatter.iter().any(|axis| axis.amount > 0.0) { return false; }
            let scaled = dynamics.size.kind == ControllerKind::Tilt
                && dynamics.tilt_scale.is_some_and(|scale| scale > 0.0 && scale != 1.0);
            let unflattened = !scaled && roundness == 1.0 && dynamics.roundness_jitter.amount == 0.0;
            if kind == TipKind::Round && unflattened { return true; }
            let angle = a.angle(rotation.to_radians()) - b.angle(rotation.to_radians());
            if matches!(kind, TipKind::Square | TipKind::Round) {
                let rotates = dynamics.rotation.kind != ControllerKind::None
                    || dynamics.direction_rotation != 0.0 || dynamics.rotation_jitter != 0.0;
                let symmetry = if kind == TipKind::Square && unflattened { 2.0 } else { 1.0 };
                return (!rotates || a.reflect == b.reflect) && (angle * symmetry).sin().abs() < 0.0001;
            }
            a == b
        };
        equivalent(self.tip_kind, self.roundness, self.rotation, &self.dynamics)
            && self.sub_brushes.iter().filter(|sub| sub.enabled).all(|sub| {
                let tip = &sub.tip;
                equivalent(tip.kind, tip.roundness, tip.rotation, &tip.dynamics)
            })
    }

    pub(crate) fn max_radius(&self) -> f32 {
        self.sub_brushes.iter().filter(|sub| sub.enabled).fold(self.tip_reach(), |radius, sub| {
            radius.max(self.sub_settings(sub).tip_reach())
        })
    }

    fn sub_settings(&self, sub: &SubBrush) -> Self {
        let tip = &sub.tip;
        let size_px = if sub.sync_size { self.size_px } else { tip.size_px };
        Self {
            size_px,
            spacing: if sub.sync_spacing { self.spacing * self.size_px / size_px.max(1.0) } else { tip.spacing },
            opacity: self.opacity * tip.opacity,
            accumulation: tip.accumulation,
            flow: tip.flow,
            hardness: tip.hardness,
            roundness: tip.roundness,
            dynamics: tip.dynamics.clone(),
            tip_kind: tip.kind,
            rotation: tip.rotation,
            tip_transform: self.tip_transform,
            stamp: tip.image.clone(),
            additional_stamps: tip.additional_images.clone(),
            texture: tip.texture.clone(),
            sub_brush_color_blend: sub.color_blend,
            colour: self.colour,
            stamp_colour: self.stamp_colour,
            protect_alpha: self.protect_alpha,
            stabilizer: self.stabilizer,
            pressure_size: sub.sync_size && self.pressure_size,
            wet_edges: tip.wet_edges.enabled.unwrap_or(self.wet_edges),
            wet_edge_curve: tip.wet_edges.curve.clone(),
            ..Self::default()
        }
    }

    fn tip_radius(&self) -> f32 {
        let height = if self.dynamics.size.kind == ControllerKind::Tilt {
            self.dynamics.tilt_scale.unwrap_or(1.0).max(1.0)
        } else { 1.0 };
        if self.tip_kind == TipKind::Round {
            return (self.size_px * 0.5 + ROUND_EDGE_PADDING) * height;
        }
        let angle = self.tip_transform.angle(self.rotation.to_radians());
        let factor = if self.dynamics.rotation.kind != ControllerKind::None
            || self.dynamics.direction_rotation > 0.0 || self.dynamics.rotation_jitter > 0.0
        {
            std::f32::consts::SQRT_2
        } else {
            angle.cos().abs() + angle.sin().abs()
        };
        self.size_px * 0.5 * factor * height
    }

    fn tip_reach(&self) -> f32 {
        let [x, y] = self.dynamics.scatter.map(|axis| axis.amount);
        self.tip_radius() + self.size_px.max(1.0) * (0.5 * x.hypot(y) + self.dynamics.path_scatter.amount)
    }
}

pub fn dabs(points: &[StrokePoint], s: &BrushSettings) -> Vec<Dab> {
    let mut result = Vec::new();
    if points.is_empty() {
        return result;
    }

    let stabilized;
    let points = if s.stabilizer && points.len() > 2 {
        stabilized = stabilize(points, s.size_px);
        stabilized.as_slice()
    } else {
        points
    };
    let base_size = s.size_px.max(1.0);
    let spacing_dist = (s.spacing * base_size).max(1.0);

    let follows = s.dynamics.direction_rotation > 0.0 || s.dynamics.path_scatter.amount > 0.0;
    let mut direction = if follows {
        points.windows(2).find_map(|pair| {
            let dx = pair[1].x - pair[0].x;
            let dy = pair[1].y - pair[0].y;
            (dx.hypot(dy) >= 0.001).then(|| s.tip_transform.source_angle(dx, dy))
        }).unwrap_or(0.0)
    } else { 0.0 };
    let mut last_center = points[0];
    let mut station = 0;
    let mut emit = |center, distance, direction| {
        for _ in 0..s.dynamics.path_scatter.count_at(center, station) {
            result.push(Dab::at(center, s, result.len(), distance, direction));
        }
        station += 1;
    };
    emit(last_center, 0.0, direction);

    let mut to_next = spacing_dist;
    let mut travelled = 0.0;

    for pair in points.windows(2) {
        let [from, to] = [pair[0], pair[1]];
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let seg_len = dx.hypot(dy);

        if !seg_len.is_finite() || seg_len < 0.001 {
            continue;
        }
        if follows { direction = s.tip_transform.source_angle(dx, dy); }

        while to_next <= seg_len {
            let t = to_next / seg_len;
            let mix = |a, b| a + (b - a) * t;
            let sample = StrokePoint {
                x: from.x + dx * t,
                y: from.y + dy * t,
                pressure: mix(from.pressure, to.pressure),
                has_pressure: from.has_pressure || to.has_pressure,
                tilt: std::array::from_fn(|axis| mix(from.tilt[axis], to.tilt[axis])),
                velocity: mix(from.velocity, to.velocity),
            };
            last_center = sample;
            emit(sample, travelled + to_next, direction);
            to_next += spacing_dist;
        }
        to_next -= seg_len;
        travelled += seg_len;
    }

    let end = points[points.len() - 1];
    if (last_center.x - end.x).abs() >= 0.001 || (last_center.y - end.y).abs() >= 0.001 {
        emit(end, travelled, direction);
    }

    result
}

fn stabilize(points: &[StrokePoint], width: f32) -> Vec<StrokePoint> {
    let mut output = points.to_vec();
    let reach = (width * 0.25).max(1.0);
    for (point, window) in output[1..points.len() - 1].iter_mut().zip(points.windows(3)) {
        let [before, center, after] = [window[0], window[1], window[2]];
        let weight = |point: StrokePoint| 0.5 * (1.0 - (point.x - center.x).hypot(point.y - center.y) / reach).max(0.0);
        let (left, right) = (weight(before), weight(after));
        let total = 1.0 + left + right;
        point.x = (center.x + before.x * left + after.x * right) / total;
        point.y = (center.y + before.y * left + after.y * right) / total;
    }
    output
}

pub fn stroke(
    dst: &mut RasterViewMut,
    points: &[StrokePoint],
    s: &BrushSettings,
    op: BrushOp,
    p: &OpParams,
) -> Option<[u32; 4]> {
    stroke_at(dst, points, s, op, p, [0; 2])
}

pub(crate) fn stroke_at(
    dst: &mut RasterViewMut,
    points: &[StrokePoint],
    s: &BrushSettings,
    op: BrushOp,
    p: &OpParams,
    raster_origin: [u32; 2],
) -> Option<[u32; 4]> {
    if points.is_empty() {
        return None;
    }

    let dab_list = dabs(points, s);
    if dab_list.is_empty() {
        return None;
    }

    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;

    let compound = matches!(op, BrushOp::Paint | BrushOp::Erase)
        && s.sub_brushes.iter().any(|sub| sub.enabled);
    let compound_radius = compound.then(|| s.max_radius());
    for dab in &dab_list {
        let half_size = compound_radius.unwrap_or_else(|| dab.extent(s.tip_kind));
        let x_min = (dab.x - half_size).floor().max(0.0) as u32;
        let y_min = (dab.y - half_size).floor().max(0.0) as u32;
        let x_max = (dab.x + half_size).ceil() as u32;
        let y_max = (dab.y + half_size).ceil() as u32;

        if x_min < min_x {
            min_x = x_min;
        }
        if y_min < min_y {
            min_y = y_min;
        }
        if x_max > max_x {
            max_x = x_max;
        }
        if y_max > max_y {
            max_y = y_max;
        }
    }

    let w = max_x.saturating_sub(min_x) + 1;
    let h = max_y.saturating_sub(min_y) + 1;

    if w == 0 || h == 0 {
        return None;
    }

    let stroke_opacity = if s.accumulation.is_some() { s.opacity } else { 1.0 };
    let whole_wet = compound && op == BrushOp::Paint && s.wet_edges && s.wet_edge_scope == WetEdgeScope::WholeStroke;
    if op == BrushOp::Mixer {
        let mut colour = (p.load_brush && !p.clean_brush).then_some(s.colour).filter(|colour| colour[3] != 0);
        mix_dabs(dst, &dab_list, s, p.strength, &mut colour, false, raster_origin);
    } else if matches!(op, BrushOp::BackgroundErase | BrushOp::ColourReplace) {
        sampled_stroke(dst, &dab_list, s, p, op);
    } else if is_filter(op) {
        filters::apply(dst, [dab_list].into_iter(), s, p, op);
    } else if matches!(op, BrushOp::Dodge | BrushOp::Burn | BrushOp::Sponge) {
        adjustment_stroke(dst, &dab_list, s, p, op);
    } else if compound && (whole_wet || stroke_opacity < 1.0 || s.sub_brushes.iter().any(|sub| sub.enabled
        && (sub.blending == SubBrushBlend::Erase
            || (op == BrushOp::Paint && sub.color_blend == SubBrushColorBlend::Average))))
    {
        // Sub-brush masking and color blending operate on the current stroke.
        let mut pixels = vec![0; dst.rgba.len()];
        let mut ink = stroke_ink(s, op);
        if s.accumulation.is_some() { ink.opacity = 1.0; }
        if whole_wet {
            // The inherited wet setting is applied after combining the tips.
            // A child explicitly set to wet still has its own finish.
            ink.wet_edges = false;
        }
        let mut stroke = RasterViewMut::new(dst.width, dst.height, &mut pixels);
        paint_brushes(&mut stroke, points, dab_list, &ink, BrushOp::Paint, raster_origin);
        let width = dst.width.max(1) as usize;
        let wet = whole_wet.then(|| wet_edge_lookup(s));
        for (index, mut pixel) in pixels.as_chunks::<4>().0.iter().copied().enumerate() {
            if pixel[3] != 0 {
                if let Some(table) = &wet {
                    pixel[3] = table[usize::from(pixel[3])];
                }
                composite_ink(dst, (index % width) as u32, (index / width) as u32, pixel, stroke_opacity, op, s.protect_alpha);
            }
        }
    } else {
        paint_brushes(dst, points, dab_list, s, op, raster_origin);
    }

    Some([min_x, min_y, w, h])
}

fn paint_brushes(
    dst: &mut RasterViewMut, points: &[StrokePoint], main_dabs: Vec<Dab>,
    settings: &BrushSettings, op: BrushOp, origin: [u32; 2],
) {
    paint_with_finish(dst, points, main_dabs, settings, op, origin);
    if matches!(op, BrushOp::Paint | BrushOp::Erase) {
        for sub in settings.sub_brushes.iter().filter(|sub| sub.enabled && sub.placement == SubBrushPlacement::StrokeEnd) {
            let child = settings.sub_settings(sub);
            paint_with_finish(dst, points, dabs(points, &child), &child, sub.blending.operation(op), origin);
        }
    }
}

fn paint_with_finish(
    dst: &mut RasterViewMut, points: &[StrokePoint], dabs: Vec<Dab>,
    settings: &BrushSettings, op: BrushOp, origin: [u32; 2],
) {
    if let Some(finish) = StrokeFinish::for_tip(settings, op, true, !has_interleaved_children(settings)) {
        paint_stroke_layer(dst, points, dabs, settings, op, origin, finish);
    } else {
        paint_stroke_dabs(dst, points, dabs, settings, op, origin);
    }
}

fn paint_stroke_dabs(
    dst: &mut RasterViewMut, points: &[StrokePoint], dabs: Vec<Dab>,
    settings: &BrushSettings, op: BrushOp, origin: [u32; 2],
) {
    if matches!(op, BrushOp::Paint | BrushOp::Erase)
        && has_interleaved_children(settings)
    {
        paint_sub_brushes(dst, points, dabs, settings, op, origin);
    } else {
        for dab in &dabs {
            paint_dab(dst, dab, settings, op, origin);
        }
    }
}

fn has_interleaved_children(settings: &BrushSettings) -> bool {
    settings.sub_brushes.iter().any(|sub| sub.enabled && sub.placement != SubBrushPlacement::StrokeEnd)
}

fn paint_stroke_layer(
    dst: &mut RasterViewMut, points: &[StrokePoint], dabs: Vec<Dab>,
    settings: &BrushSettings, op: BrushOp, origin: [u32; 2], finish: StrokeFinish<'_>,
) {
    // Wet edges and final grain operate on the accumulated stroke coverage.
    let mut pixels = vec![0; dst.rgba.len()];
    let mut ink = stroke_ink(settings, op);
    ink.sub_brush_color_blend = SubBrushColorBlend::Normal;
    if finish.grain.is_some() {
        ink.texture = None;
    }
    let mut stroke = RasterViewMut::new(dst.width, dst.height, &mut pixels);
    paint_stroke_dabs(&mut stroke, points, dabs, &ink, BrushOp::Paint, origin);

    if dst.width == 0 || dst.height == 0 { return; }
    let width = dst.width as usize;
    for (index, [r, g, b, mut alpha]) in pixels.as_chunks::<4>().0.iter().copied().enumerate() {
        if alpha == 0 { continue; }
        if let Some(curve) = &finish.alpha {
            alpha = channel8(curve.apply(f32::from(alpha) / 255.0));
        }
        let (x, y) = ((index % width) as u32, (index / width) as u32);
        let coverage = finish.grain.map_or(1.0, |texture| texture.coverage_at(x, y, origin));
        if coverage <= 0.0 { continue; }
        if op == BrushOp::Paint && settings.sub_brush_color_blend == SubBrushColorBlend::Average {
            paint_average(dst, x, y, [r, g, b, alpha], coverage);
        } else {
            composite_ink(dst, x, y, [r, g, b, alpha], coverage, op, settings.protect_alpha);
        }
    }
}

fn stroke_ink(settings: &BrushSettings, op: BrushOp) -> BrushSettings {
    let mut ink = settings.clone();
    ink.protect_alpha = false;
    ink.pixel_perfect = false;
    if op == BrushOp::Erase {
        ink.stamp_colour = false;
        ink.colour = [255; 4];
        ink.wet_edges = false;
        for sub in &mut ink.sub_brushes {
            sub.tip.wet_edges = BrushWetEdges::default();
        }
    }
    ink
}

fn composite_ink(dst: &mut RasterViewMut, x: u32, y: u32, pixel: [u8; 4], coverage: f32, op: BrushOp, protect_alpha: bool) {
    if op == BrushOp::Erase {
        erase_pixel(dst, x, y, coverage * f32::from(pixel[3]) / 255.0);
    } else {
        paint_pixel(dst, x, y, pixel, coverage, false, protect_alpha);
    }
}

fn paint_sub_brushes(
    dst: &mut RasterViewMut,
    points: &[StrokePoint],
    base_dabs: Vec<Dab>,
    base: &BrushSettings,
    op: BrushOp,
    origin: [u32; 2],
) {
    let settings: Vec<_> = base.sub_brushes.iter()
        .filter(|sub| sub.enabled && sub.placement != SubBrushPlacement::StrokeEnd)
        .map(|sub| (sub.placement, sub.blending.operation(op), base.sub_settings(sub))).collect();
    let mut events: Vec<_> = base_dabs.into_iter().map(|dab| (dab, 1, 0)).collect();
    for (index, (placement, _, settings)) in settings.iter().enumerate() {
        let phase = if *placement == SubBrushPlacement::BehindNozzle { 0 } else { 2 };
        events.extend(dabs(points, settings).into_iter().map(|dab| (dab, phase, index)));
    }
    // On the same position, paint behind tips, the main tip, and then the
    // current-position tips. Within each phase the user's list order wins.
    events.sort_by(|(a, phase_a, index_a), (b, phase_b, index_b)| {
        a.distance.total_cmp(&b.distance).then(phase_a.cmp(phase_b)).then(index_a.cmp(index_b))
    });
    let mut finishes = Vec::new();
    let mut add_finish = |brush, operation, is_main: bool| {
        let finish = StrokeFinish::for_tip(brush, operation, !is_main, true)?;
        let slot = finishes.len();
        finishes.push(finish);
        Some(slot)
    };
    let main_slot = add_finish(base, op, true);
    let slots: Vec<_> = settings.iter().map(|(_, op, brush)| add_finish(brush, *op, false)).collect();
    if !finishes.is_empty() {
        let events = events.into_iter().filter_map(|(dab, phase, index)| {
            let (op, brush, slot) = if phase == 1 { (op, base, main_slot) }
                else { (settings[index].1, &settings[index].2, slots[index]) };
            if dab.alpha <= 0.0 { return None; }
            let bounds = dab_bounds(&dab, brush.tip_kind, [dst.width, dst.height]);
            let nozzle = texture_for(brush, BrushTextureMode::Nozzle);
            Some(FinishedDab { dab, brush, op, slot, bounds, nozzle })
        }).collect::<Vec<_>>();
        paint_interleaved_finish(dst, &events, &finishes, base, op, origin);
        return;
    }
    for (dab, phase, index) in events {
        let (brush_op, brush) = if phase == 1 { (op, base) } else { (settings[index].1, &settings[index].2) };
        paint_dab(dst, &dab, brush, brush_op, origin);
    }
}

struct StrokeFinish<'a> {
    grain: Option<&'a BrushTexture>,
    alpha: Option<StrokeAlpha>,
}

struct StrokeAlpha {
    limit: f32,
    opacity: f32,
    wet: Option<[u8; 256]>,
}

impl StrokeAlpha {
    fn apply(&self, raw: f32) -> f32 {
        let capped = raw.min(self.limit);
        let ink = self.wet.as_ref().map_or(capped, |table| {
            f32::from(table[usize::from(channel8(capped))]) / 255.0
        });
        ink * self.opacity
    }
}

fn wet_edge_lookup(brush: &BrushSettings) -> [u8; 256] {
    if let Some(curve) = brush.wet_edge_curve.as_ref().filter(|curve| curve.valid()) {
        return curve.lookup();
    }
    // The default profile darkens thin ink toward a peak at 25% coverage,
    // then tapers to 75% opacity in the saturated interior of the stroke.
    std::array::from_fn(|alpha| {
        let coverage = alpha as f32 / 255.0;
        channel8((4.0 * coverage).min((1.125 - 0.5 * coverage).max(0.75)))
    })
}

impl StrokeFinish<'_> {
    fn for_tip(brush: &BrushSettings, op: BrushOp, with_grain: bool, with_alpha: bool) -> Option<StrokeFinish<'_>> {
        if !matches!(op, BrushOp::Paint | BrushOp::Erase) { return None; }
        let grain = if with_grain { texture_for(brush, BrushTextureMode::Final) } else { None };
        let limit = brush.accumulation.unwrap_or(1.0);
        let opacity = if brush.accumulation.is_some() { brush.opacity } else { 1.0 };
        let wet = (with_alpha && op == BrushOp::Paint && brush.wet_edges).then(|| wet_edge_lookup(brush));
        let alpha = (with_alpha && (limit < 1.0 || opacity < 1.0 || wet.is_some()))
            .then_some(StrokeAlpha { limit, opacity, wet });
        (grain.is_some() || alpha.is_some()).then_some(StrokeFinish { grain, alpha })
    }
}

fn stroke_alpha_ratios(
    finish: &StrokeAlpha, events: &[FinishedDab], active: &[usize], slot: usize,
    width: usize, band: std::ops::Range<u32>, origin: [u32; 2],
) -> Vec<f32> {
    let mut raw = vec![0.0; width * band.len()];
    for event in active.iter().map(|&index| &events[index]).filter(|event| event.slot == Some(slot)) {
        event.visit(band.start, band.end, origin, |x, y, _, alpha| {
            let current = &mut raw[(y - band.start) as usize * width + x as usize];
            *current += (1.0 - *current) * alpha;
        });
    }
    for opacity in &mut raw {
        *opacity = if *opacity > 0.0 { finish.apply(*opacity) / *opacity } else { 0.0 };
    }
    raw
}

struct FinishedDab<'a> {
    dab: Dab,
    brush: &'a BrushSettings,
    op: BrushOp,
    slot: Option<usize>,
    bounds: [u32; 4],
    nozzle: Option<&'a BrushTexture>,
}

impl FinishedDab<'_> {
    fn averages_color(&self) -> bool {
        self.op == BrushOp::Paint && self.brush.sub_brush_color_blend == SubBrushColorBlend::Average
    }

    fn visit(&self, top: u32, bottom: u32, origin: [u32; 2], mut ink: impl FnMut(u32, u32, [u8; 4], f32)) {
        let [left, y0, right, y1] = self.bounds;
        for y in y0.max(top)..y1.min(bottom) {
            for x in left..right {
                let Some((colour, falloff)) = self.dab.ink(
                    self.brush, x as f32 - self.dab.x, y as f32 - self.dab.y,
                ) else { continue; };
                let mut alpha = self.dab.alpha * falloff;
                if self.op == BrushOp::Paint { alpha *= f32::from(colour[3]) / 255.0; }
                if let Some(nozzle) = self.nozzle { alpha *= nozzle.coverage_at(x, y, origin); }
                if alpha > 0.0 { ink(x, y, colour, alpha); }
            }
        }
    }
}

fn paint_interleaved_finish(
    dst: &mut RasterViewMut, events: &[FinishedDab], finishes: &[StrokeFinish],
    base: &BrushSettings, op: BrushOp, origin: [u32; 2],
) {
    if events.is_empty() || dst.width == 0 || dst.height == 0 { return; }
    let width = dst.width as usize;
    let band_height = (262_144 / dst.width).clamp(1, 128);
    let mut by_top: Vec<_> = (0..events.len()).collect();
    by_top.sort_unstable_by_key(|&index| events[index].bounds[1]);
    let mut incoming = by_top.into_iter().peekable();
    let mut active: Vec<usize> = Vec::new();
    let mut band = 0;
    let mut ink = Vec::new();
    let mut future = Vec::new();
    let has_average = op == BrushOp::Paint && events.iter().any(FinishedDab::averages_color);
    let mut average = Vec::new();
    while band < dst.height {
        let end = band.saturating_add(band_height).min(dst.height);
        active.retain(|&index| events[index].bounds[3] > band);
        while let Some(index) = incoming.next_if(|&index| events[index].bounds[1] < end) {
            active.push(index);
        }
        if !active.is_empty() {
            active.sort_unstable();
            let area = width * (end - band) as usize;
            // Premultiplied RGB, ink alpha and remaining destination coverage.
            ink.resize(area, [0.0; 5]);
            ink.fill([0.0, 0.0, 0.0, 0.0, 1.0]);
            if has_average {
                average.resize(area, ReverseAverage::default());
                average.fill(ReverseAverage::default());
            }
            future.resize(area * finishes.len(), 1.0);
            future.fill(1.0);
            let alpha_ratios: Vec<_> = finishes.iter().enumerate().map(|(slot, finish)| {
                finish.alpha.as_ref().map(|alpha| stroke_alpha_ratios(alpha, events, &active, slot, width, band..end, origin))
            }).collect();
            for &index in active.iter().rev() {
                let event = &events[index];
                event.visit(band, end, origin, |x, y, colour, mut alpha| {
                    let index = (y - band) as usize * width + x as usize;
                    let remaining = ink[index][4];
                    if remaining <= 0.0 && !has_average { return; }
                    if let Some(slot) = event.slot {
                        let grain = finishes[slot].grain.map_or(1.0, |grain| grain.coverage_at(x, y, origin));
                        let scale = grain * alpha_ratios[slot].as_ref().map_or(1.0, |ratios| ratios[index]);
                        let transmission = &mut future[slot * area + index];
                        // Rescale the dab's remaining contribution so the whole
                        // stroke has coverage scale * (1 - transmission).
                        // Later dabs stay in their original order across brushes.
                        let above = *transmission;
                        *transmission *= 1.0 - alpha;
                        if scale != 1.0 {
                            let denominator = 1.0 - scale * (1.0 - above);
                            alpha = if denominator > 0.0 {
                                (alpha * scale * above / denominator).clamp(0.0, 1.0)
                            } else { 0.0 };
                        }
                    }
                    let contribution = remaining * alpha;
                    let paint = event.op == BrushOp::Paint;
                    if has_average {
                        average[index].apply(&mut ink[index], colour, alpha, paint, event.averages_color());
                    } else if paint {
                        for (out, source) in ink[index].iter_mut().zip(colour).take(3) {
                            *out += f32::from(source) * contribution;
                        }
                    }
                    if paint {
                        ink[index][3] += contribution;
                    }
                    ink[index][4] = remaining * (1.0 - alpha);
                });
            }
            for (index, &[r, g, b, alpha, remaining]) in ink.iter().enumerate() {
                let x = (index % width) as u32;
                let y = band + (index / width) as u32;
                if op == BrushOp::Erase {
                    if remaining < 1.0 { erase_pixel(dst, x, y, 1.0 - remaining); }
                } else if alpha > 0.0 {
                    let [r, g, b] = [r, g, b].map(|channel| (channel / alpha).round() as u8);
                    paint_pixel(dst, x, y, [r, g, b, 255], alpha.clamp(0.0, 1.0), false, base.protect_alpha);
                }
            }
        }
        band = end;
    }
}

#[derive(Clone, Copy)]
struct ReverseAverage {
    rgb_transmission: f32,
    alpha_to_rgb: [f32; 3],
}

impl Default for ReverseAverage {
    fn default() -> Self {
        Self { rgb_transmission: 1.0, alpha_to_rgb: [0.0; 3] }
    }
}

impl ReverseAverage {
    fn apply(&mut self, ink: &mut [f32; 5], color: [u8; 4], alpha: f32, paint: bool, average: bool) {
        // In premultiplied color, averaging only the overlap is affine in
        // both the incoming RGB and alpha. Compose that transform backwards
        // so the effective wet/grain coverage preserves the order of dabs.
        let transmission = 1.0 - alpha;
        let overlap = if average { alpha * 0.5 } else { 0.0 };
        for (channel, source) in color.into_iter().take(3).enumerate() {
            let source = f32::from(source) * self.rgb_transmission;
            let old = self.alpha_to_rgb[channel];
            if paint { ink[channel] += alpha * (source + old); }
            self.alpha_to_rgb[channel] = transmission * old - overlap * source;
        }
        self.rgb_transmission *= transmission + overlap;
    }
}

pub(crate) fn independent_dabs(settings: &BrushSettings, op: BrushOp) -> bool {
    matches!(op, BrushOp::Paint | BrushOp::Erase)
        && !settings.sub_brushes.iter().any(|sub| sub.enabled)
        && StrokeFinish::for_tip(settings, op, true, true).is_none()
}

pub(crate) fn is_filter(op: BrushOp) -> bool {
    matches!(op, BrushOp::Blur | BrushOp::Sharpen | BrushOp::Median | BrushOp::Smudge)
}

fn dab_coverage(dab: &Dab, settings: &BrushSettings, width: u32, height: u32, mut visit: impl FnMut(usize, usize, f32)) {
    let radius = dab.size * 0.5;
    if radius <= 0.0 || dab.alpha <= 0.0 { return; }
    let extent = dab.extent(settings.tip_kind);
    let limits = |center: f32, bound: u32| {
        let low = (center - extent).floor().clamp(0.0, bound as f32) as usize;
        let high = (center + extent).ceil().clamp(0.0, bound as f32) as usize;
        low..high
    };
    for y in limits(dab.y, height) {
        for x in limits(dab.x, width) {
            let [dx, dy] = [x as f32 + 0.5 - dab.x, y as f32 + 0.5 - dab.y];
            let Some((_, falloff)) = dab.ink(settings, dx, dy) else { continue };
            let edge = if settings.wet_edges {
                0.55 + 0.45 * (dab.distance(settings.tip_kind, dx, dy) / radius).clamp(0.0, 1.0).powi(2)
            } else { 1.0 };
            visit(x, y, (dab.alpha * falloff * edge).clamp(0.0, 1.0));
        }
    }
}

fn accumulate_coverage(value: &mut f32, amount: f32, opacity: f32) {
    *value = (*value + (1.0 - *value) * amount).min(opacity);
}

fn adjustment_stroke(dst: &mut RasterViewMut, dabs: &[Dab], settings: &BrushSettings, params: &OpParams, op: BrushOp) {
    let opacity = settings.opacity.clamp(0.0, 1.0);
    if opacity == 0.0 || settings.flow <= 0.0 { return; }
    let width = dst.width as usize;
    let mut coverage = vec![0.0_f32; dst.rgba.len() / 4];
    for dab in dabs {
        dab_coverage(dab, settings, dst.width, dst.height, |x, y, amount| {
            accumulate_coverage(&mut coverage[y * width + x], amount, opacity);
        });
    }
    for (pixel, amount) in dst.rgba.chunks_exact_mut(4).zip(coverage) {
        if amount == 0.0 || pixel[3] == 0 { continue; }
        let rgb = if op == BrushOp::Sponge {
            sponge_colour(pixel, amount, params.sponge_mode)
        } else {
            adjust_tone(pixel, amount, params.tonal_range, params.protect_hue, op == BrushOp::Dodge)
        };
        pixel[..3].copy_from_slice(&rgb);
    }
}

fn sponge_colour(pixel: &[u8], amount: f32, mode: SpongeMode) -> [u8; 3] {
    let [hue, saturation, lightness] = crate::colour::rgb8_to_hsl(pixel);
    if saturation <= f32::EPSILON { return [pixel[0]; 3]; }
    let saturation = match mode {
        SpongeMode::Saturate => saturation * (1.0 + amount),
        SpongeMode::Desaturate => saturation * (1.0 - amount),
        SpongeMode::Vibrance => saturation + (100.0 - saturation) * amount * (1.0 - 0.75 * saturation / 100.0),
    };
    crate::colour::hsl_to_rgb(hue, saturation, lightness).map(channel8)
}

fn luminance([r, g, b]: [f32; 3]) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

pub(crate) fn tonal_weight(rgb: [f32; 3], range: TonalRange) -> f32 {
    let luma = luminance(rgb);
    match range {
        TonalRange::Shadows => (1.0 - luma).powi(2),
        TonalRange::Midtones => (1.0 - (2.0 * luma - 1.0).abs()).powi(2),
        TonalRange::Highlights => luma.powi(2),
    }
}

fn adjust_tone(pixel: &[u8], amount: f32, range: TonalRange, protect_hue: bool, lighten: bool) -> [u8; 3] {
    let rgb @ [r, g, b] = [0, 1, 2].map(|i| f32::from(pixel[i]) / 255.0);
    let luma = luminance(rgb);
    let amount = amount * tonal_weight(rgb, range);
    if !protect_hue {
        return rgb.map(|channel| {
            if lighten {
                channel8(channel + (1.0 - channel) * amount * (0.55 + 0.45 * channel))
            } else {
                channel8(channel - channel * amount * (1.0 - 0.45 * channel))
            }
        });
    }
    if !lighten { return rgb.map(|channel| channel8(channel * (1.0 - amount))); }
    let peak = r.max(g).max(b);
    let target = luma + (1.0 - luma) * amount;
    if peak == 0.0 { return [channel8(target); 3]; }
    let gain = (target / luma).min(1.0 / peak);
    let after = luma * gain;
    let lift = if after < 1.0 { ((target - after) / (1.0 - after)).clamp(0.0, 1.0) } else { 0.0 };
    rgb.map(|channel| channel8(channel * gain * (1.0 - lift) + lift))
}

fn channel8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(crate) fn mixer_stroke(
    dst: &mut RasterViewMut, points: &[StrokePoint], settings: &BrushSettings,
    strength: f32, colour: &mut Option<[u8; 4]>, pigment: bool, raster_origin: [u32; 2],
) {
    mix_dabs(dst, &dabs(points, settings), settings, strength, colour, pigment, raster_origin);
}

fn mix_dabs(
    dst: &mut RasterViewMut, dabs: &[Dab], settings: &BrushSettings,
    strength: f32, load: &mut Option<[u8; 4]>, pigment: bool, raster_origin: [u32; 2],
) {
    let strength = strength.clamp(0.0, 1.0);
    if strength == 0.0 || dabs.is_empty() { return; }
    let original = dst.rgba.to_vec();
    let source = RasterView::new(dst.width, dst.height, &original);
    let mut tip = settings.clone();
    for dab in dabs.iter().filter(|dab| dab.alpha > 0.0) {
        let sample = source.get_pixel(dab.x.floor() as i64, dab.y.floor() as i64);
        if let Some(colour) = *load {
            tip.colour = if pigment && sample[3] != 0 {
                mix_colour(colour, sample, (1.0 - strength) * 0.5, true)
            } else {
                colour
            };
            paint_dab(dst, &Dab { alpha: dab.alpha * strength, ..*dab }, &tip, BrushOp::Paint, raster_origin);
        }
        if sample[3] != 0 {
            let pickup = (1.0 - strength * 0.75) * (f32::from(sample[3]) / 255.0);
            *load = Some(load.map_or(sample, |colour| mix_colour(colour, sample, pickup, pigment)));
        }
    }
}

pub(crate) fn mix_colour(from: [u8; 4], to: [u8; 4], amount: f32, pigment: bool) -> [u8; 4] {
    let to_channels = |colour: [u8; 4]| {
        let [r, g, b, _] = colour.map(|channel| f32::from(channel) / 255.0);
        if pigment { rgb_to_ryb([r, g, b]) } else { [r, g, b] }
    };
    let from_channels = to_channels(from);
    let to_channels = to_channels(to);
    let blended = std::array::from_fn(|i| from_channels[i] + (to_channels[i] - from_channels[i]) * amount);
    let [r, g, b] = if pigment { ryb_to_rgb(blended) } else { blended };
    let alpha = (f32::from(from[3]) + (f32::from(to[3]) - f32::from(from[3])) * amount).round() as u8;
    let [r, g, b] = [r, g, b].map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
    [r, g, b, alpha]
}

fn rgb_to_ryb(rgb: [f32; 3]) -> [f32; 3] {
    let white = rgb.into_iter().fold(1.0, f32::min);
    let [r, g, b] = rgb.map(|channel| channel - white);
    let yellow = r.min(g);
    let [r, g] = [r - yellow, g - yellow];
    let factor = if g > 0.0 && b > 0.0 { 0.5 } else { 1.0 };
    normalize_chroma([r, yellow + g * factor, (b + g) * factor], rgb.into_iter().fold(0.0, f32::max) - white, white)
}

fn ryb_to_rgb(ryb: [f32; 3]) -> [f32; 3] {
    let white = ryb.into_iter().fold(1.0, f32::min);
    let [r, y, b] = ryb.map(|channel| channel - white);
    let green = y.min(b);
    let [y, b] = [y - green, b - green];
    let factor = if green > 0.0 && b > 0.0 { 2.0 } else { 1.0 };
    normalize_chroma([r + y, y + green * factor, b * factor], ryb.into_iter().fold(0.0, f32::max) - white, white)
}

fn normalize_chroma(channels: [f32; 3], chroma: f32, white: f32) -> [f32; 3] {
    let peak = channels.into_iter().fold(0.0, f32::max);
    if peak == 0.0 { [white; 3] } else { channels.map(|channel| white + channel * chroma / peak) }
}

fn sampled_stroke(dst: &mut RasterViewMut, dabs: &[Dab], settings: &BrushSettings, params: &OpParams, op: BrushOp) {
    let original = dst.rgba.to_vec();
    let source = RasterView::new(dst.width, dst.height, &original);
    let sample = |dab: &Dab| source.get_pixel(dab.x.floor() as i64, dab.y.floor() as i64);
    let first_sample = sample(&dabs[0]);
    if !params.sample_continuously && first_sample[3] == 0 { return; }

    let replace = op == BrushOp::ColourReplace;
    let mut strength = if replace { vec![0.0_f32; original.len() / 4] } else { Vec::new() };
    let alpha = f32::from(settings.colour[3]) / 255.0;
    let [replacement_hue, replacement_saturation, _] = crate::colour::rgb8_to_hsl(&settings.colour);
    let mut visited = Vec::new();
    let mut queue = Vec::new();
    for dab in dabs {
        let target = if params.sample_continuously { sample(dab) } else { first_sample };
        if target[3] == 0 || dab.size <= 0.0 || dab.alpha <= 0.0 { continue; }
        let extent = dab.extent(settings.tip_kind);
        let x0 = (dab.x - extent).floor().clamp(0.0, dst.width as f32) as u32;
        let y0 = (dab.y - extent).floor().clamp(0.0, dst.height as f32) as u32;
        let x1 = ((dab.x + extent).ceil() + 1.0).clamp(0.0, dst.width as f32) as u32;
        let y1 = ((dab.y + extent).ceil() + 1.0).clamp(0.0, dst.height as f32) as u32;
        let (width, height) = (x1 - x0, y1 - y0);
        if width == 0 || height == 0 { continue; }
        let eligible = |x: u32, y: u32| {
            let (_, coverage) = dab.ink(settings, x as f32 + 0.5 - dab.x, y as f32 + 0.5 - dab.y)?;
            let pixel = source.get_pixel(i64::from(x), i64::from(y));
            let channels = if replace { 3 } else { 4 };
            (pixel[3] > 0 && colour_matches(&pixel[..channels], &target[..channels], params.tolerance))
                .then_some((coverage, pixel))
        };
        let mut apply_pixel = |x: u32, y: u32, falloff: f32, source_pixel: [u8; 4]| {
            let pixel = y as usize * dst.width as usize + x as usize;
            let dab_strength = dab.alpha * falloff;
            let opacity = settings.opacity.clamp(0.0, 1.0);
            if replace {
                let previous = strength[pixel];
                let amount = (previous + (1.0 - previous) * dab_strength * alpha).min(opacity * alpha);
                if amount <= previous { return; }
                strength[pixel] = amount;
                dst.rgba[pixel * 4..pixel * 4 + 3].copy_from_slice(&crate::colour::mix_chroma(
                    &source_pixel, replacement_hue, replacement_saturation, amount,
                ));
            } else {
                let index = pixel * 4 + 3;
                let previous = f32::from(dst.rgba[index]);
                let floor = f32::from(source_pixel[3]) * (1.0 - opacity);
                dst.rgba[index] = previous.min(floor.max(previous * (1.0 - dab_strength))).round() as u8;
            }
        };
        if !params.contiguous {
            for y in y0..y1 {
                for x in x0..x1 {
                    if let Some((coverage, original)) = eligible(x, y) { apply_pixel(x, y, coverage, original); }
                }
            }
            continue;
        }

        let [seed_x, seed_y] = [dab.x, dab.y].map(|value| value.floor() as i64);
        if !(i64::from(x0)..i64::from(x1)).contains(&seed_x)
            || !(i64::from(y0)..i64::from(y1)).contains(&seed_y) { continue; }
        visited.clear();
        visited.resize(width as usize * height as usize, false);
        queue.clear();
        queue.push([seed_x as u32, seed_y as u32]);
        while let Some([x, y]) = queue.pop() {
            let index = (y - y0) as usize * width as usize + (x - x0) as usize;
            if std::mem::replace(&mut visited[index], true) { continue; }
            let Some((coverage, original)) = eligible(x, y) else { continue; };
            apply_pixel(x, y, coverage, original);
            if x > x0 { queue.push([x - 1, y]); }
            if x + 1 < x1 { queue.push([x + 1, y]); }
            if y > y0 { queue.push([x, y - 1]); }
            if y + 1 < y1 { queue.push([x, y + 1]); }
        }
    }
}

pub(crate) fn clone_dabs(
    dst: &mut RasterViewMut,
    dabs: &[Dab],
    settings: &BrushSettings,
    origin: [u32; 2],
    mode: crate::blend::BlendMode,
    sample: impl Fn([u32; 2]) -> [u8; 4],
) {
    for dab in dabs {
        let [left, top, right, bottom] = dab_bounds(dab, settings.tip_kind, [dst.width, dst.height]);
        for y in top..bottom {
            for x in left..right {
                if let Some((_, coverage)) = dab.ink(settings, x as f32 - dab.x, y as f32 - dab.y) {
                    let alpha = dab.alpha * coverage;
                    if alpha > 0.0 {
                        let mut colour = sample([origin[0] + x, origin[1] + y]);
                        if mode.is_normal() {
                            paint_pixel(dst, x, y, colour, alpha, false, settings.protect_alpha);
                        } else {
                            let mut current = dst.get_pixel(i64::from(x), i64::from(y));
                            if settings.protect_alpha {
                                let mut opaque = [current[0], current[1], current[2], 255];
                                crate::blend::blend_pixel(&mut opaque, 0, [colour[0], colour[1], colour[2], 255], 1.0, mode);
                                colour[..3].copy_from_slice(&opaque[..3]);
                                paint_pixel(dst, x, y, colour, alpha, false, true);
                            } else {
                                crate::blend::blend_pixel(&mut current, 0, colour, alpha, mode);
                                dst.set_pixel(i64::from(x), i64::from(y), current[0], current[1], current[2], current[3]);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn restore_dabs(
    dst: &mut RasterViewMut,
    source: Option<&crate::pixel::PixelLayer>,
    size: [u32; 2],
    dabs: impl IntoIterator<Item = Vec<Dab>>,
    settings: &BrushSettings,
    origin: [u32; 2],
    preserve_missing: bool,
) {
    let sample = |x, y| source.map_or([0; 4], |layer| {
        let scale = |pixel, from, to| ((u64::from(pixel) * u64::from(to)) / u64::from(from)) as u32;
        layer.get(scale(x, size[0], layer.width), scale(y, size[1], layer.height))
    });
    let opacity = settings.opacity.clamp(0.0, 1.0);
    if opacity == 0.0 || settings.flow <= 0.0 { return; }
    let width = dst.width as usize;
    let mut coverage = vec![0.0; dst.rgba.len() / 4];
    for dab in dabs.into_iter().flatten() {
        dab_coverage(&dab, settings, dst.width, dst.height, |x, y, amount| {
            accumulate_coverage(&mut coverage[y * width + x], amount, opacity);
        });
    }
    for (index, (pixel, coverage)) in dst.rgba.chunks_exact_mut(4).zip(coverage).enumerate() {
        if coverage == 0.0 { continue; }
        let prior = sample(origin[0] + (index % width) as u32, origin[1] + (index / width) as u32);
        if (prior[3] == 0 && (preserve_missing || settings.protect_alpha))
            || (settings.protect_alpha && pixel[3] == 0)
        {
            continue;
        }
        let before = f32::from(pixel[3]) * (1.0 - coverage);
        let after = f32::from(prior[3]) * coverage;
        let alpha = before + after;
        if alpha > 0.0 {
            for (current, previous) in pixel[..3].iter_mut().zip(prior) {
                *current = ((f32::from(*current) * before + f32::from(previous) * after) / alpha).round() as u8;
            }
        } else if pixel[3] != 0 {
            pixel[..3].copy_from_slice(&prior[..3]);
        }
        if !settings.protect_alpha { pixel[3] = alpha.round() as u8; }
    }
}

pub(crate) fn paint_dab(
    dst: &mut RasterViewMut,
    dab: &Dab,
    s: &BrushSettings,
    op: BrushOp,
    raster_origin: [u32; 2],
) {
    if op == BrushOp::Pixel {
        if dab.x >= 0.0 && dab.y >= 0.0 {
            paint_pixel(dst, dab.x as u32, dab.y as u32, s.colour, dab.alpha, false, s.protect_alpha);
        }
        return;
    }
    let [x_min, y_min, x_max, y_max] = dab_bounds(dab, s.tip_kind, [dst.width, dst.height]);
    let texture = if matches!(op, BrushOp::Paint | BrushOp::Erase) {
        texture_for(s, BrushTextureMode::Nozzle)
    } else { None };

    let constant_ink = op == BrushOp::Paint && !s.protect_alpha
        && !s.pixel_perfect && s.stamp.is_none() && s.additional_stamps.is_empty()
        && s.sub_brush_color_blend == SubBrushColorBlend::Normal;
    let solid_round = constant_ink && s.tip_kind == TipKind::Round && dab.roundness == 1.0
        && dab.height_scale == 1.0 && dab.alpha == 1.0 && texture.is_none() && s.colour[3] == 255;
    let radius = dab.size * 0.5;
    let solid_radius = round_core(radius, dab.hardness) - 0.001;
    for py in y_min..y_max {
        let dy = py as f32 - dab.y;
        let (left, right) = if solid_round && solid_radius > dy.abs() && x_min < x_max {
            let half_width = (solid_radius * solid_radius - dy * dy).sqrt();
            let left = ((dab.x - half_width).ceil() as u32).clamp(x_min, x_max);
            let right = ((dab.x + half_width).floor() as u32).saturating_add(1).clamp(left, x_max);
            let row = py as usize * dst.width as usize;
            for pixel in dst.rgba[(row + left as usize) * 4..(row + right as usize) * 4].as_chunks_mut::<4>().0 {
                *pixel = s.colour;
            }
            (left, right)
        } else {
            (x_max, x_max)
        };
        for px in (x_min..left).chain(right..x_max) {
            if constant_ink {
                let offset = (py as usize * dst.width as usize + px as usize) * 4;
                if dst.rgba[offset + 3] == 255 && dst.rgba[offset..offset + 3] == s.colour[..3] { continue; }
            }
            let dx = px as f32 - dab.x;
            let dy = py as f32 - dab.y;
            let Some((colour, falloff)) = dab.ink(s, dx, dy) else { continue };

            let grain = texture.map_or(1.0, |texture| texture.coverage_at(px, py, raster_origin));
            let dab_alpha = dab.alpha * falloff * grain;
            if dab_alpha <= 0.0 {
                continue;
            }

            match op {
                BrushOp::Paint if s.sub_brush_color_blend == SubBrushColorBlend::Average => {
                    paint_average(dst, px, py, colour, dab_alpha);
                }
                BrushOp::Paint => {
                    paint_pixel(dst, px, py, colour, dab_alpha, s.pixel_perfect, s.protect_alpha);
                }
                BrushOp::Erase => {
                    erase_pixel(dst, px, py, dab_alpha);
                }
                _ => {
                    paint_pixel(dst, px, py, colour, dab_alpha, s.pixel_perfect, s.protect_alpha);
                }
            }
        }
    }
}

pub(crate) fn dab_bounds(dab: &Dab, kind: TipKind, [width, height]: [u32; 2]) -> [u32; 4] {
    let extent = dab.extent(kind);
    [
        (dab.x - extent).floor().max(0.0) as u32,
        (dab.y - extent).floor().max(0.0) as u32,
        ((dab.x + extent).ceil() as u32).saturating_add(1).min(width),
        ((dab.y + extent).ceil() as u32).saturating_add(1).min(height),
    ]
}

fn texture_for(settings: &BrushSettings, mode: BrushTextureMode) -> Option<&BrushTexture> {
    settings.texture.as_ref().filter(|texture| texture.enabled && texture.mode == mode)
}

fn paint_average(dst: &mut RasterViewMut, x: u32, y: u32, color: [u8; 4], coverage: f32) {
    let [x, y] = [i64::from(x), i64::from(y)];
    let backdrop = dst.get_pixel(x, y);
    let a = coverage * f32::from(color[3]) / 255.0;
    let b = f32::from(backdrop[3]) / 255.0;
    let total = a + b * (1.0 - a);
    if total <= 0.0 { return; }
    let overlap = a * b * 0.5;
    let [r, g, blue] = [0, 1, 2].map(|i| {
        ((f32::from(color[i]) * (a - overlap)
            + f32::from(backdrop[i]) * (total - a + overlap)) / total).round() as u8
    });
    dst.set_pixel(x, y, r, g, blue, (total * 255.0).round() as u8);
}

fn paint_pixel(
    dst: &mut RasterViewMut,
    x: u32,
    y: u32,
    colour: [u8; 4],
    alpha: f32,
    pixel_perfect: bool,
    protect_alpha: bool,
) {
    if pixel_perfect && !protect_alpha {
        let ix = x as i64;
        let iy = y as i64;
        if dst.set_pixel(ix, iy, colour[0], colour[1], colour[2], colour[3]) {
            return;
        }
    }

    let src_a = alpha * (colour[3] as f32 / 255.0);
    if src_a <= 0.0 { return; }
    let existing = dst.get_pixel(x as i64, y as i64);
    if !protect_alpha {
        if src_a == 1.0 {
            dst.set_pixel(x as i64, y as i64, colour[0], colour[1], colour[2], 255);
            return;
        }
        if existing[3] == 255 && existing[..3] == colour[..3] { return; }
    }
    let dst_a = existing[3] as f32 / 255.0;

    if protect_alpha {
        if existing[3] != 0 {
            let [r, g, b] = [0, 1, 2].map(|axis| {
                (f32::from(existing[axis]) * (1.0 - src_a) + f32::from(colour[axis]) * src_a).round() as u8
            });
            dst.set_pixel(x as i64, y as i64, r, g, b, existing[3]);
        }
        return;
    }
    let out_a = src_a + dst_a * (1.0 - src_a);

    if out_a <= 0.0 {
        return;
    }

    let r = ((colour[0] as f32 * src_a) + (existing[0] as f32 * dst_a * (1.0 - src_a))) / out_a;
    let g = ((colour[1] as f32 * src_a) + (existing[1] as f32 * dst_a * (1.0 - src_a))) / out_a;
    let b = ((colour[2] as f32 * src_a) + (existing[2] as f32 * dst_a * (1.0 - src_a))) / out_a;

    let r_u8 = r.clamp(0.0, 255.0).round() as u8;
    let g_u8 = g.clamp(0.0, 255.0).round() as u8;
    let b_u8 = b.clamp(0.0, 255.0).round() as u8;
    let a_u8 = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;

    dst.set_pixel(x as i64, y as i64, r_u8, g_u8, b_u8, a_u8);
}

fn erase_pixel(dst: &mut RasterViewMut, x: u32, y: u32, alpha: f32) {
    let existing = dst.get_pixel(x as i64, y as i64);
    let new_a = (existing[3] as f32 * (1.0 - alpha)).clamp(0.0, 255.0).round() as u8;
    dst.set_pixel(x as i64, y as i64, existing[0], existing[1], existing[2], new_a);
}

#[derive(Debug, Clone)]
pub struct TileSnapshot {
    pub rect: [u32; 4],
    pub before: Vec<u8>,
}

impl TileSnapshot {
    pub fn capture(src: &RasterView, rect: [u32; 4]) -> Self {
        let x = rect[0];
        let y = rect[1];
        let w = rect[2];
        let h = rect[3];

        let mut before = Vec::with_capacity((w * h * 4) as usize);

        for py in y..(y + h) {
            for px in x..(x + w) {
                let pixel = src.get_pixel(px as i64, py as i64);
                before.push(pixel[0]);
                before.push(pixel[1]);
                before.push(pixel[2]);
                before.push(pixel[3]);
            }
        }

        Self { rect, before }
    }

    pub fn restore(&self, dst: &mut RasterViewMut) {
        let x = self.rect[0];
        let y = self.rect[1];
        let w = self.rect[2];
        let h = self.rect[3];

        let mut idx = 0;
        for py in y..(y + h) {
            for px in x..(x + w) {
                if idx + 3 < self.before.len() {
                    dst.set_pixel(
                        px as i64,
                        py as i64,
                        self.before[idx],
                        self.before[idx + 1],
                        self.before[idx + 2],
                        self.before[idx + 3],
                    );
                    idx += 4;
                }
            }
        }
    }
}

pub fn flood_fill(
    dst: &mut RasterViewMut,
    seed: [u32; 2],
    colour: [u8; 4],
    tolerance: f32,
    contiguous: bool,
    sample: Option<&RasterView>,
) -> Option<[u32; 4]> {
    let mask = crate::selection::flood_mask(
        sample.unwrap_or(&dst.as_view()), seed, tolerance, contiguous, false,
    );
    if mask.width != dst.width || mask.height != dst.height { return None; }
    for (pixel, &coverage) in dst.rgba.chunks_exact_mut(4).zip(&mask.coverage) {
        if coverage != 0 { pixel.copy_from_slice(&colour); }
    }
    mask.bounds()
}

pub fn flood_erase(
    dst: &mut RasterViewMut,
    seed: [u32; 2],
    tolerance: f32,
    contiguous: bool,
    sample: Option<&RasterView>,
) -> Option<[u32; 4]> {
    flood_fill(dst, seed, [0; 4], tolerance, contiguous, sample)
}
fn colour_matches(a: &[u8], b: &[u8], tolerance: f32) -> bool {
    let limit = tolerance * 255.0;
    a.iter().zip(b).all(|(&a, &b)| f32::from(a.abs_diff(b)) <= limit)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushLibrary {
    pub tips: Vec<BrushTip>,
    pub categories: Vec<String>,
    #[serde(default)]
    pub(crate) next_id: u64,
}

impl BrushLibrary {
    pub fn builtin() -> Self {
        let tips = vec![
            BrushTip {
                id: "round_hard".to_string(),
                name: "Hard Round".to_string(),
                category: "Basic".to_string(),
                kind: TipKind::Round,
                spacing: 0.1,
                size_px: 64.0,
                opacity: 1.0,
                accumulation: None,
                flow: 1.0,
                hardness: 1.0,
                roundness: 1.0,
                image: None,
                additional_images: Vec::new(),
                wet_edges: BrushWetEdges::default(),
                texture: None,
                sub_brushes: Vec::new(),
                rotation: 0.0,
                dynamics: BrushDynamics::default(),
            },
            BrushTip {
                id: "round_soft".to_string(),
                name: "Soft Round".to_string(),
                category: "Basic".to_string(),
                kind: TipKind::Round,
                spacing: 0.1,
                size_px: 64.0,
                opacity: 1.0,
                accumulation: None,
                flow: 1.0,
                hardness: 0.0,
                roundness: 1.0,
                image: None,
                additional_images: Vec::new(),
                wet_edges: BrushWetEdges::default(),
                texture: None,
                sub_brushes: Vec::new(),
                rotation: 0.0,
                dynamics: BrushDynamics::default(),
            },
            BrushTip {
                id: "square_hard".to_string(),
                name: "Hard Square".to_string(),
                category: "Basic".to_string(),
                kind: TipKind::Square,
                spacing: 0.1,
                size_px: 64.0,
                opacity: 1.0,
                accumulation: None,
                flow: 1.0,
                hardness: 1.0,
                roundness: 1.0,
                image: None,
                additional_images: Vec::new(),
                wet_edges: BrushWetEdges::default(),
                texture: None,
                sub_brushes: Vec::new(),
                rotation: 0.0,
                dynamics: BrushDynamics::default(),
            },
        ];

        let categories = vec!["Basic".to_string()];

        Self { tips, categories, next_id: 1 }
    }

    pub fn get(&self, id: &str) -> Option<&BrushTip> {
        self.tips.iter().find(|t| t.id == id)
    }

 }
