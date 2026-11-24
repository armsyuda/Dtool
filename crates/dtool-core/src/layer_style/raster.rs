use crate::blend::{blend_pixel, BlendMode};
use crate::blend_composite::groups;
use crate::commands::layer_style::StagedEffect;
use crate::layer_style::{BevelType, EffectKind, EffectParams, LayerEffect, OutlineAlign};
use crate::paint::{Colour, Paint};
use crate::scene::{build_scene, contours_bounds, inside, rasterize_fill, rasterize_scene, representative_colour, Prim, Scene};
use crate::{CanvasFlags, Document, ObjectId};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

type Contours = Vec<Vec<[f32; 2]>>;

#[derive(Debug, Clone, PartialEq)]
pub enum Clip {
    Inside(Contours),
    Outside(Contours),
    Alpha { source: Box<Prim>, inside: bool },
    PreserveAlpha,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectLayer {
    pub object: ObjectId,
    pub prim: Prim,
    pub blur: f32,
    pub blend: BlendMode,
    pub alpha: f32,
    pub clip: Option<Clip>,
    pub invert: bool,
    pub spread: f32,
    pub tint: Option<Paint>,
    alpha_edge: Option<AlphaEdge>,
    image_clip: Option<std::sync::Arc<EffectImageClip>>,
    group_effect: Option<(Arc<Scene>, LayerEffect, [f32; 4])>,
}

#[derive(Debug, Clone, PartialEq)]
struct EffectImageClip {
    origin: [f32; 2],
    contours: Contours,
}

#[derive(Debug, Clone, PartialEq)]
enum AlphaEdge {
    Outline { width: f32, align: OutlineAlign },
    Bevel { reference: Box<Prim>, inner: bool },
}

impl EffectLayer {
    fn outline_radius(&self) -> f32 {
        match &self.alpha_edge {
            Some(AlphaEdge::Outline { width, align: OutlineAlign::Centre }) => *width * 0.5,
            Some(AlphaEdge::Outline { width, .. }) => *width,
            _ => 0.0,
        }
    }

    pub fn raster_padding(&self) -> f32 {
        let group = self.group_effect.as_ref().map_or(0.0, |(_, effect, _)| group_effect_padding(effect));
        (self.blur * 1.5).max(self.outline_radius()).max(group)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Plain(Prim),
    Exact(Prim),
    Layer(EffectLayer),
}

impl Item {
    pub fn object(&self) -> Option<ObjectId> {
        match self {
            Item::Plain(prim) | Item::Exact(prim) => prim_object(prim),
            Item::Layer(layer) => Some(layer.object),
        }
    }

    pub fn prim(&self) -> &Prim {
        match self {
            Item::Plain(prim) | Item::Exact(prim) => prim,
            Item::Layer(layer) => &layer.prim,
        }
    }
}

pub fn prim_object(prim: &Prim) -> Option<ObjectId> {
    match prim {
        Prim::Page { .. } => None,
        Prim::Fill { object, .. }
        | Prim::Appearance { object, .. }
        | Prim::Stroke { object, .. }
        | Prim::Image { object, .. }
        | Prim::Text { object, .. }
        | Prim::Mark { object, .. }
        | Prim::Handle { object, .. }
        | Prim::Label { object, .. }
        | Prim::Clip { object, .. } => Some(*object),
    }
}

pub fn live_effects(doc: &Document, id: ObjectId, staged: &[StagedEffect]) -> Vec<LayerEffect> {
    let mut list = doc.get_object(id).map(|object| object.effects.clone()).unwrap_or_default();
    for item in staged.iter().filter(|item| item.object == id) {
        match item.index {
            Some(slot) => {
                if let Some(entry) = list.get_mut(slot) {
                    *entry = item.effect.clone();
                }
            }
            None => list.push(item.effect.clone()),
        }
    }
    list.retain(|effect| effect.enabled);
    list
}

pub fn effects_key(doc: &Document, id: ObjectId, staged: &[StagedEffect]) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{:?}", live_effects(doc, id, staged)).hash(&mut hasher);
    hasher.finish()
}

fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn unit(value: f32) -> f32 {
    finite(value, 0.0).clamp(0.0, 1.0)
}

fn reach(effect: &LayerEffect) -> f32 {
    let blur = |radius: f32| finite(radius, 0.0).max(0.0) * 1.5 + 1.0;
    match &effect.params {
        EffectParams::Shadow { radius, offset, .. } if effect.kind == EffectKind::OuterShadow => finite(*offset, 0.0).abs() + blur(*radius),
        EffectParams::Glow { radius, .. } if effect.kind == EffectKind::OuterGlow => blur(*radius),
        EffectParams::Outline { width, align, .. } if *align != OutlineAlign::Inside => finite(*width, 0.0).max(0.0) * 2.0 + 1.0,
        EffectParams::Blur { radius, .. } => blur(*radius),
        EffectParams::Light { bevel, soften, .. } if *bevel != BevelType::Inner => bevel_distance(&effect.params) + blur(*soften),
        _ => 0.0,
    }
}

pub fn effect_margin(doc: &Document, id: ObjectId, staged: &[StagedEffect]) -> f32 {
    live_effects(doc, id, staged).iter().map(reach).fold(0.0, f32::max)
}

fn with_paint(prim: &Prim, d: [f32; 2], paint: Paint) -> Option<Prim> {
    let shift = |p: &[f32; 2]| [p[0] + d[0], p[1] + d[1]];
    match prim {
        Prim::Fill { object, contours, origin, .. } => Some(Prim::Fill { object: *object, contours: contours.iter().map(|c| c.iter().map(shift).collect()).collect(), origin: shift(origin), paint, opacity: 1.0 }),
        Prim::Appearance { object, inks, origin, .. } => {
            let Paint::Solid { colour } = paint else { return None; };
            let mut inks = inks.clone();
            for ink in &mut inks {
                for point in ink.contours.iter_mut().flatten() {
                    *point = shift(point);
                }
            }
            Some(Prim::Appearance { object: *object, inks, origin: shift(origin), opacity: 1.0, tint: Some(colour) })
        }
        Prim::Stroke { object, points, closed, width, .. } => Some(Prim::Stroke { object: *object, points: points.iter().map(shift).collect(), closed: *closed, width: *width, paint, opacity: 1.0 }),
        _ => None,
    }
}

pub fn is_path_brush_image(doc: &Document, prim: &Prim) -> bool {
    matches!(prim, Prim::Image { object, .. } if doc.get_object(*object).is_some_and(|object| object.path_brush.is_some()))
}

fn with_effect_paint(prim: &Prim, d: [f32; 2], paint: Paint) -> Option<(Prim, Option<Paint>)> {
    match prim {
        Prim::Image { object, quad, .. } => {
            let shifted = quad.map(|p| [p[0] + d[0], p[1] + d[1]]);
            Some((Prim::Image { object: *object, quad: shifted, opacity: 1.0 }, Some(paint)))
        }
        Prim::Fill { paint: bitmap @ Paint::Bitmap(_), .. } => {
            Some((with_paint(prim, d, bitmap.clone())?, Some(paint)))
        }
        _ => Some((with_paint(prim, d, paint)?, None)),
    }
}

fn prim_opacity(prim: &Prim) -> f32 {
    match prim {
        Prim::Fill { opacity, .. } | Prim::Appearance { opacity, .. } | Prim::Stroke { opacity, .. } | Prim::Image { opacity, .. } => unit(*opacity),
        _ => 1.0,
    }
}

fn fill_contours(prim: &Prim) -> Option<Contours> {
    match prim {
        Prim::Fill { contours, .. } => Some(contours.clone()),
        _ => None,
    }
}

fn effect_clip(prim: &Prim, inside: bool) -> Option<Clip> {
    if let Prim::Appearance { inks, origin, object, .. } = prim {
        let source = Prim::Appearance { object: *object, inks: inks.clone(), origin: *origin, opacity: 1.0, tint: None };
        return Some(Clip::Alpha { source: Box::new(source), inside });
    }
    if let Prim::Image { object, quad, .. } = prim {
        let source = Prim::Image { object: *object, quad: *quad, opacity: 1.0 };
        return Some(Clip::Alpha { source: Box::new(source), inside });
    }
    if let Prim::Fill { paint: paint @ Paint::Bitmap(_), .. } = prim {
        let source = with_paint(prim, [0.0, 0.0], paint.clone())?;
        return Some(Clip::Alpha { source: Box::new(source), inside });
    }
    fill_contours(prim).map(|contours| if inside { Clip::Inside(contours) } else { Clip::Outside(contours) })
}

fn offset(angle_deg: f32, distance: f32) -> [f32; 2] {
    let angle = finite(angle_deg, 0.0).to_radians();
    let distance = finite(distance, 0.0);
    [-angle.cos() * distance, angle.sin() * distance]
}

fn lean(elevation: f32) -> f32 {
    finite(elevation, 45.0).clamp(0.0, 90.0).to_radians().cos() * std::f32::consts::SQRT_2
}

pub fn bevel_distance(params: &EffectParams) -> f32 {
    match params {
        EffectParams::Light { radius, depth, elevation, .. } => (finite(*radius, 0.0).max(0.0) * 0.5 * finite(*depth, 1.0).max(0.0) * lean(*elevation)).max(1.0),
        _ => 0.0,
    }
}

pub fn bevel_strength(params: &EffectParams) -> f32 {
    match params {
        EffectParams::Light { elevation, .. } => lean(*elevation).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

struct Spec {
    object: ObjectId,
    alpha: f32,
    image_clip: Option<std::sync::Arc<EffectImageClip>>,
}

impl Spec {
    fn layer(&self, prim: Option<(Prim, Option<Paint>)>, blur: f32, blend: BlendMode, clip: Option<Clip>, invert: bool, spread: f32) -> Vec<EffectLayer> {
        prim.map(|(prim, tint)| EffectLayer { object: self.object, prim, blur: finite(blur, 0.0).max(0.0), blend, alpha: self.alpha, clip, invert, spread: unit(spread).min(0.99), tint, alpha_edge: None, image_clip: self.image_clip.clone(), group_effect: None }).into_iter().collect()
    }
}

fn alpha_effect_source(prim: &Prim, offset: [f32; 2]) -> Option<Prim> {
    let (mut source, _) = with_effect_paint(prim, offset, Paint::Solid { colour: Colour::new(255, 255, 255, 255) })?;
    if let Prim::Appearance { tint, .. } = &mut source {
        *tint = None;
    }
    Some(source)
}

fn uses_alpha_edges(prim: &Prim) -> bool {
    matches!(prim, Prim::Image { .. } | Prim::Appearance { .. } | Prim::Fill { paint: Paint::Bitmap(_), .. })
}

fn solid(colour: &Colour) -> Paint {
    Paint::Solid { colour: colour.clone() }
}

fn effect_layers(doc: &Document, prim: &Prim, effect: &LayerEffect) -> Option<(bool, Vec<EffectLayer>)> {
    let object = prim_object(prim)?;
    let image_clip = match prim {
        Prim::Image { quad, .. } => crate::clip_mask::clip_region(doc, object).map(|region| {
            std::sync::Arc::new(EffectImageClip {
                origin: quad[0],
                contours: region.into_iter().map(|ring| ring.into_iter().map(|p| p.map(|v| v as f32)).collect()).collect(),
            })
        }),
        _ => None,
    };
    let spec = Spec { object, alpha: unit(effect.opacity) * prim_opacity(prim), image_clip };
    let blend = effect.blend.clone();
    let inner = effect_clip(prim, true);
    match (&effect.kind, &effect.params) {
        (EffectKind::OuterShadow, EffectParams::Shadow { radius, offset: distance, angle, intensity, colour, knock_out }) => {
            let clip = if *knock_out { effect_clip(prim, false) } else { None };
            Some((false, spec.layer(with_effect_paint(prim, offset(*angle, *distance), solid(colour)), *radius, blend, clip, false, *intensity)))
        }
        (EffectKind::InnerShadow, EffectParams::Shadow { radius, offset: distance, angle, intensity, colour, .. }) => {
            let clip = inner?;
            Some((true, spec.layer(with_effect_paint(prim, offset(*angle, *distance), solid(colour)), *radius, blend, Some(clip), true, *intensity)))
        }
        (EffectKind::OuterGlow, EffectParams::Glow { radius, intensity, colour, .. }) => {
            Some((false, spec.layer(with_effect_paint(prim, [0.0, 0.0], solid(colour)), *radius, blend, effect_clip(prim, false), false, *intensity)))
        }
        (EffectKind::InnerGlow, EffectParams::Glow { radius, intensity, colour, from_centre }) => {
            let clip = inner?;
            Some((true, spec.layer(with_effect_paint(prim, [0.0, 0.0], solid(colour)), *radius, blend, Some(clip), !from_centre, *intensity)))
        }
        (EffectKind::ColourOverlay | EffectKind::GradientOverlay, EffectParams::Overlay { fill }) => Some((true, spec.layer(with_effect_paint(prim, [0.0, 0.0], fill.clone()), 0.0, blend, None, false, 0.0))),
        (EffectKind::Outline, EffectParams::Outline { width, align, fill }) => {
            let width = finite(*width, 0.0);
            if width <= 0.0 {
                return None;
            }
            if uses_alpha_edges(prim) {
                let source = alpha_effect_source(prim, [0.0, 0.0])?;
                let mut layers = spec.layer(Some((source, Some(fill.clone()))), 0.0, blend, None, false, 0.0);
                layers[0].alpha_edge = Some(AlphaEdge::Outline { width, align: *align });
                return Some((*align != OutlineAlign::Outside, layers));
            }
            let contours = fill_contours(prim)?;
            let (above, stroke, clip) = match align {
                OutlineAlign::Outside => (false, width * 2.0, Some(Clip::Outside(contours.clone()))),
                OutlineAlign::Centre => (true, width, None),
                OutlineAlign::Inside => (true, width * 2.0, Some(Clip::Inside(contours.clone()))),
            };
            let layers = contours
                .iter()
                .filter(|c| c.len() >= 2)
                .flat_map(|c| spec.layer(Some((Prim::Stroke { object, points: c.clone(), closed: true, width: stroke, paint: fill.clone(), opacity: 1.0 }, None)), 0.0, blend.clone(), clip.clone(), false, 0.0))
                .collect();
            Some((above, layers))
        }
        (EffectKind::BevelEmboss | EffectKind::ThreeD, EffectParams::Light { soften, azimuth, bevel, highlight, shadow, .. }) => {
            let strength = bevel_strength(&effect.params);
            if strength <= 0.0 {
                return None;
            }
            let spec = Spec { alpha: spec.alpha * strength, ..spec };
            let full = bevel_distance(&effect.params);
            let soft = finite(*soften, 0.0).max(0.0);
            let use_alpha = uses_alpha_edges(prim);
            let alpha_reference = use_alpha.then(|| alpha_effect_source(prim, [0.0, 0.0])).flatten();
            if alpha_reference.is_none() && fill_contours(prim).is_none() {
                return None;
            }
            let pair = |distance: f32, inner: bool| {
                let cast = offset(*azimuth, distance);
                let back = [-cast[0], -cast[1]];
                let (light_delta, shadow_delta) = if inner { (cast, back) } else { (back, cast) };
                let mut layers = Vec::new();
                for (delta, colour, blend) in [(light_delta, highlight, BlendMode::Screen), (shadow_delta, shadow, BlendMode::Multiply)] {
                    let mut layer = if let Some(reference) = &alpha_reference {
                        let source = alpha_effect_source(prim, delta).map(|source| (source, Some(solid(colour))));
                        let mut out = spec.layer(source, soft, blend, None, false, 0.0);
                        for item in &mut out {
                            item.alpha_edge = Some(AlphaEdge::Bevel { reference: Box::new(reference.clone()), inner });
                        }
                        out
                    } else {
                        spec.layer(with_effect_paint(prim, delta, solid(colour)), soft, blend, effect_clip(prim, inner), inner, 0.0)
                    };
                    layers.append(&mut layer);
                }
                layers
            };
            let layers = match bevel {
                BevelType::Inner => pair(full, true),
                BevelType::Outer => pair(full, false),
                BevelType::Emboss | BevelType::Pillow => {
                    let half = (full * 0.5).max(1.0);
                    let mut out = pair(if *bevel == BevelType::Pillow { -half } else { half }, true);
                    out.extend(pair(half, false));
                    out
                }
            };
            Some((true, layers))
        }
        _ => None,
    }
}

pub fn composite_rank(kind: EffectKind) -> u8 {
    match kind {
        EffectKind::OuterShadow => 0,
        EffectKind::OuterGlow => 1,
        EffectKind::GaussianBlur => 2,
        EffectKind::GradientOverlay => 3,
        EffectKind::ColourOverlay => 4,
        EffectKind::InnerGlow => 5,
        EffectKind::InnerShadow => 6,
        EffectKind::ThreeD | EffectKind::BevelEmboss => 7,
        EffectKind::Outline => 8,
    }
}

pub fn blurred_base(prim: &Prim, effects: &[LayerEffect]) -> Option<EffectLayer> {
    let (radius, keep) = effects.iter().filter(|effect| effect.enabled).find_map(|effect| match (&effect.kind, &effect.params) {
        (EffectKind::GaussianBlur, EffectParams::Blur { radius, preserve_alpha }) => Some((finite(*radius, 0.0).max(0.0), *preserve_alpha)),
        _ => None,
    })?;
    if radius == 0.0 {
        return None;
    }
    let object = prim_object(prim)?;
    let base = match prim {
        Prim::Fill { paint, .. } | Prim::Stroke { paint, .. } => with_paint(prim, [0.0, 0.0], paint.clone())?,
        Prim::Image { object, quad, .. } => Prim::Image { object: *object, quad: *quad, opacity: 1.0 },
        Prim::Appearance { object, inks, origin, .. } => {
            Prim::Appearance { object: *object, inks: inks.clone(), origin: *origin, opacity: 1.0, tint: None }
        }
        _ => return None,
    };
    let clip = keep.then_some(Clip::PreserveAlpha);
    Some(EffectLayer { object, prim: base, blur: radius, blend: BlendMode::Normal, alpha: prim_opacity(prim), clip, invert: false, spread: 0.0, tint: None, alpha_edge: None, image_clip: None, group_effect: None })
}

fn appearance_sources(prim: &Prim) -> Option<impl Iterator<Item = Prim> + '_> {
    let Prim::Appearance { object, inks, origin, opacity, .. } = prim else { return None; };
    if inks.iter().any(|ink| ink.blend.erases()) {
        return None;
    }
    Some(inks.iter().map(|ink| Prim::Fill {
        object: *object, contours: ink.contours.clone(), origin: *origin,
        paint: ink.paint.clone(), opacity: *opacity,
    }))
}

pub fn scene_items(scene: &Scene) -> Vec<Item> {
    scene.prims.iter().cloned().map(Item::Plain).collect()
}

pub(crate) fn stroke_effect_group(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    let owner = doc.get_object(id)?.parent_id.and_then(|parent| doc.get_object(parent))?;
    (owner.is_group() && owner.expanded_stroke_effects).then_some(owner.id)
}

/// Collect ink for each layer's effects, including the shared style of an expanded stroke.
pub fn export_effect_sources(doc: &Document, scene: &Scene) -> HashMap<ObjectId, Vec<Prim>> {
    let mut sources = HashMap::<_, Vec<_>>::new();
    for prim in &scene.prims {
        let Some(object) = prim_object(prim).and_then(|id| doc.get_object(id)) else { continue; };
        let own = object.effects.iter().any(|effect| effect.enabled);
        let parent = stroke_effect_group(doc, object.id)
            .filter(|&id| doc.get_object(id).is_some_and(|group| group.effects.iter().any(|effect| effect.enabled)));
        if !own && parent.is_none() {
            continue;
        }
        let mut prim = prim.clone();
        let opacity = match &mut prim {
            Prim::Fill { opacity, .. } | Prim::Stroke { opacity, .. } | Prim::Appearance { opacity, .. } |
            Prim::Image { opacity, .. } | Prim::Text { opacity, .. } => opacity,
            _ => continue,
        };
        // The vector exporter applies ancestor group opacity separately.
        *opacity = object.opacity;
        if own {
            sources.entry(object.id).or_default().push(prim.clone());
        }
        if let Some(parent) = parent {
            sources.entry(parent).or_default().push(prim);
        }
    }
    sources
}

fn bitmap_only_curve(doc: &Document, source: &Prim) -> bool {
    let Prim::Fill { object, paint: Paint::Bitmap(_), .. } = source else { return false; };
    doc.get_object(*object).is_some_and(|object| {
        matches!(object.kind, crate::ObjectKind::Curve(_))
            && matches!(object.fill, Paint::Bitmap(_))
            && matches!(object.stroke.paint, Paint::None)
            && !crate::commands::appearance::has_layers(object)
    })
}

/// A bitmap fill already has raster ink. Apply its blur at the original paint
/// position so the crisp bitmap cannot cover the blurred version.
pub fn export_bitmap_blurs(
    doc: &Document, sources: &HashMap<ObjectId, Vec<Prim>>,
) -> HashMap<ObjectId, ([f32; 4], Vec<EffectLayer>)> {
    let mut blurs = HashMap::<_, ([f32; 4], Vec<EffectLayer>)>::new();
    for source in sources.values().flatten().filter(|source| bitmap_only_curve(doc, source)) {
        let Some(id) = prim_object(source) else { continue; };
        let Some(object) = doc.get_object(id) else { continue; };
        let parent = stroke_effect_group(doc, id).and_then(|id| doc.get_object(id));
        let Some(layer) = blurred_base(source, &object.effects)
            .or_else(|| parent.and_then(|group| blurred_base(source, &group.effects))) else { continue; };
        let Prim::Fill { contours, .. } = source else { continue; };
        let Some([left, top, right, bottom]) = contours_bounds(contours) else { continue; };
        let pad = layer.raster_padding() + 1.0;
        let ink = [left - pad, top - pad, right + pad, bottom + pad];
        let (bounds, layers) = blurs.entry(id).or_insert(([ink[0], ink[1], ink[2] - ink[0], ink[3] - ink[1]], Vec::new()));
        if !layers.contains(&layer) {
            let x = bounds[0].min(ink[0]);
            let y = bounds[1].min(ink[1]);
            *bounds = [x, y, (bounds[0] + bounds[2]).max(ink[2]) - x, (bounds[1] + bounds[3]).max(ink[3]) - y];
            layers.push(layer);
        }
    }
    blurs
}

fn text_effect_sources(doc: &Document, prim: &Prim) -> Option<Vec<Prim>> {
    let Prim::Text { object: id, lines, opacity } = prim else { return None; };
    if crate::scene::has_live_text_warp(doc, *id) {
        return None;
    }
    let object = doc.get_object(*id)?;
    let outlines = crate::text_lines::text_outlines(doc, *id)?;
    let region = crate::clip_mask::clip_region(doc, *id);
    Some(lines.iter().zip(crate::scene::glyph_groups(lines, outlines)).filter_map(|(line, mut contours)| {
        if let Some(region) = &region {
            contours = crate::clip_mask::clip_fill_contours(&contours, region);
        }
        (!contours.is_empty()).then(|| Prim::Fill {
            object: *id, contours, origin: object.pos, paint: line.paint.clone(), opacity: *opacity,
        })
    }).collect())
}

/// Collect only the effect coverage, keeping the source drawing separate.
pub fn export_effect_layers(doc: &Document, id: ObjectId, primitives: &[&Prim]) -> (Vec<EffectLayer>, Vec<EffectLayer>) {
    let mut effects = live_effects(doc, id, &[]);
    effects.sort_by_key(|effect| composite_rank(effect.kind));
    let mut below = Vec::new();
    let mut above = Vec::new();
    let mut sources = Vec::new();
    for &prim in primitives {
        match prim {
            Prim::Fill { .. } | Prim::Stroke { .. } => sources.push(prim.clone()),
            Prim::Image { .. } if is_path_brush_image(doc, prim) => sources.push(prim.clone()),
            Prim::Appearance { .. } => {
                if let Some(rows) = appearance_sources(prim) {
                    sources.extend(rows);
                } else {
                    sources.push(prim.clone());
                }
            }
            Prim::Text { .. } => sources.extend(text_effect_sources(doc, prim).into_iter().flatten()),
            _ => {}
        }
    }
    for effect in &effects {
        for source in &sources {
            if let Some((on_top, layers)) = effect_layers(doc, source, effect) {
                if on_top { above.extend(layers); } else { below.extend(layers); }
            }
            // Raster brush and bitmap ink are blurred in their own paint order.
            if !matches!(source, Prim::Image { .. }) && !bitmap_only_curve(doc, source)
                && effect.kind == EffectKind::GaussianBlur
                && let Some(layer) = blurred_base(source, std::slice::from_ref(effect))
            {
                below.push(layer);
            }
        }
    }
    (below, above)
}

pub fn effect_items(doc: &Document, flags: &CanvasFlags, base: Vec<Item>, staged: &[StagedEffect]) -> Vec<Item> {
    if matches!(flags.view_mode, crate::ViewMode::Outline | crate::ViewMode::Wireframe) {
        return base;
    }
    let mut cache: HashMap<ObjectId, Vec<LayerEffect>> = HashMap::new();
    let (mut group_behind, mut group_above) = group_effect_items(doc, &base, staged);
    let mut first: HashMap<ObjectId, usize> = HashMap::new();
    let mut out: Vec<Item> = Vec::with_capacity(base.len());
    for (index, item) in base.into_iter().enumerate() {
        if let Some(layers) = group_behind.remove(&index) {
            out.extend(layers.into_iter().map(Item::Layer));
        }
        let prim = item.prim().clone();
        let id = prim_object(&prim).filter(|_| is_path_brush_image(doc, &prim)
            || matches!(prim, Prim::Fill { .. } | Prim::Appearance { .. } | Prim::Stroke { .. } | Prim::Text { .. }));
        let mut effects = id.map(|id| cache.entry(id).or_insert_with(|| live_effects(doc, id, staged)).clone()).unwrap_or_default();
        let group = id.and_then(|id| stroke_effect_group(doc, id));
        if let Some(group) = group {
            effects.extend(cache.entry(group).or_insert_with(|| live_effects(doc, group, staged)).iter().cloned());
        }
        effects.sort_by_key(|effect| composite_rank(effect.kind));
        let (mut below, mut above) = (Vec::new(), Vec::new());
        if !effects.is_empty() {
            let split = appearance_sources(&prim).map(|sources| sources.collect::<Vec<_>>())
                .or_else(|| text_effect_sources(doc, &prim));
            let sources = split.as_deref().unwrap_or_else(|| std::slice::from_ref(&prim));
            for effect in &effects {
                for source in sources {
                    if let Some((on_top, extra)) = effect_layers(doc, source, effect) {
                        let target = if on_top { &mut above } else { &mut below };
                        target.extend(extra.into_iter().map(Item::Layer));
                    }
                    if split.is_some() && effect.kind == EffectKind::GaussianBlur
                        && let Some(layer) = blurred_base(source, std::slice::from_ref(effect))
                    {
                        below.push(Item::Layer(layer));
                    }
                }
            }
        }
        let item = id.and_then(|_| blurred_base(&prim, &effects)).map_or(item, Item::Layer);
        let at = match group.or(id) {
            Some(id) => *first.entry(id).or_insert(out.len()),
            None => out.len(),
        };
        let added = below.len();
        if added > 0 {
            out.splice(at..at, below);
            for pos in first.values_mut() {
                if *pos > at {
                    *pos += added;
                }
            }
        }
        out.push(item);
        out.extend(above);
        if let Some(layers) = group_above.remove(&index) {
            out.extend(layers.into_iter().map(Item::Layer));
        }
    }
    out
}

pub fn over(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let sa = f32::from(s[3]) / 255.0;
        if sa <= 0.0 {
            continue;
        }
        let da = f32::from(d[3]) / 255.0;
        let out = sa + da * (1.0 - sa);
        for i in 0..3 {
            d[i] = ((f32::from(s[i]) * sa + f32::from(d[i]) * da * (1.0 - sa)) / out).round().clamp(0.0, 255.0) as u8;
        }
        d[3] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

pub fn fill_layer(prim: &Prim, bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    let len = width as usize * height as usize * 4;
    let Prim::Fill { contours, origin, paint, opacity, .. } = prim else { return vec![0; len] };
    if matches!(paint, Paint::Solid { .. })
        && let Some(mut layer) = rasterize_fill(paint, contours, *origin, bounds_edges(bounds), [width as usize, height as usize])
        && layer.len() == len
    {
        let opacity = unit(*opacity);
        for pixel in layer.chunks_exact_mut(4) {
            pixel[3] = (f32::from(pixel[3]) * opacity).round() as u8;
        }
        return layer;
    }
    let mut layer = vec![0u8; len];
    let (cw, ch) = (bounds[2] / width as f32, bounds[3] / height as f32);
    for py in 0..height {
        for px in 0..width {
            let x = bounds[0] + (px as f32 + 0.5) * cw;
            let y = bounds[1] + (py as f32 + 0.5) * ch;
            if !inside(contours, x, y) {
                continue;
            }
            let rgba = match paint {
                Paint::Solid { colour } => Some(vec![colour.r, colour.g, colour.b, colour.a]),
                _ => {
                    let cell = vec![vec![[x - cw * 0.5, y - ch * 0.5], [x + cw * 0.5, y - ch * 0.5], [x + cw * 0.5, y + ch * 0.5], [x - cw * 0.5, y + ch * 0.5]]];
                    rasterize_fill(paint, &cell, *origin, [x - cw * 0.5, y - ch * 0.5, x + cw * 0.5, y + ch * 0.5], [1, 1]).or_else(|| representative_colour(paint).map(|c| vec![c.r, c.g, c.b, c.a]))
                }
            };
            let Some(rgba) = rgba.filter(|v| v.len() >= 4) else { continue };
            let at = ((py * width + px) * 4) as usize;
            layer[at..at + 3].copy_from_slice(&rgba[..3]);
            layer[at + 3] = (f32::from(rgba[3]) * unit(*opacity)).round() as u8;
        }
    }
    layer
}

fn bounds_edges([x, y, width, height]: [f32; 4]) -> [f32; 4] {
    [x, y, x + width, y + height]
}

fn blur_layer(layer: &mut [u8], width: usize, height: usize, sigma: f32) {
    let mut visible = layer.chunks_exact(4).filter(|pixel| pixel[3] != 0);
    let Some(first) = visible.next() else { return };
    let colour = [first[0], first[1], first[2]];
    if visible.all(|pixel| pixel[..3] == colour) {
        let alpha = layer.chunks_exact(4).map(|pixel| [f32::from(pixel[3])]).collect();
        for (pixel, [a]) in layer.chunks_exact_mut(4).zip(gaussian_blur(alpha, width, height, sigma)) {
            pixel[..3].copy_from_slice(&colour);
            pixel[3] = a.round().clamp(0.0, 255.0) as u8;
        }
        return;
    }
    let premultiplied = layer.chunks_exact(4).map(|p| {
        let a = f32::from(p[3]) / 255.0;
        [f32::from(p[0]) * a, f32::from(p[1]) * a, f32::from(p[2]) * a, f32::from(p[3])]
    }).collect();
    for (pixel, value) in layer.chunks_exact_mut(4).zip(gaussian_blur(premultiplied, width, height, sigma)) {
        let alpha = value[3] / 255.0;
        for channel in 0..3 {
            pixel[channel] = if alpha > 0.0 { (value[channel] / alpha).round().clamp(0.0, 255.0) as u8 } else { 0 };
        }
        pixel[3] = value[3].round().clamp(0.0, 255.0) as u8;
    }
}

fn gaussian_blur<const CHANNELS: usize>(
    mut source: Vec<[f32; CHANNELS]>, width: usize, height: usize, sigma: f32,
) -> Vec<[f32; CHANNELS]> {
    let reach = (sigma * 3.0).ceil() as usize;
    let kernel: Vec<f32> = (-(reach as isize)..=reach as isize)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = kernel.iter().sum();
    let mut target = vec![[0.0; CHANNELS]; source.len()];
    // Transposing each pass keeps both passes reading contiguous rows.
    for (columns, rows) in [(width, height), (height, width)] {
        target.fill([0.0; CHANNELS]);
        for (y, row) in source.chunks_exact(columns).enumerate() {
            let has_ink = |sample: &[f32; CHANNELS]| sample.iter().any(|value| *value != 0.0);
            let Some(first) = row.iter().position(has_ink) else { continue };
            let last = row.iter().rposition(has_ink).unwrap_or(first);
            let start = first.saturating_sub(reach);
            let end = last.saturating_add(reach).min(columns - 1);
            let mut repeat_until = start;
            let mut repeated = [0.0; CHANNELS];
            for x in start..=end {
                if x < repeat_until {
                    target[x * rows + y] = repeated;
                    continue;
                }
                let mut sum = [0.0; CHANNELS];
                // Skip zero samples without changing the weights or edge clamping.
                let first_tap = if first == 0 { 0 } else { (first + reach).saturating_sub(x) };
                let last_tap = if last == columns - 1 { kernel.len() - 1 }
                    else { (last + reach - x).min(kernel.len() - 1) };
                let begin = x + first_tap;
                let end = x + last_tap;
                let weights = &kernel[first_tap..=last_tap];
                if begin >= reach && end - reach < columns {
                    let samples = &row[begin - reach..=end - reach];
                    for (sample, weight) in samples.iter().zip(weights) {
                        for channel in 0..CHANNELS {
                            sum[channel] += sample[channel] * weight;
                        }
                    }
                    // Every full window inside a constant alpha span has the same
                    // weighted sum. Reuse that sum without changing its rounding.
                    if CHANNELS == 1 && reach >= 4 && weights.len() == kernel.len()
                        && samples.first() == samples.last()
                        && samples.iter().all(|sample| *sample == samples[0])
                    {
                        let extension = row[end - reach + 1..].iter().take_while(|sample| **sample == samples[0]).count();
                        repeat_until = x + extension + 1;
                    }
                } else {
                    for (tap, weight) in (first_tap..=last_tap).zip(weights) {
                        let sample = row[(x + tap).saturating_sub(reach).min(columns - 1)];
                        for channel in 0..CHANNELS {
                            sum[channel] += sample[channel] * weight;
                        }
                    }
                }
                repeated = sum.map(|value| value / total);
                target[x * rows + y] = repeated;
            }
        }
        std::mem::swap(&mut source, &mut target);
    }
    source
}

fn shape(doc: &Document, prim: &Prim, bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    match prim {
        Prim::Fill { paint, .. } if !matches!(paint, Paint::Bitmap(_)) => fill_layer(prim, bounds, width, height),
        prim => crate::blend_composite::rasterize_effect_source(
            doc, &Scene { version: 0, prims: vec![prim.clone()], quads: Vec::new() }, bounds, width, height,
        ),
    }
}

fn effect_shape(
    doc: &Document, item: &EffectLayer, prim: &Prim, bounds: [f32; 4], [width, height]: [u32; 2],
) -> Vec<u8> {
    let mut pixels = shape(doc, prim, bounds, width, height);
    if let (Some(clip), Prim::Image { quad, .. }) = (&item.image_clip, prim) {
        let [dx, dy] = [quad[0][0] - clip.origin[0], quad[0][1] - clip.origin[1]];
        let contours = clip.contours.iter().map(|ring| ring.iter().map(|&[x, y]| [x + dx, y + dy]).collect()).collect();
        let coverage = Prim::Fill {
            object: item.object, contours, origin: [0.0, 0.0],
            paint: Paint::Solid { colour: Colour::new(255, 255, 255, 255) }, opacity: 1.0,
        };
        let mask = fill_layer(&coverage, bounds, width, height);
        for (pixel, alpha) in pixels.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
            pixel[3] = ((u16::from(pixel[3]) * u16::from(alpha[3]) + 127) / 255) as u8;
        }
    }
    pixels
}

fn paint_of(prim: &Prim) -> Option<&Paint> {
    match prim {
        Prim::Fill { paint, .. } | Prim::Stroke { paint, .. } => Some(paint),
        _ => None,
    }
}

fn tint_coverage(pixels: &mut [u8], tint: &Paint, prim: &Prim, invert: bool, bounds: [f32; 4], [width, height]: [u32; 2]) {
    let origin = match prim {
        Prim::Image { quad, .. } => quad[0],
        Prim::Fill { origin, .. } => *origin,
        _ => [0.0, 0.0],
    };
    tint_coverage_at(pixels, tint, origin, invert, bounds, [width, height]);
}

fn tint_coverage_at(pixels: &mut [u8], tint: &Paint, origin: [f32; 2], invert: bool, bounds: [f32; 4], [width, height]: [u32; 2]) {
    let coverage = |alpha| if invert { 255 - alpha } else { alpha };
    let masked = |alpha, opacity| ((u16::from(coverage(alpha)) * u16::from(opacity) + 127) / 255) as u8;
    if let Paint::Solid { colour } = tint {
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = masked(pixel[3], colour.a);
            pixel.copy_from_slice(&[colour.r, colour.g, colour.b, alpha]);
        }
        return;
    }
    let [left, top, right, bottom] = bounds_edges(bounds);
    let contours = [vec![[left, top], [right, top], [right, bottom], [left, bottom]]];
    if let Some(paint) = rasterize_fill(tint, &contours, origin, bounds_edges(bounds), [width as usize, height as usize]) {
        for (pixel, color) in pixels.chunks_exact_mut(4).zip(paint.chunks_exact(4)) {
            let alpha = masked(pixel[3], color[3]);
            pixel.copy_from_slice(&[color[0], color[1], color[2], alpha]);
        }
    } else {
        pixels.fill(0);
    }
}

mod alpha_edge;
use alpha_edge::alpha_outline;

pub(crate) fn erode_alpha(alpha: &[u8], size: [usize; 2], radius: usize) -> Vec<u8> {
    if size.contains(&0) {
        return Vec::new();
    }
    alpha_edge::octagon(alpha, size, radius, false)
}

pub fn draw_layer(doc: &Document, pixels: &mut [u8], item: &EffectLayer, bounds: [f32; 4], width: u32, height: u32) {
    if width == 0 || height == 0 || bounds[2] <= 0.0 || bounds[3] <= 0.0 {
        return;
    }
    if let Some((scene, effect, _)) = &item.group_effect {
        if let Some(source) = group_effect_pixels(doc, item.object, scene, effect, bounds, [width, height]) {
            for (index, rgba) in source.chunks_exact(4).enumerate() {
                blend_pixel(pixels, index * 4, [rgba[0], rgba[1], rgba[2], rgba[3]], item.alpha, item.blend);
            }
        }
        return;
    }
    let scale = width as f32 / bounds[2];
    let sigma = item.blur * 0.5 * scale;
    let blur_pad = if sigma > 0.05 { (sigma * 3.0).ceil() as u32 + 1 } else { 0 };
    let outline_radius = (item.outline_radius() * scale).ceil().min(width.max(height) as f32) as u32;
    let pad = blur_pad.max(outline_radius);
    let unit_pad = pad as f32 / scale;
    let (w, h) = (width + pad * 2, height + pad * 2);
    let wide = [bounds[0] - unit_pad, bounds[1] - unit_pad, bounds[2] + unit_pad * 2.0, bounds[3] + unit_pad * 2.0];
    let mut buffer = if item.invert
        && let Prim::Appearance { object, inks, origin, opacity, .. } = &item.prim
    {
        let untinted = Prim::Appearance {
            object: *object, inks: inks.clone(), origin: *origin, opacity: *opacity, tint: None,
        };
        effect_shape(doc, item, &untinted, wide, [w, h])
    } else {
        effect_shape(doc, item, &item.prim, wide, [w, h])
    };
    let original_alpha = matches!(item.clip, Some(Clip::PreserveAlpha))
        .then(|| buffer.chunks_exact(4).map(|pixel| pixel[3]).collect::<Vec<_>>());
    match &item.alpha_edge {
        Some(AlphaEdge::Outline { align, .. }) => {
            alpha_outline(&mut buffer, [w as usize, h as usize], outline_radius as usize, *align);
        }
        Some(AlphaEdge::Bevel { reference, inner }) => {
            if blur_pad > 0 {
                blur_layer(&mut buffer, w as usize, h as usize, sigma);
            }
            let reference = effect_shape(doc, item, reference, wide, [w, h]);
            for (pixel, original) in buffer.chunks_exact_mut(4).zip(reference.chunks_exact(4)) {
                let shifted = pixel[3];
                pixel[3] = if *inner { original[3].saturating_sub(shifted) } else { shifted.saturating_sub(original[3]) };
            }
        }
        None => {}
    }
    if let Some(tint) = &item.tint {
        tint_coverage(&mut buffer, tint, &item.prim, item.invert, wide, [w, h]);
    } else if item.invert {
        let colour = match &item.prim {
            Prim::Appearance { tint: Some(colour), .. } => *colour,
            prim => paint_of(prim).and_then(representative_colour).unwrap_or(Colour::new(0, 0, 0, 255)),
        };
        for p in buffer.chunks_exact_mut(4) {
            let a = 255 - p[3];
            p.copy_from_slice(&[colour.r, colour.g, colour.b, ((f32::from(a) * f32::from(colour.a)) / 255.0).round() as u8]);
        }
    }
    if blur_pad > 0 && item.alpha_edge.is_none() {
        blur_layer(&mut buffer, w as usize, h as usize, sigma);
    }
    let clip_mask = item.clip.as_ref().and_then(|clip| {
        let contours = match clip {
            Clip::Inside(contours) | Clip::Outside(contours) => contours,
            Clip::Alpha { source, .. } => return Some(effect_shape(doc, item, source, bounds, [width, height])),
            Clip::PreserveAlpha => return None,
        };
        let mask = Paint::Solid { colour: Colour::new(255, 255, 255, 255) };
        rasterize_fill(&mask, contours, [0.0, 0.0], bounds_edges(bounds), [width as usize, height as usize])
            .filter(|data| data.len() == pixels.len())
    });
    let (cw, ch) = (bounds[2] / width as f32, bounds[3] / height as f32);
    for py in 0..height {
        for px in 0..width {
            let src = (((py + pad) * w + px + pad) * 4) as usize;
            let mut a = f32::from(original_alpha.as_ref().map_or(buffer[src + 3], |alpha| alpha[src / 4]));
            if a <= 0.0 {
                continue;
            }
            if item.spread > 0.0 {
                a = (a / (1.0 - item.spread)).min(255.0);
            }
            if let Some(clip) = &item.clip {
                let point_alpha = || clip_mask.as_ref().map(|mask| mask[((py * width + px) * 4 + 3) as usize]);
                match clip {
                    Clip::Inside(contours) | Clip::Outside(contours) => {
                        let covered = point_alpha().map_or_else(|| {
                            let x = bounds[0] + (px as f32 + 0.5) * cw;
                            let y = bounds[1] + (py as f32 + 0.5) * ch;
                            inside(contours, x, y)
                        }, |alpha| alpha > 0);
                        if covered != matches!(clip, Clip::Inside(_)) {
                            continue;
                        }
                    }
                    Clip::Alpha { inside, .. } => {
                        let covered = f32::from(point_alpha().unwrap_or(0)) / 255.0;
                        a *= if *inside { covered } else { 1.0 - covered };
                    }
                    Clip::PreserveAlpha => {}
                }
            }
            let colour = [buffer[src], buffer[src + 1], buffer[src + 2], a.round() as u8];
            blend_pixel(pixels, ((py * width + px) * 4) as usize, colour, item.alpha, item.blend.clone());
        }
    }
}

pub fn rasterize_items(doc: &Document, scene: &Scene, items: &[Item], bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    rasterize_items_scaled(doc, scene, items, bounds, width, height, 1.0)
}

fn upsample(src: &[u8], sw: u32, sh: u32, width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let fy = ((y as f32 + 0.5) * sh as f32 / height as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(sh as usize - 1);
        for x in 0..width as usize {
            let fx = ((x as f32 + 0.5) * sw as f32 / width as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(sw as usize - 1);
            for c in 0..4 {
                let at = |xx: usize, yy: usize| f32::from(src[(yy * sw as usize + xx) * 4 + c]);
                let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                out[(y * width as usize + x) * 4 + c] = (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

fn draw_effect_region(doc: &Document, pixels: &mut [u8], layer: &EffectLayer, bounds: [f32; 4], size: [u32; 2]) {
    let [width, height] = size;
    let ink = if let Some((_, _, [x, y, w, h])) = layer.group_effect.as_ref() {
        Some([*x, *y, x + w, y + h])
    } else if !layer.invert && layer.alpha_edge.is_none() {
        match &layer.prim {
            Prim::Fill { contours, .. } => contours_bounds(contours),
            Prim::Image { quad, .. } => {
                let x = quad.map(|p| p[0]);
                let y = quad.map(|p| p[1]);
                Some([
                    x.into_iter().fold(f32::INFINITY, f32::min),
                    y.into_iter().fold(f32::INFINITY, f32::min),
                    x.into_iter().fold(f32::NEG_INFINITY, f32::max),
                    y.into_iter().fold(f32::NEG_INFINITY, f32::max),
                ])
            }
            _ => None,
        }
    } else {
        None
    };
    let Some([left, top, right, bottom]) = ink.filter(|ink| ink.iter().all(|v| v.is_finite())) else {
        draw_layer(doc, pixels, layer, bounds, width, height);
        return;
    };

    // Crop on the original sample grid. draw_layer still pads this region for
    // the convolution, so a tile edge never truncates visible blur.
    let [x, y, w, h] = bounds;
    let dx = w / width as f32;
    let dy = h / height as f32;
    let pad = layer.raster_padding() + 2.0 * dx.max(dy);
    let x0 = (((left - pad - x) / dx).floor() as u32).min(width);
    let y0 = (((top - pad - y) / dy).floor() as u32).min(height);
    let x1 = (((right + pad - x) / dx).ceil() as u32).min(width);
    let y1 = (((bottom + pad - y) / dy).ceil() as u32).min(height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    if x0 == 0 && y0 == 0 && x1 == width && y1 == height {
        draw_layer(doc, pixels, layer, bounds, width, height);
        return;
    }

    let [w, h] = [x1 - x0, y1 - y0];
    let region = [x + x0 as f32 * dx, y + y0 as f32 * dy, w as f32 * dx, h as f32 * dy];
    let stride = width as usize * 4;
    let rows = y0 as usize..y1 as usize;
    let columns = x0 as usize * 4..x1 as usize * 4;
    let mut tile = Vec::with_capacity(w as usize * h as usize * 4);
    for row in pixels.chunks_exact(stride).take(rows.end).skip(rows.start) {
        tile.extend_from_slice(&row[columns.clone()]);
    }
    draw_layer(doc, &mut tile, layer, region, w, h);
    for (target, source) in pixels.chunks_exact_mut(stride).take(rows.end).skip(rows.start).zip(tile.chunks_exact(w as usize * 4)) {
        target[columns.clone()].copy_from_slice(source);
    }
}

fn effect_pass(doc: &Document, pixels: &mut [u8], item: &Item, bounds: [f32; 4], width: u32, height: u32, scale: f32) {
    let scale = finite(scale, 1.0).clamp(0.01, 1.0);
    let (sw, sh) = (((width as f32 * scale).round() as u32).max(1), ((height as f32 * scale).round() as u32).max(1));
    if sw >= width && sh >= height {
        match item {
            Item::Exact(prim) => over(pixels, &fill_layer(prim, bounds, width, height)),
            Item::Layer(effect) => draw_effect_region(doc, pixels, effect, bounds, [width, height]),
            Item::Plain(_) => {}
        }
        return;
    }
    let mut layer = vec![0u8; sw as usize * sh as usize * 4];
    let mode = match item {
        Item::Exact(prim) => {
            over(&mut layer, &fill_layer(prim, bounds, sw, sh));
            crate::blend::BlendMode::default()
        }
        Item::Layer(effect) => {
            draw_layer(doc, &mut layer, effect, bounds, sw, sh);
            effect.blend
        }
        Item::Plain(_) => return,
    };
    let up = upsample(&layer, sw, sh, width, height);
    for (offset, px) in up.chunks_exact(4).enumerate() {
        crate::blend::blend_pixel(pixels, offset * 4, [px[0], px[1], px[2], px[3]], 1.0, mode);
    }
}

pub fn rasterize_items_scaled(doc: &Document, scene: &Scene, items: &[Item], bounds: [f32; 4], width: u32, height: u32, effect_scale: f32) -> Vec<u8> {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    if width == 0 || height == 0 || !(bounds[2] > 0.0) || !(bounds[3] > 0.0) || !bounds.iter().all(|v| v.is_finite()) {
        return pixels;
    }
    if items.iter().all(|item| matches!(item, Item::Plain(_))) {
        let all = Scene { version: scene.version, prims: items.iter().map(|item| item.prim().clone()).collect(), quads: scene.quads.clone() };
        return rasterize_scene(doc, &all, bounds, width, height, None);
    }
    let grid = groups::RasterGrid { bounds, size: [width, height], view: None };
    let effect_groups: Vec<_> = items.iter().filter_map(|item| match item {
        Item::Layer(layer) if layer.group_effect.is_some() => Some(layer.object),
        _ => None,
    }).collect();
    if !groups::render(doc, items, Item::object, &effect_groups, grid, &mut pixels, |mut run, opacity, target| {
        for item in &mut run {
            match item {
                Item::Plain(prim) | Item::Exact(prim) => groups::normalize_prim_opacity(prim, opacity),
                Item::Layer(layer) => groups::normalize_opacity(&mut layer.alpha, opacity),
            }
        }
        draw_items_over(doc, scene, &run, bounds, [width, height], effect_scale, target);
    }) {
        draw_items_over(doc, scene, items, bounds, [width, height], effect_scale, &mut pixels);
    }
    pixels
}

fn draw_items_over(doc: &Document, scene: &Scene, items: &[Item], bounds: [f32; 4], [width, height]: [u32; 2], effect_scale: f32, pixels: &mut [u8]) {
    let mut segment = Scene { version: scene.version, prims: Vec::new(), quads: scene.quads.clone() };
    let mut open_clips = Vec::new();
    let mut has_content = false;
    for item in items {
        if let Item::Plain(prim) = item {
            match prim {
                Prim::Clip { rect: Some(_), .. } => open_clips.push(prim.clone()),
                Prim::Clip { rect: None, .. } => { open_clips.pop(); }
                Prim::Page { .. } | Prim::Handle { .. } | Prim::Mark { .. } => {}
                _ => has_content = true,
            }
            segment.prims.push(prim.clone());
            continue;
        }
        if has_content {
            crate::blend_composite::rasterize_over(doc, &segment, bounds, [width, height], pixels);
        }
        // An effect splits the draw order, but does not close the enclosing clips.
        segment.prims.clone_from(&open_clips);
        has_content = false;
        effect_pass(doc, pixels, item, bounds, width, height, effect_scale);
    }
    if has_content {
        crate::blend_composite::rasterize_over(doc, &segment, bounds, [width, height], pixels);
    }
}

fn upto(items: Vec<Item>, object: ObjectId) -> Vec<Item> {
    let mut items = items;
    if let Some(last) = items.iter().rposition(|item| item.object() == Some(object)) {
        items.truncate(last + 1);
    }
    items
}

fn clipped_scene(doc: &Document, flags: &CanvasFlags) -> Scene {
    // C09D already runs clip_mask::apply inside build_scene
    build_scene(doc, flags)
}

pub fn render_region(doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect], bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    render_region_scaled(doc, flags, staged, bounds, width, height, 1.0)
}

pub fn render_region_scaled(doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect], bounds: [f32; 4], width: u32, height: u32, effect_scale: f32) -> Vec<u8> {
    let scene = clipped_scene(doc, flags);
    render_scene_region(doc, flags, staged, &scene, bounds, [width, height], effect_scale)
}

pub fn render_scene_region(doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect], scene: &Scene, bounds: [f32; 4], size: [u32; 2], effect_scale: f32) -> Vec<u8> {
    let items = effect_items(doc, flags, scene_items(scene), staged);
    rasterize_items_scaled(doc, scene, &items, bounds, size[0], size[1], effect_scale)
}

pub fn render_object_alone(doc: &Document, flags: &CanvasFlags, object: ObjectId, bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    render_isolated_object(doc, flags, &[], object, bounds, [width, height])
}

pub fn render_isolated_object(doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect], object: ObjectId, bounds: [f32; 4], size: [u32; 2]) -> Vec<u8> {
    let mut scene = clipped_scene(doc, flags);
    scene.prims.retain(|prim| prim_object(prim) == Some(object));
    let items = effect_items(doc, flags, scene_items(&scene), staged);
    rasterize_items(doc, &scene, &items, bounds, size[0], size[1])
}

pub fn render_object_region(doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect], object: ObjectId, bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    let scene = clipped_scene(doc, flags);
    let items = upto(effect_items(doc, flags, scene_items(&scene), staged), object);
    rasterize_items(doc, &scene, &items, bounds, width, height)
}

pub fn object_effect_bounds(doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect], object: ObjectId) -> Option<[f32; 4]> {
    let scene = build_scene(doc, flags);
    let members = doc.get_object(object).filter(|source| group_effect_target(source)).map(|_| doc.subtree_ids(object));
    effect_bounds(doc, staged, object, scene.prims.iter().filter(|prim| prim_object(prim).is_some_and(|id| {
        members.as_ref().map_or(id == object, |members| members.contains(&id))
    })))
}

pub fn effect_bounds<'a>(doc: &Document, staged: &[StagedEffect], object: ObjectId, primitives: impl IntoIterator<Item = &'a Prim>) -> Option<[f32; 4]> {
    let group = doc.get_object(object).is_some_and(|object| object.is_group());
    let contours: Contours = primitives.into_iter().flat_map(|prim| match prim {
            Prim::Fill { contours, .. } => contours.clone(),
            Prim::Appearance { inks, .. } => inks.iter().flat_map(|ink| ink.contours.iter().cloned()).collect(),
            Prim::Stroke { points, width, .. } => {
                let pad = finite(*width, 0.0).abs() * 0.5;
                points.iter().map(|p| vec![[p[0] - pad, p[1] - pad], [p[0] + pad, p[1] + pad]]).collect()
            }
            Prim::Image { quad, .. } if group || is_path_brush_image(doc, prim) => vec![quad.to_vec()],
            Prim::Text { object, .. } if !crate::scene::has_live_text_warp(doc, *object) => crate::text_lines::text_outlines(doc, *object).unwrap_or_default(),
            _ => Vec::new(),
        })
        .collect();
    let [x0, y0, x1, y1] = contours_bounds(&contours)?;
    let margin = effect_margin(doc, object, staged);
    Some([x0 - margin, y0 - margin, x1 - x0 + margin * 2.0, y1 - y0 + margin * 2.0])
}

pub fn group_effect_target(object: &crate::Object) -> bool {
    object.is_group() && !object.expanded_stroke_effects
}

pub fn has_group_effect(object: &crate::Object) -> bool {
    group_effect_target(object) && object.effects.iter().any(group_effect_enabled)
}

pub fn group_effect_enabled(effect: &LayerEffect) -> bool {
    effect.enabled && matches!(effect.kind, EffectKind::OuterShadow | EffectKind::InnerShadow
        | EffectKind::OuterGlow | EffectKind::InnerGlow | EffectKind::Outline)
}

pub fn group_effect_on_top(effect: &LayerEffect) -> bool {
    matches!(effect.kind, EffectKind::InnerShadow | EffectKind::InnerGlow)
        || matches!(effect.params, EffectParams::Outline { align: OutlineAlign::Centre | OutlineAlign::Inside, .. })
}

pub fn group_effect_padding(effect: &LayerEffect) -> f32 {
    match effect.params {
        EffectParams::Shadow { radius, .. } | EffectParams::Glow { radius, .. } => finite(radius, 0.0).max(0.0) * 1.5,
        EffectParams::Outline { width, align, .. } => finite(width, 0.0).max(0.0)
            * if align == OutlineAlign::Centre { 0.5 } else { 1.0 },
        _ => 0.0,
    }
}

pub fn group_effect_region([x, y, w, h]: [f32; 4], stack_margin: f32, effect: &LayerEffect) -> [f32; 4] {
    let reach = match effect.params {
        EffectParams::Outline { align: OutlineAlign::Inside, .. } => 0.0,
        EffectParams::Outline { .. } => group_effect_padding(effect) + 1.0,
        _ => reach(effect),
    };
    let inset = (stack_margin - reach).max(0.0);
    [x + inset, y + inset, w - 2.0 * inset, h - 2.0 * inset]
}

pub fn group_effect_scene<'a>(doc: &Document, group: ObjectId, prims: impl Iterator<Item = &'a Prim>) -> Scene {
    let opacity = crate::scene::effective_opacity(doc, group);
    let prims = prims.cloned().map(|mut prim| {
        groups::normalize_prim_opacity(&mut prim, opacity);
        prim
    }).collect();
    Scene { version: doc.version, prims, quads: Vec::new() }
}

type GroupEffectItems = HashMap<usize, Vec<EffectLayer>>;

fn group_effect_items(doc: &Document, base: &[Item], staged: &[StagedEffect]) -> (GroupEffectItems, GroupEffectItems) {
    let mut behind = GroupEffectItems::new();
    let mut above = GroupEffectItems::new();
    for group in doc.objects.iter().filter(|object| group_effect_target(object)) {
        let mut effects: Vec<_> = live_effects(doc, group.id, staged).into_iter().filter(group_effect_enabled).collect();
        let opacity = crate::scene::effective_opacity(doc, group.id);
        if effects.is_empty() || opacity <= 0.0 {
            continue;
        }
        let members = doc.subtree_ids(group.id);
        let prims = || base.iter().map(Item::prim).filter(|prim| {
            prim_object(prim).is_some_and(|id| id != group.id && members.contains(&id))
        });
        let Some(bounds) = effect_bounds(doc, staged, group.id, prims()) else { continue };
        let belongs = |item: &Item| item.object().is_some_and(|id| members.contains(&id));
        let Some(first) = base.iter().position(belongs) else { continue };
        let last = base.iter().rposition(belongs).unwrap_or(first);
        let scene = Arc::new(group_effect_scene(doc, group.id, prims()));
        let margin = effect_margin(doc, group.id, staged);
        effects.sort_by_key(|effect| composite_rank(effect.kind));
        let mut inner = Vec::new();
        for effect in effects {
            let radius = match effect.params {
                EffectParams::Shadow { radius, .. } | EffectParams::Glow { radius, .. } => radius,
                EffectParams::Outline { .. } => 0.0,
                _ => continue,
            };
            let bounds = group_effect_region(bounds, margin, &effect);
            let on_top = group_effect_on_top(&effect);
            let layer = EffectLayer {
                object: group.id, prim: Prim::Mark { object: group.id, rect: bounds, role: String::new() },
                blur: radius, blend: effect.blend, alpha: opacity, clip: None, invert: false, spread: 0.0,
                tint: None, alpha_edge: None, image_clip: None, group_effect: Some((scene.clone(), effect, bounds)),
            };
            if on_top {
                inner.push(layer);
            } else {
                behind.entry(first).or_default().push(layer);
            }
        }
        above.entry(last).or_default().splice(0..0, inner);
    }
    (behind, above)
}

pub fn group_effect_pixels(
    doc: &Document, group: ObjectId, scene: &Scene, effect: &LayerEffect, bounds: [f32; 4], [w, h]: [u32; 2],
) -> Option<Vec<u8>> {
    // The enclosing group composites its own opacity, mask, and blend separately.
    let mut source = doc.clone();
    let root = source.find_object_mut(group.0)?;
    let origin = root.pos;
    root.parent_id = None;
    root.opacity = 1.0;
    root.blend_mode = BlendMode::PassThrough;
    root.transparency = Default::default();
    root.blend_ranges = None;
    root.tone_blend = None;
    source.layer_masks.retain(|mask| mask.object != group);
    if let EffectParams::Outline { align, fill, .. } = &effect.params {
        return group_outline_pixels(&source, scene, effect, *align, fill, origin, bounds, [w, h]);
    }
    let inside = group_effect_on_top(effect);
    let (radius, [dx, dy], intensity, colour, invert, clip) = match &effect.params {
        EffectParams::Shadow { radius, offset: distance, angle, intensity, colour, knock_out } =>
            (*radius, offset(*angle, *distance), *intensity, colour, inside, inside || *knock_out),
        EffectParams::Glow { radius, intensity, colour, from_centre } =>
            (*radius, [0.0, 0.0], *intensity, colour, inside && !from_centre, true),
        _ => return None,
    };
    let [x, y, width, height] = bounds;
    let sigma = finite(radius, 0.0).max(0.0) * 0.5 * w as f32 / width;
    let pad = if sigma > 0.05 { (sigma * 3.0).ceil() as u32 + 1 } else { 0 };
    let pad_x = width * pad as f32 / w as f32;
    let pad_y = height * pad as f32 / h as f32;
    let [wide_w, wide_h] = [w + 2 * pad, h + 2 * pad];
    let shifted = [x - dx - pad_x, y - dy - pad_y, width + 2.0 * pad_x, height + 2.0 * pad_y];
    let mut shadow = rasterize_scene(&source, scene, shifted, wide_w, wide_h, None);
    for pixel in shadow.chunks_exact_mut(4) {
        pixel[..3].copy_from_slice(&[colour.r, colour.g, colour.b]);
    }
    if sigma > 0.05 {
        blur_layer(&mut shadow, wide_w as usize, wide_h as usize, sigma);
    }
    let original = clip.then(|| rasterize_scene(&source, scene, bounds, w, h, None));
    let opacity = unit(effect.opacity) * f32::from(colour.a) / 255.0;
    let spread = 1.0 - unit(intensity).min(0.99);
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    let stride = wide_w as usize * 4;
    let start = pad as usize * 4;
    let rows = shadow.chunks_exact(stride).skip(pad as usize).take(h as usize);
    for (index, pixel) in rows.flat_map(|row| row[start..start + w as usize * 4].chunks_exact(4)).enumerate() {
        let coverage = original.as_ref().map_or(1.0, |pixels| {
            let alpha = f32::from(pixels[index * 4 + 3]) / 255.0;
            if inside { alpha } else { 1.0 - alpha }
        });
        let shape = if invert { 255 - pixel[3] } else { pixel[3] };
        let alpha = (f32::from(shape) / spread).min(255.0);
        pixels.extend_from_slice(&[colour.r, colour.g, colour.b, (alpha * opacity * coverage).round() as u8]);
    }
    Some(pixels)
}

fn group_outline_pixels(
    doc: &Document, scene: &Scene, effect: &LayerEffect, align: OutlineAlign, paint: &Paint,
    origin: [f32; 2], [x, y, width, height]: [f32; 4], [w, h]: [u32; 2],
) -> Option<Vec<u8>> {
    let radius = (group_effect_padding(effect) * w as f32 / width).ceil() as u32;
    if radius == 0 {
        return None;
    }
    let pad = radius + 1;
    let pad_x = width * pad as f32 / w as f32;
    let pad_y = height * pad as f32 / h as f32;
    let bounds = [x - pad_x, y - pad_y, width + 2.0 * pad_x, height + 2.0 * pad_y];
    let [wide_w, wide_h] = [w + 2 * pad, h + 2 * pad];
    let mut buffer = rasterize_scene(doc, scene, bounds, wide_w, wide_h, None);
    alpha_outline(&mut buffer, [wide_w as usize, wide_h as usize], radius as usize, align);
    let start = pad as usize * 4;
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for row in buffer.chunks_exact(wide_w as usize * 4).skip(pad as usize).take(h as usize) {
        pixels.extend_from_slice(&row[start..start + w as usize * 4]);
    }
    tint_coverage_at(&mut pixels, paint, origin, false, [x, y, width, height], [w, h]);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = (f32::from(pixel[3]) * unit(effect.opacity)).round() as u8;
    }
    Some(pixels)
}

pub type CacheKey = (ObjectId, u64, u64, [u32; 2], [u32; 4]);

#[derive(Debug, Default)]
pub struct EffectCache {
    entries: HashMap<CacheKey, Vec<u8>>,
    pub hits: u64,
    pub misses: u64,
}

impl EffectCache {
    pub fn key(doc: &Document, staged: &[StagedEffect], object: ObjectId, bounds: [f32; 4], size: [u32; 2]) -> CacheKey {
        (object, doc.version, effects_key(doc, object, staged), size, bounds.map(f32::to_bits))
    }

    pub fn object_region(&mut self, doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect], object: ObjectId, bounds: [f32; 4], size: [u32; 2]) -> &[u8] {
        let key = Self::key(doc, staged, object, bounds, size);
        let version = doc.version;
        self.entries.retain(|k, _| k.1 == version);
        if self.entries.contains_key(&key) {
            self.hits += 1;
        } else {
            self.misses += 1;
            let pixels = render_object_region(doc, flags, staged, object, bounds, size[0], size[1]);
            self.entries.insert(key, pixels);
        }
        self.entries.get(&key).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
