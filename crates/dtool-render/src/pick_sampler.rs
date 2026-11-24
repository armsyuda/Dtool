use dtool_core::commands::colour_pick::{average, PickMode};
use dtool_core::curve::Contour;
use dtool_core::object::ObjectKind;
use dtool_core::paint::{Colour, Paint};
use dtool_core::scene::{Prim, Scene};
use dtool_core::text_lines::{story_paint, text_outline_curve};
use dtool_core::{CanvasFlags, Document, ObjectId};
use dtool_core::commands::layer_style::StagedEffect;
use dtool_core::layer_style::raster::{effect_items, prim_object, rasterize_items, Item};
use std::collections::HashSet;

fn world(contour: &Contour, pos: [f32; 2]) -> Vec<[f32; 2]> {
    contour.points.iter().map(|p| [p.pos[0] as f32 + pos[0], p.pos[1] as f32 + pos[1]]).collect()
}

fn text_paint(doc: &Document, object: ObjectId, fallback: Option<&Paint>) -> Paint {
    let story = doc.get_object(object).and_then(|item| match &item.kind {
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => Some(*story),
        ObjectKind::PathText(data) => Some(data.story),
        _ => None,
    });
    fallback
        .cloned()
        .or_else(|| story.and_then(|id| doc.stories.iter().find(|s| s.id == id)).map(story_paint))
        .unwrap_or(Paint::Solid { colour: Colour::new(0, 0, 0, 255) })
}

fn glyph_fill(doc: &Document, object: ObjectId, fallback: Option<&Paint>, opacity: f32) -> Option<Prim> {
    let (pos, _, curve) = text_outline_curve(doc, object).ok()?;
    let contours: Vec<Vec<[f32; 2]>> = std::iter::once(&curve.contour).chain(curve.holes.iter()).map(|c| world(c, pos)).filter(|c| c.len() >= 3).collect();
    if contours.is_empty() {
        return None;
    }
    Some(Prim::Fill { object, contours, origin: pos, paint: text_paint(doc, object, fallback), opacity })
}

fn glyph_scene_marked(doc: &Document, flags: &CanvasFlags) -> (Scene, HashSet<usize>) {
    let mut scene = crate::scene::build_scene(doc, flags);
    let mut done: HashSet<ObjectId> = HashSet::new();
    for prim in scene.prims.iter_mut() {
        let Prim::Text { object, lines, opacity } = prim else { continue };
        let id = *object;
        if dtool_core::scene::has_live_text_warp(doc, id) {
            done.insert(id);
            continue;
        }
        if let Some(fill) = glyph_fill(doc, id, lines.first().map(|line| &line.paint), *opacity) {
            *prim = fill;
            done.insert(id);
        }
    }
    if !matches!(flags.view_mode, dtool_core::ViewMode::Outline | dtool_core::ViewMode::Wireframe) {
        for object in &doc.objects {
            if done.contains(&object.id) || !matches!(object.kind, ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_)) {
                continue;
            }
            let Some(fill) = glyph_fill(doc, object.id, None, object.opacity.clamp(0.0, 1.0)) else { continue };
            let at = scene.prims.iter().rposition(|prim| prim_object(prim) == Some(object.id)).map(|i| i + 1).unwrap_or(scene.prims.len());
            scene.prims.insert(at, fill);
            done.insert(object.id);
        }
    }
    let marked = scene
        .prims
        .iter()
        .enumerate()
        .filter(|(_, prim)| matches!(prim, Prim::Fill { object, .. } if done.contains(object)))
        .map(|(i, _)| i)
        .collect();
    (scene, marked)
}

pub fn glyph_scene(doc: &Document, flags: &CanvasFlags) -> Scene {
    glyph_scene_marked(doc, flags).0
}

pub use dtool_core::layer_style::raster::live_effects;

fn glyph_items(doc: &Document, flags: &CanvasFlags) -> (Scene, Vec<Item>) {
    let (scene, marked) = glyph_scene_marked(doc, flags);
    let items = scene.prims.iter().enumerate().map(|(i, p)| if marked.contains(&i) { Item::Exact(p.clone()) } else { Item::Plain(p.clone()) }).collect();
    (scene, items)
}

pub fn effect_scene(doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect]) -> Scene {
    let (scene, items) = glyph_items(doc, flags);
    let prims = effect_items(doc, flags, items, staged).iter().map(|item| item.prim().clone()).collect();
    Scene { version: scene.version, prims, quads: scene.quads }
}

pub fn rasterize_with_glyphs(doc: &Document, flags: &CanvasFlags, bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    let (scene, items) = glyph_items(doc, flags);
    rasterize_items(doc, &scene, &items, bounds, width, height)
}

pub fn rasterize_with_effects(doc: &Document, flags: &CanvasFlags, staged: &[StagedEffect], bounds: [f32; 4], width: u32, height: u32) -> Vec<u8> {
    let (scene, items) = glyph_items(doc, flags);
    let items = effect_items(doc, flags, items, staged);
    rasterize_items(doc, &scene, &items, bounds, width, height)
}

fn layer_only(doc: &Document, layer: ObjectId) -> Option<Document> {
    doc.get_object(layer)?;
    let mut keep: HashSet<ObjectId> = HashSet::from([layer]);
    loop {
        let before = keep.len();
        for object in &doc.objects {
            if object.parent_id.is_some_and(|parent| keep.contains(&parent)) {
                keep.insert(object.id);
            }
        }
        if keep.len() == before {
            break;
        }
    }
    let mut filtered = doc.clone();
    filtered.objects.retain(|object| keep.contains(&object.id));
    Some(filtered)
}

pub fn pick_with_flags(doc: &Document, flags: &CanvasFlags, x: f32, y: f32, mode: PickMode, layer: Option<ObjectId>) -> Option<Colour> {
    pick_live(doc, flags, x, y, mode, layer, &[])
}

pub fn pick_live(doc: &Document, flags: &CanvasFlags, x: f32, y: f32, mode: PickMode, layer: Option<ObjectId>, staged: &[StagedEffect]) -> Option<Colour> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let filtered = match (mode, layer) {
        (PickMode::Merged, _) | (_, None) => None,
        (_, Some(id)) => Some(layer_only(doc, id)?),
    };
    let source = filtered.as_ref().unwrap_or(doc);
    let size = mode.size();
    let half = size as f32 / 2.0;
    let pixels = rasterize_with_effects(source, flags, staged, [x - half, y - half, size as f32, size as f32], size, size);
    average(&pixels)
}

pub fn pick(doc: &Document, x: f32, y: f32, mode: PickMode, layer: Option<ObjectId>) -> Option<Colour> {
    pick_with_flags(doc, &CanvasFlags::default(), x, y, mode, layer)
}
