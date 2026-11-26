use dtool_core::blend::BlendMode;
use dtool_core::layer_style::raster::{EffectLayer, blurred_base, composite_rank, draw_layer, effect_bounds, effect_margin, group_effect_enabled, group_effect_on_top, group_effect_padding, group_effect_pixels, group_effect_region, group_effect_scene, prim_object};
use dtool_core::path_brush::Ink;
use dtool_core::scene::{Prim, Scene, effective_opacity};
use dtool_core::{Document, Object};

pub(crate) fn bitmap_mask_tile(doc: &Document, bitmap: &dtool_core::paint::BitmapPaint) -> Option<([u32; 2], Vec<u8>)> {
    let (size, mut pixels) = dtool_core::scene::bitmap_source(doc, &bitmap.resource_id)?;
    let size = [u32::try_from(size[0]).ok()?, u32::try_from(size[1]).ok()?];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[255, 255, 255, 255 - pixel[3]]);
    }
    Some((size, pixels))
}

pub(crate) fn transparency_pixels(object: &Object, bounds: [f32; 4], scale: f32) -> Option<([u32; 2], Vec<u8>)> {
    let [x, y, w, h] = bounds;
    if !bounds.iter().all(|value| value.is_finite()) || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let inverse = dtool_core::geometry::invert_matrix(&dtool_core::geometry::object_matrix(object))?;
    let [width, height] = dtool_core::path_brush::pixel_size([w, h], scale);
    let mut pixels = vec![255; width * height * 4];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let point = [x + (index % width) as f32 * w / width as f32 + w / (2 * width) as f32,
                     y + (index / width) as f32 * h / height as f32 + h / (2 * height) as f32];
        let [px, py] = dtool_core::geometry::apply_matrix(&inverse, point);
        let local = [f64::from(px - object.pos[0]), f64::from(py - object.pos[1])];
        pixel[3] = (object.transparency.alpha_at(local) * 255.0).round() as u8;
    }
    Some(([width as u32, height as u32], pixels))
}

pub(crate) fn size(bounds: [f32; 4], layers: &[EffectLayer], pixels_per_unit: f32) -> Option<[u32; 2]> {
    if layers.is_empty() {
        return None;
    }
    let halo = layers.iter().map(EffectLayer::raster_padding).fold(0.0, f32::max) * 2.0;
    raster_size(bounds, halo, pixels_per_unit)
}

fn raster_size(bounds: [f32; 4], halo: f32, pixels_per_unit: f32) -> Option<[u32; 2]> {
    let [_, _, width, height] = bounds;
    if !bounds.iter().all(|value| value.is_finite()) || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let [padded_w, padded_h] = [width + halo, height + halo];
    let scale = pixels_per_unit.min(8192.0 / padded_w.max(padded_h))
        .min((16_000_000.0 / padded_w / padded_h).sqrt());
    Some([width, height].map(|span| ((span * scale).ceil() as u32).max(1)))
}

pub(crate) struct EffectImage {
    pub bounds: [f32; 4],
    pub size: [u32; 2],
    pub pixels: Vec<u8>,
}

/// A single solid fill and a single blur can be represented by vector paint
/// with only its coverage sampled, or by SVG's native vector filter.
pub(crate) fn solid_leaf_blur<'a>(object: &Object, sources: Option<&'a [Prim]>) -> Option<(&'a Prim, dtool_core::layer_style::raster::GaussianBlur)> {
    use dtool_core::ObjectKind;
    use dtool_core::layer_style::raster::group_blur_params;
    use dtool_core::paint::Paint;

    if !matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_))
        || object.path_brush.is_some()
    {
        return None;
    }
    let [source @ Prim::Fill { paint: Paint::Solid { .. }, .. }] = sources? else { return None };
    let mut enabled = object.effects.iter().filter(|effect| effect.enabled);
    let effect = enabled.next()?;
    let blur = group_blur_params(effect)?;
    (!blur.preserve_alpha && enabled.next().is_none()).then_some((source, blur))
}

pub(crate) fn rgba(doc: &Document, bounds: [f32; 4], [w, h]: [u32; 2], layer: &EffectLayer) -> Option<EffectImage> {
    let mut pixels = vec![0; w as usize * h as usize * 4];
    let mut source = layer.clone();
    source.blend = BlendMode::Normal;
    draw_layer(doc, &mut pixels, &source, bounds, w, h);
    trim_rgba(bounds, [w, h], pixels)
}

fn trim_rgba(bounds: [f32; 4], [w, h]: [u32; 2], pixels: Vec<u8>) -> Option<EffectImage> {
    let stride = w as usize * 4;
    let (mut left, mut top, mut right, mut bottom) = (w as usize, h as usize, 0, 0);
    for (y, row) in pixels.chunks_exact(stride).enumerate() {
        for (x, pixel) in row.chunks_exact(4).enumerate() {
            if pixel[3] != 0 {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = y + 1;
            }
        }
    }
    if right == 0 {
        return None;
    }
    // Keep a transparent pixel next to the ink for image interpolation.
    left = left.saturating_sub(1);
    top = top.saturating_sub(1);
    right = (right + 1).min(w as usize);
    bottom = (bottom + 1).min(h as usize);
    if [left, top, right, bottom] == [0, 0, w as usize, h as usize] {
        return Some(EffectImage { bounds, size: [w, h], pixels });
    }

    let [x, y, width, height] = bounds;
    let step_x = width / w as f32;
    let step_y = height / h as f32;
    let size = [(right - left) as u32, (bottom - top) as u32];
    let bounds = [x + left as f32 * step_x, y + top as f32 * step_y,
        size[0] as f32 * step_x, size[1] as f32 * step_y];
    let mut cropped = Vec::with_capacity(size[0] as usize * size[1] as usize * 4);
    for row in pixels.chunks_exact(stride).skip(top).take(size[1] as usize) {
        cropped.extend_from_slice(&row[left * 4..right * 4]);
    }
    Some(EffectImage { bounds, size, pixels: cropped })
}

pub(crate) use dtool_core::layer_style::raster::has_group_effect;

#[derive(Default)]
pub(crate) struct GroupEffectImages {
    pub behind: Vec<(BlendMode, EffectImage)>,
    pub above: Vec<(BlendMode, EffectImage)>,
}

pub(crate) struct PdfGroupBlur {
    pub source_mask: crate::blend_range_mask::RangeMask,
    pub raster: EffectImage,
    pub clip_to_bounds: bool,
}

pub(crate) fn pdf_group_blur(
    doc: &Document, group: &Object, scene: Option<&Scene>, pixels_per_unit: f32, points_per_unit: f32,
) -> Option<PdfGroupBlur> {
    use dtool_core::layer_style::raster::{group_blur_params, group_effect_target, render_scene_region};

    let scene = scene?;
    if !group_effect_target(group) || effective_opacity(doc, group.id) <= 0.0 {
        return None;
    }
    let blur = group.effects.iter().find_map(group_blur_params)?;
    let members = doc.subtree_ids(group.id);
    let belongs = |prim: &&Prim| prim_object(prim).is_some_and(|id| members.contains(&id));
    let [x, y, width, height] = effect_bounds(doc, &[], group.id, scene.prims.iter().filter(belongs))?;
    // A child's ink can already extend past its geometry before the group is blurred.
    let child_margin = doc.objects().iter().filter(|object| {
        object.id != group.id && members.contains(&object.id) && object.effects.iter().any(|effect| effect.enabled)
    }).map(|object| {
        let mut margin = 0.0;
        let mut parent = Some(object.id);
        while let Some(id) = parent.filter(|id| *id != group.id) {
            margin += effect_margin(doc, id, &[]);
            parent = doc.get_object(id).and_then(|object| object.parent_id);
        }
        margin
    }).fold(0.0, f32::max);
    let mut bounds = [x - child_margin, y - child_margin, width + 2.0 * child_margin, height + 2.0 * child_margin];
    let exact_axis = blur.radii.iter().position(|radius| *radius == 0.0).filter(|_| blur.angle == 0.0 && child_margin == 0.0
        && group.effects.iter().filter(|effect| effect.enabled).count() == 1
        && scene.prims.iter().filter(belongs).all(|prim| matches!(prim,
            Prim::Fill { .. } | Prim::Appearance { .. } | Prim::Clip { .. } | Prim::Mark { .. } | Prim::Handle { .. })));
    if let Some(axis) = exact_axis {
        // In the unblurred direction the fill bounds still delimit the ink.
        // Start the sampled replacement at the vector edge, so there is no
        // partially covered source row or column for a PDF reader to resample.
        let margin = effect_margin(doc, group.id, &[]);
        bounds[axis] += margin;
        bounds[axis + 2] -= 2.0 * margin;
    }
    let size = raster_size(bounds, 0.0, pixels_per_unit)?;
    let scene = group_effect_scene(doc, group.id, scene.prims.iter().filter(belongs));
    let mut source = doc.clone();
    let layer = source.find_object_mut(group.id.0)?;
    // The PDF group applies its own opacity, masks and blend to both parts together.
    layer.parent_id = None;
    layer.opacity = 1.0;
    layer.blend_mode = BlendMode::Normal;
    layer.transparency = dtool_core::paint::Transparency::None;
    layer.blend_ranges = None;
    layer.tone_blend = None;
    source.layer_masks.retain(|mask| mask.object != group.id);
    let flags = dtool_core::CanvasFlags::default();
    let effect_scale = (size[0] as f32 / bounds[2]).min(size[1] as f32 / bounds[3]);
    let mut blurred = render_scene_region(&source, &flags, &[], &scene, bounds, size, effect_scale);
    source.find_object_mut(group.id.0)?.effects.retain(|effect| group_blur_params(effect).is_none());
    let original = render_scene_region(&source, &flags, &[], &scene, bounds, size, effect_scale);
    if blurred == original {
        return None;
    }
    let alpha: Vec<u8> = blurred.chunks_exact(4).zip(original.chunks_exact(4))
        .map(|(after, before)| if after == before { 255 } else { 0 }).collect();
    drop(original);
    let stride = size[0] as usize;
    // Leave matching opaque ink under the soft mask boundary when a PDF viewer
    // downsamples the two image streams separately.
    let overlap = (effect_scale * 3.0 / points_per_unit).ceil().clamp(2.0, 64.0) as u8;
    let mut distance = alpha.clone();
    for index in 0..distance.len() {
        if index % stride != 0 {
            distance[index] = distance[index].min(distance[index - 1].saturating_add(1));
        }
        if index >= stride {
            distance[index] = distance[index].min(distance[index - stride].saturating_add(1));
        }
    }
    for index in (0..distance.len()).rev() {
        if index % stride != stride - 1 {
            distance[index] = distance[index].min(distance[index + 1].saturating_add(1));
        }
        if index + stride < distance.len() {
            distance[index] = distance[index].min(distance[index + stride].saturating_add(1));
        }
    }
    for (index, pixel) in blurred.chunks_exact_mut(4).enumerate() {
        if alpha[index] == 255 && (pixel[3] < 255 || distance[index] > overlap) {
            // PDF readers can filter colour and coverage separately.
            pixel[3] = 0;
        }
    }
    let raster = EffectImage { bounds, size, pixels: blurred };
    Some(PdfGroupBlur {
        source_mask: crate::blend_range_mask::RangeMask { bounds, size, alpha },
        raster,
        clip_to_bounds: exact_axis.is_some(),
    })
}

pub(crate) fn group_effect_images(
    doc: &Document, group: &Object, scene: Option<&Scene>, scale: f32,
) -> GroupEffectImages {
    let mut images = GroupEffectImages::default();
    let Some(scene) = scene.filter(|_| has_group_effect(group)) else { return images };
    let members = doc.subtree_ids(group.id);
    let opacity = effective_opacity(doc, group.id);
    if opacity <= 0.0 {
        return images;
    }
    let belongs = |prim: &&Prim| prim_object(prim).is_some_and(|id| id != group.id && members.contains(&id));
    let Some(bounds) = effect_bounds(doc, &[], group.id, scene.prims.iter().filter(belongs)) else { return images };
    let scene = group_effect_scene(doc, group.id, scene.prims.iter().filter(belongs));
    let margin = effect_margin(doc, group.id, &[]);
    let mut effects: Vec<_> = group.effects.iter().filter(|effect| group_effect_enabled(effect)).collect();
    effects.sort_by_key(|effect| composite_rank(effect.kind));
    for effect in effects {
        let bounds = group_effect_region(bounds, margin, effect);
        let Some(size) = raster_size(bounds, group_effect_padding(effect) * 2.0, scale) else { continue };
        let Some(pixels) = group_effect_pixels(doc, group.id, &scene, effect, bounds, size) else { continue };
        let Some(raster) = trim_rgba(bounds, size, pixels) else { continue };
        let target = if group_effect_on_top(effect) { &mut images.above } else { &mut images.behind };
        target.push((effect.blend, raster));
    }
    images
}

pub(crate) fn brush_blur(object: &Object, ink: &Ink) -> Option<([f32; 4], EffectLayer)> {
    let prim = Prim::Image { object: object.id, quad: ink.quad(), opacity: object.opacity, sample_in_quad: false };
    let layer = blurred_base(&prim, &object.effects)?;
    let [x, y, width, height] = ink.bounds;
    let pad = layer.raster_padding() + 1.0;
    Some(([x - pad, y - pad, width + 2.0 * pad, height + 2.0 * pad], layer))
}
