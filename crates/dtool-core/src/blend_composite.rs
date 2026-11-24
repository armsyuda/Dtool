//! B29-T007 object blend modes + B29-T008 blend ranges (blend-if) for the scene rasterizer.
//!
//! `rasterize` splits the scene into runs: consecutive prims of Normal objects are rasterised together,
//! each prim run of an object with a non-Normal blend mode or blend ranges is rasterised alone (with the
//! clip stack that was open at that point) and composited onto what is underneath with its mode, its
//! opacity scaled by the blend-range curves (source luminance x underlying luminance).
//! Returns None when nothing in the scene blends, so the caller keeps its plain path.
//! Hook (scene.rs, rasterize_scene, first line):
//! `if let Some(px) = crate::blend_composite::rasterize(doc, scene, bounds, width, height, view, rasterize_scene) { return px; }`
use crate::blend::{blend_pixel, BlendMode};
use crate::layer_style::BlendRanges;
use crate::object::ObjectId;
use crate::scene::{Prim, Scene};
use crate::view_state::ViewState;
use crate::Document;
use std::cell::Cell;

pub(crate) mod groups;

pub type Raster = fn(&Document, &Scene, [f32; 4], u32, u32, Option<&ViewState>) -> Vec<u8>;

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

struct Guard(bool);
impl Guard {
    fn enter() -> Self {
        Self(INSIDE.with(|active| active.replace(true)))
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        INSIDE.with(|c| c.set(self.0));
    }
}

fn drawn_object(prim: &Prim) -> Option<ObjectId> {
    match prim {
        Prim::Fill { object, .. } | Prim::Appearance { object, .. } | Prim::Stroke { object, .. } | Prim::Image { object, .. } | Prim::Text { object, .. } | Prim::Label { object, .. } => Some(*object),
        _ => None,
    }
}

/// The object's blend mode and (valid) blend ranges when it does not composite as plain Normal.
pub fn blend_of(doc: &Document, id: ObjectId) -> Option<(BlendMode, Option<&BlendRanges>)> {
    let object = doc.get_object(id)?;
    let ranges = object.blend_ranges.as_ref().filter(|r| r.is_valid());
    (!object.blend_mode.clone().is_normal() || ranges.is_some()).then(|| (object.blend_mode.clone(), ranges))
}

fn needs_object_run(doc: &Document, id: ObjectId, include_transparency: bool, tone_groups: bool) -> bool {
    doc.get_object(id).is_some_and(|object| (include_transparency
        && !matches!(object.transparency, crate::paint::Transparency::None))
        || crate::raster_source::compositing_mask(doc, object).is_some())
        || blend_of(doc, id).is_some()
        || tone_groups && crate::tone_blend::for_layer(doc, id).is_some()
}

pub fn scene_uses_blending(doc: &Document, scene: &Scene) -> bool {
    scene_needs_composite(doc, scene, true)
}

/// A canvas that already paints transparent objects separately can retain its
/// direct rendering path until a mask, group or blend requires compositing.
pub fn scene_needs_canvas_composite(doc: &Document, scene: &Scene) -> bool {
    scene_needs_composite(doc, scene, false)
}

fn scene_needs_composite(doc: &Document, scene: &Scene, include_transparency: bool) -> bool {
    scene.prims.iter().any(|prim| matches!(prim, Prim::Appearance { .. })
        || drawn_object(prim).is_some_and(|id| needs_object_run(doc, id, include_transparency, false)))
        || groups::scene_has_groups(doc, scene)
        || scene.prims.iter().filter_map(drawn_object)
            .any(|id| crate::layer_style::raster::stroke_effect_group(doc, id).is_some())
}

pub fn curve(points: &[[f32; 2]], x: f32) -> f32 {
    let Some(first) = points.first() else { return 1.0 };
    let last = points[points.len() - 1];
    if x <= first[0] {
        return first[1];
    }
    if x >= last[0] {
        return last[1];
    }
    for pair in points.windows(2) {
        let ([x0, y0], [x1, y1]) = (pair[0], pair[1]);
        if x >= x0 && x <= x1 {
            return if x1 - x0 <= f32::EPSILON { y1 } else { y0 + (y1 - y0) * (x - x0) / (x1 - x0) };
        }
    }
    last[1]
}

fn luminance(px: &[u8]) -> f32 {
    (0.2126 * f32::from(px[0]) + 0.7152 * f32::from(px[1]) + 0.0722 * f32::from(px[2])) / 255.0
}

/// Blend-range opacity factor for one pixel (source over underlying).
pub fn range_factor(ranges: &BlendRanges, source: &[u8], underlying: &[u8]) -> f32 {
    let g = if ranges.gamma.is_finite() && ranges.gamma > 0.0 { ranges.gamma } else { 1.0 };
    let s = curve(&ranges.source, luminance(source).powf(g));
    let u = if underlying[3] == 0 { 1.0 } else { curve(&ranges.underlying, luminance(underlying).powf(g)) };
    (s * u).clamp(0.0, 1.0)
}

pub(crate) fn composite(pixels: &mut [u8], layer: &[u8], blend: Option<(BlendMode, Option<&BlendRanges>)>, opacity: f32) {
    let blend = blend.unwrap_or((BlendMode::Normal, None));
    for (offset, src) in layer.chunks_exact(4).enumerate().map(|(i, c)| (i * 4, c)) {
        composite_sample(pixels, offset, src, &blend, opacity);
    }
}

pub(crate) fn composite_sample(pixels: &mut [u8], offset: usize, src: &[u8], (mode, ranges): &(BlendMode, Option<&BlendRanges>), opacity: f32) {
    if src[3] == 0 {
        return;
    }
    let factor = opacity * ranges.map_or(1.0, |r| range_factor(r, src, &pixels[offset..offset + 4]));
    if factor > 0.0 {
        blend_pixel(pixels, offset, [src[0], src[1], src[2], src[3]], factor, mode.clone());
    }
}

pub fn rasterize(doc: &Document, scene: &Scene, bounds: [f32; 4], width: u32, height: u32, view: Option<&ViewState>, raster: Raster) -> Option<Vec<u8>> {
    if INSIDE.with(|c| c.get()) || !scene_uses_blending(doc, scene) {
        return None;
    }
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let size = [width, height];
    let grid = groups::RasterGrid { bounds, size, view };
    if !groups::render(doc, &scene.prims, crate::layer_style::raster::prim_object, &[], grid, &mut pixels, |mut prims, opacity, target| {
        for prim in &mut prims {
            groups::normalize_prim_opacity(prim, opacity);
        }
        let sub = Scene { version: scene.version, prims, quads: scene.quads.clone() };
        rasterize_local_over(doc, &sub, bounds, size, view, raster, target);
    }) {
        composite_scene(doc, scene, bounds, size, view, raster, &mut pixels);
    }
    Some(pixels)
}

pub(crate) fn rasterize_over(doc: &Document, scene: &Scene, bounds: [f32; 4], size: [u32; 2], pixels: &mut [u8]) {
    rasterize_local_over(doc, scene, bounds, size, None, crate::scene::rasterize_scene, pixels);
}

pub(crate) fn rasterize_effect_source(doc: &Document, scene: &Scene, bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    let _guard = Guard::enter();
    crate::scene::rasterize_scene(doc, scene, bounds, width, height, None)
}

fn rasterize_local_over(doc: &Document, scene: &Scene, bounds: [f32; 4], size: [u32; 2], view: Option<&ViewState>, raster: Raster, pixels: &mut [u8]) {
    let tone_groups = doc.objects.iter().any(|object| object.tone_blend.is_some());
    if scene.prims.iter().filter_map(drawn_object).any(|id| needs_object_run(doc, id, true, tone_groups)) {
        composite_scene(doc, scene, bounds, size, view, raster, pixels);
    } else {
        let _guard = Guard::enter();
        let layer = raster(doc, scene, bounds, size[0], size[1], view);
        crate::layer_style::raster::over(pixels, &layer);
    }
}

fn composite_scene(doc: &Document, scene: &Scene, bounds: [f32; 4], [width, height]: [u32; 2], view: Option<&ViewState>, raster: Raster, pixels: &mut [u8]) {
    let _guard = Guard::enter();
    let tone_groups = doc.objects.iter().any(|object| object.tone_blend.is_some());
    let mut open: Vec<Prim> = Vec::new();
    let mut prefix: Vec<Prim> = Vec::new();
    let mut run: Vec<Prim> = Vec::new();
    let mut key: Option<ObjectId> = None;
    let flush = |pixels: &mut [u8], prefix: &[Prim], run: &mut Vec<Prim>, key: Option<ObjectId>| {
        if run.iter().any(|p| drawn_object(p).is_some()) {
            let blend = key.and_then(|id| blend_of(doc, id));
            if view.is_none() && std::ptr::fn_addr_eq(raster, crate::scene::rasterize_scene as Raster)
                && let (Some(id), Some(blend)) = (key, blend.clone())
                && doc.get_object(id).is_some_and(|object| {
                    matches!(object.transparency, crate::paint::Transparency::None)
                        && crate::raster_source::compositing_mask(doc, object).is_none()
                })
                && (!tone_groups || crate::tone_blend::for_layer(doc, id).is_none())
                && crate::scene::blend_single_fill(doc, prefix.iter().chain(run.iter()), bounds, [width, height], pixels, blend)
            {
                run.clear();
                return;
            }
            let prims = prefix.iter().cloned().chain(run.drain(..)).collect();
            let sub = Scene { version: scene.version, prims, quads: scene.quads.clone() };
            let mut layer = raster(doc, &sub, bounds, width, height, view);
            if let Some(object) = key.and_then(|id| doc.get_object(id)) {
                let grid = groups::RasterGrid { bounds, size: [width, height], view };
                if let Some(mask) = groups::PixelMask::new(doc, object) {
                    mask.apply(&mut layer, grid);
                }
                apply_transparency(&mut layer, object, bounds, [width, height], view);
            }
            if tone_groups && let Some((_, tone)) = key.and_then(|id| crate::tone_blend::for_layer(doc, id)) {
                for (index, source) in layer.chunks_exact_mut(4).enumerate() {
                    if source[3] == 0 { continue; }
                    let offset = index * 4;
                    let backdrop = &pixels[offset..offset + 4];
                    let softened = (tone.low_pass > 0.0 && backdrop[3] > 0)
                        .then(|| crate::tone_blend::softened_tone(pixels, [width, height], index)).flatten();
                    source[3] = (f32::from(source[3]) * tone.opacity(source, backdrop, softened)).round() as u8;
                }
            }
            composite(pixels, &layer, blend, 1.0);
        }
        run.clear();
    };
    for prim in &scene.prims {
        if let Some(id) = drawn_object(prim) {
            let next = needs_object_run(doc, id, true, tone_groups).then_some(id);
            if next != key {
                flush(pixels, &prefix, &mut run, key);
                prefix = open.clone();
                key = next;
            }
        }
        if let Prim::Clip { rect, .. } = prim {
            if rect.is_some() {
                open.push(prim.clone());
            } else {
                open.pop();
            }
        }
        run.push(prim.clone());
    }
    flush(pixels, &prefix, &mut run, key);
}

pub(super) fn apply_transparency(layer: &mut [u8], object: &crate::Object, bounds: [f32; 4], [width, height]: [u32; 2], view: Option<&ViewState>) {
    use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
    if matches!(object.transparency, crate::paint::Transparency::None) {
        return;
    }
    let Some(inverse) = invert_matrix(&object_matrix(object)) else { return };
    for (index, pixel) in layer.chunks_exact_mut(4).enumerate() {
        if pixel[3] == 0 { continue; }
        let i = index as u32;
        let point = crate::scene::doc_point(bounds, width, height, i % width, i / width, view);
        let [x, y] = apply_matrix(&inverse, point);
        let local = [f64::from(x - object.pos[0]), f64::from(y - object.pos[1])];
        pixel[3] = (f32::from(pixel[3]) * object.transparency.alpha_at(local)).round() as u8;
    }
}
