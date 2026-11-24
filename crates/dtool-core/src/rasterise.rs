use std::collections::HashSet;

use crate::commands::arrange;
use crate::layer_style::raster::{effect_bounds, prim_object, render_scene_region};
use crate::object::{ClipRole, Object, ObjectId, ObjectKind};
use crate::pixel::PixelLayer;
use crate::scene::{Prim, build_scene, scene_ready};
use crate::{CanvasFlags, Command, CommandError, CommandResult, Document, Session};
use serde_json::json;

pub const RASTERISE: &str = "layer.rasterise";
pub const RASTERISE_TRIM: &str = "layer.rasterise.trim";
const MAX_PIXELS: u64 = 32 * 1024 * 1024;

fn with_ancestors(document: &Document, start: ObjectId) -> Vec<ObjectId> {
    let mut ancestors = Vec::new();
    let mut next = Some(start);
    while let Some(id) = next {
        if ancestors.contains(&id) { break; }
        ancestors.push(id);
        next = document.get_object(id).and_then(|object| object.parent_id);
    }
    ancestors
}

pub(crate) fn roots(session: &Session, command: &Command) -> Result<Vec<ObjectId>, CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let ids = arrange::target_ids(session, command)?;
    let doc = &session.document;
    let mut roots = Vec::new();
    for id in ids.iter().copied() {
        let ancestors = with_ancestors(doc, id);
        if ancestors.iter().skip(1).any(|id| ids.contains(id)) { continue; }
        if ancestors.iter().any(|id| doc.get_object(*id).is_some_and(|object| object.locked)) {
            return Err(CommandError::new("LOCKED"));
        }
        let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
        if matches!(object.kind, ObjectKind::Artboard) || object.clip_role == ClipRole::Mask {
            return Err(CommandError::invalid_params());
        }
        roots.push(id);
    }
    if roots.is_empty() { return Err(CommandError::invalid_params()); }
    Ok(roots)
}

pub fn can_execute(session: &Session, command: &Command) -> bool {
    scene_ready() && roots(session, command).is_ok()
}

pub fn run(session: &mut Session, command: &Command) -> CommandResult {
    let roots = roots(session, command)?;
    if !scene_ready() { return Err(CommandError::new("NOT_AVAILABLE")); }
    let trim = match command.params.get("trim") {
        Some(value) => value.as_bool().ok_or_else(CommandError::invalid_params)?,
        None => command.name == RASTERISE_TRIM,
    };
    let scale = crate::units::to_px(1.0, session.document.units, session.document.dpi)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(CommandError::invalid_params)?;
    let mut document = session.document.clone();
    for &id in &roots {
        let (object, pixels, descendants) = render(&session.document, id, scale, trim)?;
        document.detach_objects(&descendants);
        document.placed_images.retain(|entry| entry.object_id != id);
        document.layer_masks.retain(|mask| mask.object != id);
        *document.find_object_mut(id.0).ok_or_else(CommandError::not_found)? = object;
        document.pixel_layers.insert(id, pixels);
    }
    crate::frame::prune_chains(&mut document);
    crate::commands::document_change::commit(session, &command.name, document, roots.clone(), json!({
        "ids": roots.iter().map(|id| id.0).collect::<Vec<_>>(),
        "trim": trim,
    }))
}

pub(crate) fn render(doc: &Document, id: ObjectId, scale: f32, trim: bool) -> Result<(Object, PixelLayer, HashSet<ObjectId>), CommandError> {
    let root = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    let mut subtree = doc.subtree_ids(id);
    let mut source = doc.clone();
    // Parent opacity and the layer's blend settings are applied when the new
    // image is composited at the same position in the layer stack.
    for id in with_ancestors(doc, id) {
        if let Some(object) = source.find_object_mut(id.0) {
            object.pickable = true;
            object.opacity = 1.0;
            object.blend_mode = Default::default();
            object.blend_ranges = None;
        }
    }
    let flags = CanvasFlags::default();
    let mut scene = build_scene(&source, &flags);
    scene.prims.retain(|prim| prim_object(prim).is_some_and(|id| subtree.contains(&id))
        && !matches!(prim, Prim::Handle { .. } | Prim::Mark { .. }));
    if trim && scene.prims.is_empty() { return Err(CommandError::invalid_params()); }

    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    let mut include = |point: [f32; 2]| {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    };
    for &member in &subtree {
        if let Some(quad) = scene.quad(member) {
            quad.into_iter().for_each(&mut include);
        }
        let prims = scene.prims.iter().filter(|prim| prim_object(prim) == Some(member));
        for bounds in effect_bounds(&source, &[], member, prims).into_iter()
            .chain(crate::scene::text_glyph_bounds(&source, member).map(|[x0, y0, x1, y1]| [x0, y0, x1 - x0, y1 - y0]))
        {
            let [x, y, w, h] = bounds;
            include([x, y]);
            include([x + w, y + h]);
        }
    }
    let origin = min.map(|coord| (coord * scale).floor());
    let extent = max.map(|coord| (coord * scale).ceil());
    let dims = [extent[0] - origin[0], extent[1] - origin[1]];
    if dims.iter().any(|dim| !(1.0..=32768.0).contains(dim)) {
        return Err(CommandError::invalid_params());
    }
    let [w, h] = dims.map(|dim| dim as u32);
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    let bounds = [origin[0] / scale, origin[1] / scale, w as f32 / scale, h as f32 / scale];
    let rgba = render_scene_region(&source, &flags, &[], &scene, bounds, [w, h], 1.0);
    let [left, top, right, bottom] = if trim {
        ink_bounds(&rgba, w, h).ok_or_else(CommandError::invalid_params)?
    } else {
        [0, 0, w, h]
    };
    let size = [right - left, bottom - top];
    let clipped = if size == [w, h] {
        rgba
    } else {
        rgba.chunks_exact(w as usize * 4).skip(top as usize).take(size[1] as usize)
            .flat_map(|row| row[left as usize * 4..right as usize * 4].iter().copied()).collect()
    };
    let pixels = PixelLayer::from_rgba(size[0], size[1], &clipped);
    let pos = [(origin[0] + left as f32) / scale, (origin[1] + top as f32) / scale];
    let object = Object {
        id,
        name: root.name.clone(),
        pos,
        size: size.map(|value| value as f32 / scale),
        kind: ObjectKind::Image,
        raster_size: Some((size[0], size[1])),
        opacity: root.opacity,
        blend_mode: root.blend_mode,
        blend_ranges: root.blend_ranges.clone(),
        parent_id: root.parent_id,
        page_id: root.page_id,
        constraint: root.constraint.clone(),
        pickable: root.pickable,
        transform_origin: root.transform_origin,
        clip_role: root.clip_role,
        baked_warps: root.baked_warps.clone(),
        ..Object::default()
    };
    subtree.remove(&id);
    Ok((object, pixels, subtree))
}

fn ink_bounds(rgba: &[u8], w: u32, h: u32) -> Option<[u32; 4]> {
    let [mut left, mut top, mut right, mut bottom] = [w, h, 0, 0];
    for (index, pixel) in rgba.chunks_exact(4).enumerate() {
        if pixel[3] == 0 { continue; }
        let x = index as u32 % w;
        let y = index as u32 / w;
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + 1);
        bottom = bottom.max(y + 1);
    }
    (right > left && bottom > top).then_some([left, top, right, bottom])
}
