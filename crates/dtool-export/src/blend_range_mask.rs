use std::collections::{HashMap, HashSet};

use dtool_core::blend::BlendMode;
use dtool_core::blend_composite::range_factor;
use dtool_core::layer_style::raster::{prim_object, render_scene_region};
use dtool_core::paint::{Paint, Transparency};
use dtool_core::scene::{Prim, Scene, build_scene};
use dtool_core::{CanvasFlags, Document, Object, ObjectId, tone_blend};

/// Sample blend-range opacity on the export grid, leaving the vector paint intact.
pub(crate) struct RangeMasks<'a> {
    doc: &'a Document,
    parents: HashMap<ObjectId, Option<ObjectId>>,
    siblings: HashMap<Option<ObjectId>, Vec<ObjectId>>,
}

pub(crate) struct RangeMask {
    pub bounds: [f32; 4],
    pub size: [u32; 2],
    pub alpha: Vec<u8>,
}

impl RangeMasks<'_> {
    pub fn new(doc: &Document) -> Option<RangeMasks<'_>> {
        Self::for_members(doc, None)
    }

    pub(crate) fn for_members<'a>(doc: &'a Document, members: Option<&HashSet<ObjectId>>) -> Option<RangeMasks<'a>> {
        if !doc.objects().iter().any(|object| {
            (object.tone_blend.is_some() || object.blend_ranges.as_ref().is_some_and(|ranges| ranges.is_valid()))
                && dtool_core::scene::is_drawn(doc, object.id)
        }) {
            return None;
        }

        dtool_render::scene::install();
        dtool_text::canvas_text::install();
        let mut parents = HashMap::new();
        let mut siblings = HashMap::<_, Vec<_>>::new();
        for object in doc.objects() {
            if members.is_some_and(|ids| !ids.contains(&object.id))
                || !dtool_core::scene::is_drawn(doc, object.id) || dtool_core::clip_mask::is_mask(doc, object) {
                continue;
            }
            let mut parent = object.parent_id.and_then(|id| doc.get_object(id));
            while let Some(owner) = parent.filter(|owner| !owner.is_group()) {
                parent = owner.parent_id.and_then(|id| doc.get_object(id));
            }
            let parent = parent.map(|owner| owner.id);
            parents.insert(object.id, parent);
            siblings.entry(parent).or_default().push(object.id);
        }
        Some(RangeMasks { doc, parents, siblings })
    }

    fn subtree(&self, id: ObjectId, objects: &mut HashSet<ObjectId>) {
        if objects.insert(id) {
            for &child in self.siblings.get(&Some(id)).into_iter().flatten() {
                self.subtree(child, objects);
            }
        }
    }

    fn collect_backdrop(&self, id: ObjectId) -> (HashSet<ObjectId>, Option<ObjectId>) {
        let mut result = HashSet::new();
        let mut current = id;
        loop {
            let parent = self.parents.get(&current).copied().flatten();
            for &sibling in self.siblings.get(&parent).into_iter().flatten().take_while(|&&sibling| sibling != current) {
                self.subtree(sibling, &mut result);
            }
            let Some(group) = parent.and_then(|id| self.doc.get_object(id)) else { return (result, None); };
            if group.blend_mode != BlendMode::PassThrough
                || group.blend_ranges.as_ref().is_some_and(|ranges| ranges.is_valid())
                || !matches!(group.transparency, Transparency::None)
                || dtool_core::raster_source::compositing_mask(self.doc, group).is_some()
            {
                return (result, parent);
            }
            current = group.id;
        }
    }

    /// Keep the glyph's own vector edge when an isolated tone removes its flat background.
    pub fn clear_text_background(&self, object: &Object) -> Option<Object> {
        use dtool_core::ObjectKind::{ArtText, FrameText};

        let (ArtText { story } | FrameText { story }) = object.kind else { return None; };
        let Paint::Solid { colour: background } = object.fill else { return None; };
        if background.a != 255
            || object.opacity != 1.0
            || object.stroke.width > 0.0 && !matches!(object.stroke.paint, Paint::None)
            || object.blend_ranges.as_ref().is_some_and(|ranges| ranges.is_valid())
            || object.effects.iter().any(|effect| effect.enabled)
            || dtool_core::commands::appearance::has_layers(object)
            || dtool_core::scene::has_live_text_warp(self.doc, object.id)
        {
            return None;
        }

        let (_, tone) = tone_blend::for_layer(self.doc, object.id)?;
        let transparent = [0; 4];
        if tone.opacity(&[background.r, background.g, background.b, background.a], &transparent, None) != 0.0 {
            return None;
        }
        let story = self.doc.stories.iter().find(|entry| entry.id == story)?;
        let Paint::Solid { colour: glyph } = dtool_core::text_lines::story_paint(story) else { return None; };
        if glyph.a != 255 || tone.opacity(&[glyph.r, glyph.g, glyph.b, glyph.a], &transparent, None) != 1.0 {
            return None;
        }

        let (backdrop, boundary) = self.collect_backdrop(object.id);
        if boundary.is_none() || !backdrop.is_empty()
            || !dtool_core::text_lines::text_decorations(self.doc, object.id).is_empty()
        {
            return None;
        }
        Some(Object { fill: Paint::None, ..object.clone() })
    }

    pub fn sample(&self, object: &Object, area: [f32; 4], pixels_per_unit: f32) -> Option<RangeMask> {
        let ranges = object.blend_ranges.as_ref().filter(|ranges| ranges.is_valid());
        let tone = tone_blend::for_layer(self.doc, object.id);
        let source_flat = ranges.map_or(Some(1.0), |ranges| flat_curve(&ranges.source));
        let underlying_flat = ranges.map_or(Some(1.0), |ranges| flat_curve(&ranges.underlying));
        if source_flat == Some(1.0) && underlying_flat == Some(1.0) && tone.is_none() {
            return None;
        }

        let mut source_ids = HashSet::new();
        self.subtree(object.id, &mut source_ids);
        let [x, y, w, h] = area;
        let mut ink = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for id in &source_ids {
            let Some(layer) = self.doc.get_object(*id).filter(|layer| !layer.is_group()) else { continue; };
            let [l, t, r, b] = crate::vector_object_bounds(self.doc, layer);
            ink = [ink[0].min(l), ink[1].min(t), ink[2].max(r), ink[3].max(b)];
        }
        let pad = 1.0 / pixels_per_unit.max(0.01);
        let left = x.max(ink[0] - pad);
        let top = y.max(ink[1] - pad);
        let right = (x + w).min(ink[2] + pad);
        let bottom = (y + h).min(ink[3] + pad);
        let bounds = [left, top, right - left, bottom - top];
        if bounds[2] <= 0.0 || bounds[3] <= 0.0 || !bounds.iter().all(|v| v.is_finite()) {
            return None;
        }
        let size = dtool_core::path_brush::pixel_size([bounds[2], bounds[3]], pixels_per_unit).map(|value| value as u32);

        let (backdrop_ids, boundary) = self.collect_backdrop(object.id);
        let mut doc = self.doc.clone();
        if let Some(boundary) = boundary {
            neutralize(&mut doc, Some(boundary));
        }
        let flags = CanvasFlags::default();
        let backdrop = ((underlying_flat != Some(1.0) || tone.is_some()) && !backdrop_ids.is_empty()).then(|| {
            let scene = subset_scene(&doc, &flags, &backdrop_ids);
            render_scene_region(&doc, &flags, &[], &scene, bounds, size, 1.0)
        });
        let source = (source_flat.is_none() || tone.is_some()).then(|| {
            if let Some((group, _)) = tone && let Some(layer) = doc.find_object_mut(group.0) {
                layer.tone_blend = None;
            }
            neutralize(&mut doc, Some(object.id));
            if !object.is_group() && let Some(layer) = doc.objects.iter_mut().find(|layer| layer.id == object.id) {
                layer.opacity = object.opacity;
            }
            let scene = subset_scene(&doc, &flags, &source_ids);
            if !object.is_group() {
                // Leaf range settings apply to its own ink. Effects composite separately.
                let items = dtool_core::layer_style::raster::scene_items(&scene);
                return dtool_core::layer_style::raster::rasterize_items(&doc, &scene, &items, bounds, size[0], size[1]);
            }
            render_scene_region(&doc, &flags, &[], &scene, bounds, size, 1.0)
        });
        let count = size[0] as usize * size[1] as usize;
        let alpha: Vec<_> = (0..count).map(|pixel| {
            let i = pixel * 4;
            let src = pixel_at(source.as_deref(), i);
            let under = pixel_at(backdrop.as_deref(), i);
            let range_factor = ranges.map_or(1.0, |ranges| range_factor(ranges, src, under));
            let tone_factor = tone.map_or(1.0, |(_, tone)| {
                let softened = backdrop.as_deref().filter(|_| tone.low_pass > 0.0 && under[3] > 0)
                    .and_then(|backdrop| tone_blend::softened_tone(backdrop, size, pixel));
                tone.opacity(src, under, softened)
            });
            (255.0 * range_factor * tone_factor).round() as u8
        }).collect();
        if alpha.iter().all(|&a| a == 255) {
            return None;
        }
        Some(RangeMask { bounds, size, alpha })
    }
}

fn flat_curve(points: &[[f32; 2]]) -> Option<f32> {
    let value = points.first()?[1];
    points.iter().all(|point| point[1] == value).then_some(value)
}

fn neutralize(doc: &mut Document, mut current: Option<ObjectId>) {
    while let Some(id) = current {
        let Some(object) = doc.objects.iter_mut().find(|object| object.id == id) else { break; };
        current = object.parent_id;
        object.opacity = 1.0;
        object.blend_mode = if object.is_group() { BlendMode::PassThrough } else { BlendMode::Normal };
        object.blend_ranges = None;
        object.transparency = Transparency::None;
        doc.layer_masks.retain(|mask| mask.object != id);
    }
}

fn subset_scene(doc: &Document, flags: &CanvasFlags, ids: &HashSet<ObjectId>) -> Scene {
    let mut scene = build_scene(doc, flags);
    scene.prims.retain(|prim| match prim {
        Prim::Page { .. } => false,
        Prim::Clip { object, .. } => ids.contains(object),
        _ => prim_object(prim).is_some_and(|id| ids.contains(&id)),
    });
    scene
}

fn pixel_at(buffer: Option<&[u8]>, offset: usize) -> &[u8] {
    buffer.map_or(&[0, 0, 0, 0], |pixels| &pixels[offset..offset + 4])
}
