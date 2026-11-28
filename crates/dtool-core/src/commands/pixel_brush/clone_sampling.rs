use crate::{CanvasFlags, CommandError, Document, ObjectId, Session};
use crate::context_toolbar::{CLONE_FLIP, CLONE_ROTATION, CLONE_SCALE, CLONE_SCOPE, CLONE_TOOL, HEAL_TOOL, uses_brush_source};
use crate::geometry::apply_matrix;
use crate::pixel::PixelLayer;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloneTransform([f32; 6]);

impl CloneTransform {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn current(session: &Session) -> Self {
        let tool = &session.active_tool;
        if !uses_brush_source(tool) { return Self::IDENTITY; }
        let options = &session.tool_options;
        let rotation = options.number(tool, CLONE_ROTATION).unwrap_or(0.0) as f32;
        let scale = options.number(tool, CLONE_SCALE).unwrap_or(100.0) as f32 / 100.0;
        let flip = options.choice(tool, CLONE_FLIP);
        let sx = if matches!(flip.as_deref(), Some("horizontal" | "both")) { -scale } else { scale };
        let sy = if matches!(flip.as_deref(), Some("vertical" | "both")) { -scale } else { scale };
        let (sin, cos) = rotation.to_radians().sin_cos();
        Self([cos / sx, -sin / sy, sin / sx, cos / sy, 0.0, 0.0])
    }

    pub fn sample_delta(self, destination_delta: [f32; 2]) -> [f32; 2] {
        apply_matrix(&self.0, destination_delta)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope { Current, CurrentAndBelow, Below, All, Global }

impl Scope {
    pub fn current(session: &Session) -> Self {
        let tool = if uses_brush_source(&session.active_tool) { session.active_tool.as_str() } else { CLONE_TOOL };
        match session.tool_options.choice(tool, CLONE_SCOPE).as_deref() {
            Some("current") => Self::Current,
            Some("below") => Self::Below,
            Some("all") => Self::All,
            Some("global") => Self::Global,
            Some("current_and_below") => Self::CurrentAndBelow,
            _ if tool == HEAL_TOOL => Self::Current,
            _ => Self::CurrentAndBelow,
        }
    }
}

pub(super) struct Options {
    pub transform: CloneTransform,
    pub scope: Scope,
    pub global: Option<super::global_source::Source>,
    pub blend: crate::blend::BlendMode,
}

impl Options {
    pub const CURRENT_LAYER: Self = Self {
        transform: CloneTransform::IDENTITY, scope: Scope::Current, global: None, blend: crate::blend::BlendMode::Normal,
    };

    pub fn current(session: &Session) -> Self {
        if !uses_brush_source(&session.active_tool) { return Self::CURRENT_LAYER; }
        use crate::blend::BlendMode;
        let scope = Scope::current(session);
        let global = (scope == Scope::Global).then(|| session.clone_sources.selected()).flatten();
        let blend = match session.tool_options.choice(&session.active_tool, "clone.blend_mode").as_deref() {
            Some("multiply") => BlendMode::Multiply,
            Some("screen") => BlendMode::Screen,
            Some("overlay") => BlendMode::Overlay,
            Some("darken") => BlendMode::Darken,
            Some("lighten") => BlendMode::Lighten,
            _ => BlendMode::Normal,
        };
        Self { transform: CloneTransform::current(session), scope, global, blend }
    }
}

pub(super) struct Sampler {
    linear: Option<[[f32; 2]; 2]>,
    anchor: [f32; 2],
    offset: [i64; 2],
}

impl Sampler {
    pub(super) fn new(transform: CloneTransform, mut world: [f32; 6], mut inverse: [f32; 6], scale: [f32; 2],
        anchor: [f32; 2], offset: [i64; 2],
    ) -> Self {
        world[4..].fill(0.0);
        inverse[4..].fill(0.0);
        let convert = |raster_step| {
            let delta = apply_matrix(&world, raster_step);
            let local = apply_matrix(&inverse, transform.sample_delta(delta));
            [local[0] * scale[0], local[1] * scale[1]]
        };
        let linear = (transform != CloneTransform::IDENTITY)
            .then(|| [convert([1.0 / scale[0], 0.0]), convert([0.0, 1.0 / scale[1]])]);
        Self { linear, anchor, offset }
    }

    pub(super) fn sample(&self, source: &PixelLayer, [x, y]: [u32; 2]) -> [u8; 4] {
        if self.linear.is_none() {
            return read(source, i64::from(x).saturating_add(self.offset[0]), i64::from(y).saturating_add(self.offset[1]));
        }
        interpolate([source.width, source.height], self.position([x as f32, y as f32]), |x, y| read(source, x, y))
    }

    pub(super) fn position(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        if let Some([column_x, column_y]) = self.linear {
            let [dx, dy] = [x - self.anchor[0], y - self.anchor[1]];
            std::array::from_fn(|axis| {
                self.anchor[axis] + self.offset[axis] as f32 + column_x[axis] * dx + column_y[axis] * dy
            })
        } else {
            [x + self.offset[0] as f32, y + self.offset[1] as f32]
        }
    }

    pub(super) fn composite_region(
        &self, doc: &Document, layer: ObjectId, scope: Scope, [x, y, w, h]: [u32; 4],
        to_world: impl Fn([f32; 2]) -> [f32; 2],
    ) -> Result<CompositeRegion, CommandError> {
        use crate::layer_style::raster;
        let flags = CanvasFlags::default();
        CompositeRegion::render(doc, [x, y, w, h], |pos| to_world(self.position(pos)), |bounds, size| match scope {
            Scope::Current | Scope::Global => None,
            Scope::CurrentAndBelow | Scope::Below => raster::render_relative_object_region(
                doc, &flags, layer, bounds, size, scope == Scope::CurrentAndBelow,
            ),
            Scope::All => Some(raster::render_region(doc, &flags, &[], bounds, size[0], size[1])),
        })
    }
}

pub(super) struct CompositeRegion {
    pixels: Option<Vec<u8>>,
    bounds: [f32; 4],
    size: [u32; 2],
}

impl CompositeRegion {
    pub(super) fn render(
        doc: &Document, [x, y, w, h]: [u32; 4],
        to_source: impl Fn([f32; 2]) -> [f32; 2],
        render: impl FnOnce([f32; 4], [u32; 2]) -> Option<Vec<u8>>,
    ) -> Result<Self, CommandError> {
        let corners = [[x, y], [x + w, y], [x, y + h], [x + w, y + h]]
            .map(|position| to_source(position.map(|axis| axis as f32)));
        let ppm = crate::units::to_px(1.0, doc.units, doc.dpi).ok_or_else(CommandError::invalid_params)?;
        let px = |value: f32| f64::from(value) * f64::from(ppm);
        let min = |axis: usize| px(corners.iter().map(|point| point[axis]).fold(f32::INFINITY, f32::min)).floor() - 1.0;
        let max = |axis: usize| px(corners.iter().map(|point| point[axis]).fold(f32::NEG_INFINITY, f32::max)).ceil() + 1.0;
        let [left, top] = [min(0), min(1)];
        let [width, height] = [max(0) - left, max(1) - top];
        if ![left, top, width, height].iter().all(|value| value.is_finite()) || width <= 0.0 || height <= 0.0 {
            return Err(CommandError::invalid_params());
        }
        let factor = (4096.0 / width.max(height)).min((4_194_304.0 / (width * height)).sqrt()).min(1.0);
        let size = [width, height].map(|side| (side * factor).floor().max(1.0) as u32);
        let bounds = [left, top, width, height].map(|value| (value / f64::from(ppm)) as f32);
        if bounds.iter().any(|value| !value.is_finite()) { return Err(CommandError::invalid_params()); }
        let pixels = render(bounds, size);
        if pixels.as_ref().is_some_and(|data| data.len() != size[0] as usize * size[1] as usize * 4) {
            return Err(CommandError::invalid_params());
        }
        Ok(CompositeRegion { pixels, bounds, size })
    }
    pub(super) fn sample(&self, point: [f32; 2]) -> [u8; 4] {
        let Some(pixels) = self.pixels.as_ref() else { return [0; 4]; };
        let [width, height] = self.size.map(i64::from);
        let position = std::array::from_fn(|axis| {
            (point[axis] - self.bounds[axis]) / self.bounds[axis + 2] * self.size[axis] as f32 - 0.5
        });
        interpolate(self.size, position, |x, y| {
            if x < 0 || y < 0 || x >= width || y >= height { return [0; 4]; }
            let index = ((y * width + x) * 4) as usize;
            pixels[index..index + 4].try_into().unwrap_or([0; 4])
        })
    }
}

fn read(source: &PixelLayer, x: i64, y: i64) -> [u8; 4] {
    if x < 0 || y < 0 || x >= i64::from(source.width) || y >= i64::from(source.height) {
        return [0; 4];
    }
    source.get(x as u32, y as u32)
}

fn interpolate(size: [u32; 2], [x, y]: [f32; 2], read: impl Fn(i64, i64) -> [u8; 4]) -> [u8; 4] {
    if !(x >= -1.0 && y >= -1.0 && x < size[0] as f32 && y < size[1] as f32) {
        return [0; 4];
    }
    let [left, top] = [x.floor() as i64, y.floor() as i64];
    let [fx, fy] = [x - left as f32, y - top as f32];
    let mut alpha = 0.0;
    let mut premultiplied = [0.0; 3];
    for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
        for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
            let rgba = read(left + dx, top + dy);
            let weight = f32::from(rgba[3]) * wx * wy;
            alpha += weight;
            for (out, channel) in premultiplied.iter_mut().zip(rgba) {
                *out += f32::from(channel) * weight;
            }
        }
    }
    if alpha.round() == 0.0 { return [0; 4]; }
    let [r, g, b] = premultiplied.map(|value| (value / alpha).round() as u8);
    [r, g, b, alpha.round() as u8]
}
