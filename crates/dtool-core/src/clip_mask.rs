use crate::command::Session;
use crate::geometry::{object_matrix, object_world_quad};
use crate::object::{ClipRole, Object, ObjectId, ObjectKind};
use crate::paint::{Colour, Paint};
use crate::scene::{doc_contours, Prim, Scene};
use crate::view_state::CanvasFlags;
use crate::Document;
use dtool_protocol::{Command, CommandError, CommandResult};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::overlay::FloatOverlay;
use i_overlay::mesh::float::stroke::offset::StrokeOffset;
use i_overlay::mesh::float::style::{LineJoin, StrokeStyle};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

pub const CLIP_SET: &str = "layer.clip.set";
pub const CLIP_RELEASE: &str = "layer.clip.release";
pub const VECTOR_MASK_SET: &str = "layer.vector_mask.set";
pub const VECTOR_MASK_APPLY_BELOW: &str = "layer.vector_mask.apply_below";
pub const VECTOR_MASK_RELEASE: &str = "layer.vector_mask.release";
pub const MASK_VIEW_SET: &str = "view.mask_view.set";

type Ring = Vec<[f64; 2]>;

fn closed_rings(doc: &Document, object: &Object) -> Vec<Ring> {
    let matrix = object_matrix(object);
    crate::scene::warped_contours(doc, object, &matrix).unwrap_or_else(|| doc_contours(object, &matrix))
        .into_iter()
        .filter(|(points, closed)| *closed && points.len() >= 3)
        .map(|(points, _)| points.into_iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect())
        .collect()
}

fn mask_rings(doc: &Document, object: &Object) -> Vec<Ring> {
    if object.is_group() && object.expanded_stroke_effects {
        return doc.objects().iter()
            .filter(|child| child.parent_id == Some(object.id) && child.stroke_mask_outline)
            .flat_map(|child| closed_rings(doc, child))
            .collect();
    }
    closed_rings(doc, object)
}

fn world_rings(doc: &Document, object: &Object) -> Vec<Ring> {
    let rings = mask_rings(doc, object);
    // A closed mask with no filled region hides the entire target.
    let closed_mask = object.clip_role == ClipRole::Mask && matches!(&object.kind, ObjectKind::Curve(curve)
        if curve.contour.closed || curve.holes.iter().any(|contour| contour.closed));
    if !rings.is_empty() || closed_mask || (object.is_group() && object.expanded_stroke_effects && object.clip_role == ClipRole::Mask) {
        return crate::boolean::parity_rings(rings, object.fill_rule);
    }
    vec![object_world_quad(object).iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect()]
}

fn intersect(a: &[Ring], b: &[Ring], rule: FillRule) -> Vec<Ring> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    FloatOverlay::with_subj_and_clip(&a.to_vec(), &b.to_vec()).overlay(OverlayRule::Intersect, rule).into_iter().flatten().collect()
}

fn to_rings(contours: &[Vec<[f32; 2]>]) -> Vec<Ring> {
    contours.iter().map(|ring| ring.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect()).collect()
}

fn to_contours(rings: &[Ring]) -> Vec<Vec<[f32; 2]>> {
    rings.iter().map(|ring| ring.iter().map(|p| [p[0] as f32, p[1] as f32]).collect()).collect()
}

pub(crate) fn bounds(rings: &[Ring]) -> Option<[f32; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in rings.iter().flatten() {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    (b[2] >= b[0] && b[3] >= b[1]).then(|| [b[0] as f32, b[1] as f32, (b[2] - b[0]) as f32, (b[3] - b[1]) as f32])
}

fn parent_of<'a>(doc: &'a Document, object: &Object) -> Option<&'a Object> {
    object.parent_id.and_then(|id| doc.get_object(id))
}

pub fn is_mask(doc: &Document, object: &Object) -> bool {
    is_mask_or_descendant(object, |id| doc.get_object(id))
}

fn is_mask_or_descendant<'a>(mut current: &'a Object, find: impl Fn(ObjectId) -> Option<&'a Object>) -> bool {
    for _ in 0..64 {
        let Some(parent) = current.parent_id.and_then(&find) else { break; };
        if current.clip_role == ClipRole::Mask {
            return true;
        }
        current = parent;
    }
    false
}

pub fn vector_masks(doc: &Document, target: ObjectId) -> Vec<&Object> {
    doc.objects.iter().filter(|o| o.parent_id == Some(target) && o.clip_role == ClipRole::Mask).collect()
}

pub fn has_roles(doc: &Document) -> bool {
    doc.objects.iter().any(|o| o.clip_role != ClipRole::None && o.parent_id.is_some())
}

pub fn clip_region(doc: &Document, id: ObjectId) -> Option<Vec<Ring>> {
    clip_region_with_crop(doc, id, layer_clip_region(doc, id))
}

fn clip_region_with_crop(doc: &Document, id: ObjectId, masks: Option<Vec<Ring>>) -> Option<Vec<Ring>> {
    intersect_crop(masks, doc.get_object(id).and_then(crate::scene::oriented_vector_crop))
}

fn intersect_crop(masks: Option<Vec<Ring>>, quad: Option<crate::scene::Quad>) -> Option<Vec<Ring>> {
    let Some(quad) = quad else { return masks };
    let crop = vec![quad_ring(quad)];
    Some(match masks {
        Some(rings) => intersect(&rings, &crop, FillRule::EvenOdd),
        None => crop,
    })
}

/// Clipping and mask layers, excluding the object's own crop.
pub fn layer_clip_region(doc: &Document, id: ObjectId) -> Option<Vec<Ring>> {
    collect_layer_clip_region(doc, doc.get_object(id)?, |target| vector_masks(doc, target).into_iter(), |id| doc.get_object(id))
}

fn collect_layer_clip_region<'a, I>(
    doc: &'a Document,
    mut current: &'a Object,
    masks: impl Fn(ObjectId) -> I,
    find: impl Fn(ObjectId) -> Option<&'a Object>,
) -> Option<Vec<Ring>>
where
    I: Iterator<Item = &'a Object>,
{
    let mut shapes: Vec<Vec<Ring>> = Vec::new();
    for _ in 0..64 {
        shapes.extend(masks(current.id).map(|mask| world_rings(doc, mask)));
        let Some(parent) = current.parent_id.and_then(&find) else { break };
        if current.clip_role == ClipRole::Clip {
            shapes.push(world_rings(doc, parent));
        }
        current = parent;
    }
    let mut shapes = shapes.into_iter();
    let first = shapes.next()?;
    if first.is_empty() {
        return Some(Vec::new());
    }
    let mut region: Vec<Ring> = FloatOverlay::with_subj(&first).overlay(OverlayRule::Subject, FillRule::EvenOdd).into_iter().flatten().collect();
    for next in shapes {
        region = intersect(&region, &next, FillRule::EvenOdd);
    }
    Some(region)
}

/// Index masks for callers that need to resolve the clip of many objects at once.
pub(crate) struct ClipRegions<'a> {
    doc: &'a Document,
    layers: HashMap<ObjectId, &'a Object>,
    masks: HashMap<ObjectId, Vec<&'a Object>>,
}

impl<'a> ClipRegions<'a> {
    pub(crate) fn new(doc: &'a Document) -> Self {
        let mut masks: HashMap<_, Vec<_>> = HashMap::new();
        for object in &doc.objects {
            if object.clip_role == ClipRole::Mask && let Some(parent) = object.parent_id {
                masks.entry(parent).or_default().push(object);
            }
        }
        let layers = doc.objects.iter().map(|object| (object.id, object)).collect();
        Self { doc, layers, masks }
    }

    pub(crate) fn is_mask(&self, object: &Object) -> bool {
        is_mask_or_descendant(object, |id| self.layers.get(&id).copied())
    }

    pub(crate) fn region(&self, object: &Object) -> Option<Vec<Ring>> {
        let masks = collect_layer_clip_region(
            self.doc,
            object,
            |target| self.masks.get(&target).into_iter().flatten().copied(),
            |id| self.layers.get(&id).copied(),
        );
        intersect_crop(masks, crate::scene::vector_crop_quad(object))
    }
}

fn prim_object(prim: &Prim) -> Option<ObjectId> {
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

pub(crate) fn clip_fill_contours(contours: &[Vec<[f32; 2]>], region: &[Ring]) -> Vec<Vec<[f32; 2]>> {
    to_contours(&intersect(&to_rings(contours), region, FillRule::EvenOdd))
}

/// Intersect even-odd vector ink with a document-space rectangle.
pub fn clip_fill_rect(contours: &[Vec<[f32; 2]>], [x, y, width, height]: [f32; 4]) -> Vec<Vec<[f32; 2]>> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let [left, top, right, bottom] = [x, y, x + width, y + height].map(f64::from);
    let region = [vec![[left, top], [right, top], [right, bottom], [left, bottom]]];
    clip_fill_contours(contours, &region)
}

fn quad_ring(quad: crate::scene::Quad) -> Ring {
    quad.into_iter().map(|point| point.map(f64::from)).collect()
}

/// Trim vector ink geometrically. Raster and glyph ink use the clip region when drawn.
pub(crate) fn clip_crop_ink(object: &Object, quad: crate::scene::Quad, start: usize, prims: &mut Vec<Prim>) {
    let region = [quad_ring(quad)];
    for prim in prims.split_off(start) {
        if matches!(prim, Prim::Fill { .. } | Prim::Appearance { .. } | Prim::Stroke { .. }) {
            clip_prim(prim, object, &region, prims);
        } else {
            prims.push(prim);
        }
    }
}

fn clip_prim(prim: Prim, object: &Object, region: &[Ring], out: &mut Vec<Prim>) {
    match prim {
        Prim::Fill { object: id, contours, origin, paint, opacity } => {
            let contours = clip_fill_contours(&contours, region);
            if !contours.is_empty() {
                out.push(Prim::Fill { object: id, contours, origin, paint, opacity });
            }
        }
        Prim::Appearance { object, mut inks, origin, opacity, tint } => {
            for ink in &mut inks {
                let kept = intersect(&to_rings(&ink.contours), region, FillRule::EvenOdd);
                ink.contours = to_contours(&kept);
            }
            inks.retain(|ink| !ink.contours.is_empty());
            if !inks.is_empty() {
                out.push(Prim::Appearance { object, inks, origin, opacity, tint });
            }
        }
        Prim::Stroke { object: id, points, closed, width, paint, opacity } if width > 0.0 && points.len() >= 2 => {
            let line: Ring = points.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect();
            let band: Vec<Ring> = line.stroke(StrokeStyle::new(f64::from(width)).line_join(LineJoin::Bevel), closed).into_iter().flatten().collect();
            let kept = intersect(&band, region, FillRule::NonZero);
            if !kept.is_empty() {
                out.push(Prim::Fill { object: id, contours: to_contours(&kept), origin: object.pos, paint, opacity });
            }
        }
        Prim::Mark { .. } | Prim::Handle { .. } | Prim::Clip { .. } | Prim::Page { .. } => out.push(prim),
        other => {
            if let Some(rect) = bounds(region) {
                out.push(Prim::Clip { object: object.id, rect: Some(rect) });
                out.push(other);
                out.push(Prim::Clip { object: object.id, rect: None });
            }
        }
    }
}

pub fn apply(doc: &Document, flags: &CanvasFlags, scene: &mut Scene) {
    if let Some(target) = flags.mask_view
        && let Some(prims) = mask_view_prims(doc, scene, target)
    {
        scene.prims = prims;
        return;
    }
    if !has_roles(doc) {
        return;
    }
    let mut regions: HashMap<ObjectId, Option<Vec<Ring>>> = HashMap::new();
    let mut out = Vec::with_capacity(scene.prims.len());
    for prim in std::mem::take(&mut scene.prims) {
        let Some(object) = prim_object(&prim).and_then(|id| doc.get_object(id)) else {
            out.push(prim);
            continue;
        };
        if is_mask(doc, object) {
            if matches!(prim, Prim::Mark { .. } | Prim::Handle { .. }) {
                out.push(prim);
            }
            continue;
        }
        match regions.entry(object.id).or_insert_with(|| layer_clip_region(doc, object.id)) {
            Some(region) => clip_prim(prim, object, region, &mut out),
            None => out.push(prim),
        }
    }
    scene.prims = out;
}

pub fn has_any_mask(doc: &Document, target: ObjectId) -> bool {
    doc.objects.iter().any(|object| object.parent_id == Some(target) && object.clip_role == ClipRole::Mask)
        || doc.layer_masks.iter().any(|mask| mask.object == target)
}

pub(crate) fn reconcile_mask_view(session: &mut Session) {
    if let Some(target) = session.view.canvas_flags.mask_view
        && (session.document.get_object(target).is_none() || !has_any_mask(&session.document, target))
    {
        session.view.canvas_flags.mask_view = None;
    }
}

fn solid(value: u8) -> Paint {
    Paint::Solid { colour: Colour::new(value, value, value, 255) }
}

const MASK_PREVIEW_FULL_RESOLUTION_PIXELS: usize = 4_000_000;
const MASK_PREVIEW_RUN_LIMIT: usize = 16_384;
const MASK_PREVIEW_FALLBACK_SIDE: usize = 128;

struct MaskRun {
    level: u8,
    x: [usize; 2],
    y: [usize; 2],
}

fn mask_preview_runs(coverage: &[u8], width: usize, run_limit: usize) -> Option<Vec<MaskRun>> {
    let quantize = |value: u8| ((u32::from(value) * 16 + 127) / 255) as u8;
    let mut runs = Vec::new();
    let mut open = BTreeMap::new();
    let mut next = BTreeMap::new();
    for (y, row) in coverage.chunks_exact(width).enumerate() {
        let mut x = 0;
        while x < width {
            let level = quantize(row[x]);
            let start = x;
            while x < width && quantize(row[x]) == level {
                x += 1;
            }
            if level != 0 {
                let key = (level, start, x);
                next.insert(key, open.remove(&key).unwrap_or(y));
                if runs.len() + open.len() + next.len() > run_limit {
                    return None;
                }
            }
        }
        // Join identical runs across rows so antialiasing cannot introduce seams.
        runs.extend(std::mem::take(&mut open).into_iter().map(|((level, left, right), top)| {
            MaskRun { level, x: [left, right], y: [top, y] }
        }));
        std::mem::swap(&mut open, &mut next);
    }
    let height = coverage.len() / width;
    runs.extend(open.into_iter().map(|((level, left, right), top)| {
        MaskRun { level, x: [left, right], y: [top, height] }
    }));
    Some(runs)
}

fn average_mask_preview(coverage: &[u8], [width, height]: [usize; 2]) -> (Vec<u8>, [usize; 2]) {
    let size = [width, height].map(|side| side.min(MASK_PREVIEW_FALLBACK_SIDE));
    let [preview_width, preview_height] = size;
    let mut preview = vec![0; preview_width * preview_height];
    for y in 0..preview_height {
        let top = y * height / preview_height;
        let bottom = (y + 1) * height / preview_height;
        for x in 0..preview_width {
            let left = x * width / preview_width;
            let right = (x + 1) * width / preview_width;
            let sum = (top..bottom)
                .flat_map(|row| &coverage[row * width + left..row * width + right])
                .map(|&value| u64::from(value))
                .sum::<u64>();
            let count = ((right - left) * (bottom - top)) as u64;
            preview[y * preview_width + x] = ((sum + count / 2) / count) as u8;
        }
    }
    (preview, size)
}

fn mask_view_prims(doc: &Document, scene: &Scene, target: ObjectId) -> Option<Vec<Prim>> {
    let object = doc.get_object(target)?;
    if !has_any_mask(doc, target) {
        return None;
    }
    let mut prims: Vec<Prim> = scene.prims.iter().filter(|prim| matches!(prim, Prim::Page { .. })).cloned().collect();
    let quad = object_world_quad(object);
    let area: Vec<Ring> = vec![quad.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect()];
    let vector = vector_masks(doc, target);
    let mut region: Option<Vec<Ring>> = None;
    for mask in &vector {
        let rings = world_rings(doc, mask);
        region = Some(match region {
            None if rings.is_empty() => Vec::new(),
            None => FloatOverlay::with_subj(&rings).overlay(OverlayRule::Subject, FillRule::EvenOdd).into_iter().flatten().collect(),
            Some(current) => intersect(&current, &rings, FillRule::EvenOdd),
        });
    }
    let mut cover: Vec<Ring> = area.clone();
    if let Some(region) = &region {
        if let Some(b) = bounds(region) {
            cover.push(vec![[f64::from(b[0]), f64::from(b[1])], [f64::from(b[0] + b[2]), f64::from(b[1])], [f64::from(b[0] + b[2]), f64::from(b[1] + b[3])], [f64::from(b[0]), f64::from(b[1] + b[3])]]);
        }
    }
    let cover: Vec<Ring> = FloatOverlay::with_subj(&cover).overlay(OverlayRule::Subject, FillRule::NonZero).into_iter().flatten().collect();
    prims.push(Prim::Fill { object: target, contours: to_contours(&cover), origin: object.pos, paint: solid(0), opacity: 1.0 });
    match doc.layer_masks.iter().find(|mask| mask.object == target) {
        Some(mask) => {
            let coverage = &mask.coverage;
            let size = [coverage.width as usize, coverage.height as usize];
            if size.contains(&0) || size[0].checked_mul(size[1]) != Some(coverage.coverage.len()) {
                return Some(prims);
            }
            let run_limit = if coverage.coverage.len() <= MASK_PREVIEW_FULL_RESOLUTION_PIXELS {
                usize::MAX
            } else {
                MASK_PREVIEW_RUN_LIMIT
            };
            let (runs, [w, h]) = if let Some(runs) = mask_preview_runs(&coverage.coverage, size[0], run_limit) {
                (runs, size)
            } else {
                // Only simplify the preview when the mask would generate too much geometry.
                // Averaging retains fractional coverage in detailed or noisy masks.
                let (preview, size) = average_mask_preview(&coverage.coverage, size);
                (mask_preview_runs(&preview, size[0], usize::MAX).unwrap_or_default(), size)
            };
            let [left, top, width, height] = mask.relative_frame().map(f64::from);
            let at = |u: usize, v: usize| {
                let u = left + width * u as f64 / w as f64;
                let v = top + height * v as f64 / h as f64;
                [
                    f64::from(quad[0][0]) + f64::from(quad[1][0] - quad[0][0]) * u + f64::from(quad[3][0] - quad[0][0]) * v,
                    f64::from(quad[0][1]) + f64::from(quad[1][1] - quad[0][1]) * u + f64::from(quad[3][1] - quad[0][1]) * v,
                ]
            };
            for MaskRun { level, x: [left, right], y: [top, bottom] } in runs {
                let ring = vec![at(left, top), at(right, top), at(right, bottom), at(left, bottom)];
                let rings = match &region {
                    Some(region) => intersect(&[ring], region, FillRule::EvenOdd),
                    None => vec![ring],
                };
                if !rings.is_empty() {
                    prims.push(Prim::Fill { object: target, contours: to_contours(&rings), origin: object.pos, paint: solid(((u32::from(level) * 255) / 16) as u8), opacity: 1.0 });
                }
            }
        }
        None => {
            if let Some(region) = region.filter(|region| !region.is_empty()) {
                prims.push(Prim::Fill { object: target, contours: to_contours(&region), origin: object.pos, paint: solid(255), opacity: 1.0 });
            }
        }
    }
    Some(prims)
}

fn ours(name: &str) -> bool {
    matches!(name, CLIP_SET | CLIP_RELEASE | VECTOR_MASK_SET | VECTOR_MASK_APPLY_BELOW | VECTOR_MASK_RELEASE | MASK_VIEW_SET)
}

enum Plan {
    Role { id: ObjectId, parent: Option<ObjectId>, role: ClipRole },
    View(Option<ObjectId>),
    Same,
}

fn id_of(params: &Value, keys: &[&str]) -> Result<Option<ObjectId>, CommandError> {
    for key in keys {
        match params.get(*key) {
            None => continue,
            Some(Value::Null) => return Ok(None),
            Some(value) => return value.as_u64().filter(|id| *id > 0).map(|id| Some(ObjectId(id))).ok_or_else(CommandError::invalid_params),
        }
    }
    Ok(None)
}

fn descends_from(doc: &Document, id: ObjectId, ancestor: ObjectId) -> bool {
    let mut current = doc.get_object(id);
    for _ in 0..256 {
        let Some(object) = current else { return false };
        if object.id == ancestor {
            return true;
        }
        current = parent_of(doc, object);
    }
    true
}

fn below_target(doc: &Document, source: &Object) -> Result<ObjectId, CommandError> {
    let index = doc.objects.iter().position(|object| object.id == source.id).ok_or_else(CommandError::not_found)?;
    let page = (source.parent_id.is_none() && doc.pages.len() > 1)
        .then(|| crate::data_merge::layout::object_page_index(doc, source.id));
    let target = doc.objects[..index].iter().rev().find(|object| {
        object.parent_id == source.parent_id
            && page.is_none_or(|page| crate::data_merge::layout::object_page_index(doc, object.id) == page)
    }).ok_or_else(|| CommandError::new("CANNOT_EXECUTE"))?;
    if crate::artboard::is_artboard(source) || crate::artboard::is_artboard(target)
        || source.clip_role != ClipRole::None || target.clip_role == ClipRole::Mask
    {
        return Err(CommandError::new("CANNOT_EXECUTE"));
    }
    Ok(target.id)
}

fn plan(session: &Session, command: &Command) -> Result<Plan, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let doc = &session.document;
    let params = &command.params;
    if command.name == MASK_VIEW_SET {
        let target = id_of(params, &["id"])?;
        if let Some(id) = target {
            doc.get_object(id).ok_or_else(CommandError::not_found)?;
            if !has_any_mask(doc, id) {
                return Err(CommandError::not_found());
            }
        }
        return Ok(if session.view.canvas_flags.mask_view == target { Plan::Same } else { Plan::View(target) });
    }
    let apply_below = command.name == VECTOR_MASK_APPLY_BELOW;
    let id = match id_of(params, &["id"])? {
        Some(id) => id,
        None if apply_below => match session.selection.ids.as_slice() {
            [id] => *id,
            _ => return Err(CommandError::new("CANNOT_EXECUTE")),
        },
        None => return Err(CommandError::invalid_params()),
    };
    let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    if doc.is_object_locked(object) {
        return Err(CommandError::new("LOCKED"));
    }
    let role = match command.name.as_str() {
        CLIP_SET => ClipRole::Clip,
        VECTOR_MASK_SET | VECTOR_MASK_APPLY_BELOW => ClipRole::Mask,
        _ => ClipRole::None,
    };
    if role == ClipRole::None {
        let wanted = if command.name == CLIP_RELEASE { ClipRole::Clip } else { ClipRole::Mask };
        if object.clip_role != wanted || object.parent_id.is_none() {
            return Err(CommandError::not_found());
        }
        let parent = parent_of(doc, object).and_then(|parent| parent.parent_id);
        return Ok(Plan::Role { id, parent, role });
    }
    let parent = if apply_below {
        below_target(doc, object)?
    } else {
        id_of(params, &["parent", "target"])?.or(object.parent_id).ok_or_else(CommandError::invalid_params)?
    };
    let target = doc.get_object(parent).ok_or_else(CommandError::not_found)?;
    if parent == id || descends_from(doc, parent, id) {
        return Err(CommandError::invalid_params());
    }
    if doc.is_object_locked(target) {
        return Err(CommandError::new("LOCKED"));
    }
    if role == ClipRole::Mask && mask_rings(doc, object).is_empty() {
        return Err(CommandError::invalid_params());
    }
    if object.parent_id == Some(parent) && object.clip_role == role {
        return Ok(Plan::Same);
    }
    Ok(Plan::Role { id, parent: Some(parent), role })
}

fn place_above(doc: &mut Document, id: ObjectId, parent: ObjectId) {
    let Some(from) = doc.objects.iter().position(|o| o.id == id) else { return };
    let object = doc.objects.remove(from);
    let last = doc.objects.iter().enumerate().filter(|(_, o)| descends_from(doc, o.id, parent)).map(|(index, _)| index).max();
    let at = last.map(|index| index + 1).unwrap_or(doc.objects.len()).min(doc.objects.len());
    doc.objects.insert(at, object);
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    ours(command.name.as_str()).then(|| plan(session, command).is_ok())
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !ours(command.name.as_str()) {
        return None;
    }
    Some((|| match plan(session, command)? {
        Plan::Same => Ok(json!({ "doc_version": session.generation, "changes": [], "mask_view": session.view.canvas_flags.mask_view.map(|id| id.0) })),
        Plan::View(target) => {
            session.view.canvas_flags.mask_view = target;
            Ok(json!({ "doc_version": session.generation, "changes": [{ "name": MASK_VIEW_SET }], "mask_view": target.map(|id| id.0) }))
        }
        Plan::Role { id, parent, role } => {
            let before = crate::commands::arrange::snapshot(session);
            if let Some(object) = session.document.find_object_mut(id.0) {
                object.parent_id = parent;
                object.clip_role = role;
            }
            if let (Some(parent), true) = (parent, role != ClipRole::None) {
                place_above(&mut session.document, id, parent);
            }
            let extra = json!({ "id": id.0, "parent": parent.map(|p| p.0), "role": role });
            crate::commands::arrange::commit_snapshot(session, command.name.as_str(), before, extra)
        }
    })())
}
