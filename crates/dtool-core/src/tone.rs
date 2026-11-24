use crate::blend::{blend, BlendMode};
use crate::geometry::{apply_matrix, compose, invert_matrix, object_matrix, IDENTITY};
use crate::pixel::TILE_SIZE;
use crate::{Document, Object};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    DodgeBurn,
    Blend,
    InverseBlend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Range {
    Full,
    Shadows,
    Midtones,
    Highlights,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToneLayer {
    pub mode: Mode,
    pub range: Range,
}

pub fn attached(document: &Document, object: &Object) -> bool {
    object.tone_layer.is_some() && object.parent_id
        .and_then(|id| document.get_object(id))
        .is_some_and(|source| source.tone_layer.is_none() && crate::raster_source::dimensions(document, source).is_some())
}

pub(crate) fn apply(document: &Document, source: &Object, size: [usize; 2], pixels: &mut [u8]) {
    if source.tone_layer.is_some() { return; }
    let [source_width, source_height] = size;
    for layer in document.objects.iter().filter(|object| object.parent_id == Some(source.id) && object.pickable) {
        let Some(spec) = layer.tone_layer else { continue; };
        if layer.opacity <= 0.0 || !layer.opacity.is_finite() { continue; }
        let Some([width, height]) = crate::raster_source::dimensions(document, layer) else { continue; };
        let Some(inverse) = invert_matrix(&object_matrix(layer)) else { continue; };
        let relative = compose(&inverse, &object_matrix(source));
        let direct = relative.iter().zip(IDENTITY).all(|(value, identity)| (*value - identity).abs() < 1e-5)
            && source.pos == layer.pos && source.size == layer.size && size == [width, height];
        let native = document.pixel_layers.get(layer.id);
        let legacy = if native.is_none() { crate::scene::raster_rgba(layer).map(|(_, rgba)| rgba) } else { None };
        let mask = document.layer_masks.iter().find(|mask| mask.object == layer.id)
            .map(|mask| &mask.coverage).filter(|mask| {
                [mask.width as usize, mask.height as usize] == [width, height]
                    && mask.coverage.len() == width * height
            });
        let ranges = layer.blend_ranges.as_ref().filter(|ranges| ranges.is_valid());
        let amount = |ink: &[u8], base: &[u8], x, y| {
            let mask = mask.map_or(1.0, |mask| f32::from(mask.get(x, y)) / 255.0);
            let ranges = ranges.map_or(1.0, |ranges| crate::blend_composite::range_factor(ranges, ink, base));
            layer.opacity * mask * ranges
        };
        if direct && let Some(native) = native {
            let columns = native.tile_columns();
            for (index, tile) in native.tiles.iter().enumerate().filter_map(|(i, tile)| Some((i as u32, tile.as_ref()?))) {
                let ox = (index % columns) * TILE_SIZE;
                let oy = (index / columns) * TILE_SIZE;
                for y in oy..(oy + TILE_SIZE).min(native.height) {
                    for x in ox..(ox + TILE_SIZE).min(native.width) {
                        let ink = tile.pixel(x - ox, y - oy);
                        if ink[3] == 0 { continue; }
                        let index = (y as usize * width + x as usize) * 4;
                        let base = &mut pixels[index..index + 4];
                        if base[3] == 0 { continue; }
                        let opacity = amount(&ink, base, x, y);
                        apply_pixel(base, ink, spec.range, layer.blend_mode, opacity);
                    }
                }
            }
            continue;
        }
        let [sw, sh] = source.size.map(f32::abs);
        let [lw, lh] = layer.size.map(f32::abs);
        if [sw, sh, lw, lh].iter().any(|v| !v.is_finite() || *v <= 0.0) { continue; }
        for (index, base) in pixels.chunks_exact_mut(4).enumerate() {
            if base[3] == 0 { continue; }
            let x = index % source_width;
            let y = index / source_width;
            let [x, y] = if direct {
                [x, y]
            } else {
                let point = [
                    source.pos[0] + (x as f32 + 0.5) * sw / source_width as f32,
                    source.pos[1] + (y as f32 + 0.5) * sh / source_height as f32,
                ];
                let [x, y] = apply_matrix(&relative, point);
                let u = (x - layer.pos[0]) / lw;
                let v = (y - layer.pos[1]) / lh;
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) { continue; }
                [((u * width as f32) as usize).min(width - 1), ((v * height as f32) as usize).min(height - 1)]
            };
            let ink = if let Some(native) = native {
                native.get(x as u32, y as u32)
            } else if let Some(legacy) = &legacy {
                let index = (y * width + x) * 4;
                [legacy[index], legacy[index + 1], legacy[index + 2], legacy[index + 3]]
            } else {
                continue;
            };
            if ink[3] == 0 { continue; }
            let opacity = amount(&ink, base, x as u32, y as u32);
            apply_pixel(base, ink, spec.range, layer.blend_mode, opacity);
        }
    }
}

fn apply_pixel(base: &mut [u8], ink: [u8; 4], range: Range, mode: BlendMode, opacity: f32) {
    let [r, g, b] = [0, 1, 2].map(|i| f32::from(base[i]) / 255.0);
    let range = match range {
        Range::Full => 1.0,
        Range::Shadows => crate::brush::tonal_weight([r, g, b], crate::brush::TonalRange::Shadows),
        Range::Midtones => crate::brush::tonal_weight([r, g, b], crate::brush::TonalRange::Midtones),
        Range::Highlights => crate::brush::tonal_weight([r, g, b], crate::brush::TonalRange::Highlights),
    };
    let [ir, ig, ib, ia] = ink.map(|channel| f32::from(channel) / 255.0);
    let mixed = blend([r, g, b, 1.0], [ir, ig, ib, (ia * opacity * range).clamp(0.0, 1.0)], mode);
    for i in 0..3 {
        base[i] = (mixed[i].clamp(0.0, 1.0) * 255.0).round() as u8;
    }
}
