use crate::command::Session;
use crate::geometry::{object_matrix, object_world_quad};
use crate::object::{ClipRole, Object, ObjectId};
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
use std::collections::HashMap;

pub const CLIP_SET: &str = "layer.clip.set";
pub const CLIP_RELEASE: &str = "layer.clip.release";
pub const VECTOR_MASK_SET: &str = "layer.vector_mask.set";
pub const VECTOR_MASK_APPLY_BELOW: &str = "layer.vector_mask.apply_below";
pub const VECTOR_MASK_RELEASE: &str = "layer.vector_mask.release";
pub const MASK_VIEW_SET: &str = "view.mask_view.set";

type Ring = Vec<[f64; 2]>;

fn closed_rings(object: &Object) -> Vec<Ring> {
    doc_contours(object, &object_matrix(object))
        .into_iter()
        .filter(|(points, closed)| *closed && points.len() >= 3)
        .map(|(points, _)| points.into_iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect())
        .collect()
}

fn mask_rings(doc: &Document, object: &Object) -> Vec<Ring> {
    if object.is_group() && object.expanded_stroke_effects {
        return doc.objects().iter()
            .filter(|child| child.parent_id == Some(object.id) && child.stroke_mask_outline)
            .flat_map(closed_rings)
            .collect();
    }
    closed_rings(object)
}

fn world_rings(doc: &Document, object: &Object) -> Vec<Ring> {
    let rings = mask_rings(doc, object);
    if !rings.is_empty() || (object.is_group() && object.expanded_stroke_effects && object.clip_role == ClipRole::Mask) {
        return rings;
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

fn bounds(rings: &[Ring]) -> Option<[f32; 4]> {
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
    let mut current = object;
    for _ in 0..64 {
        let Some(parent) = parent_of(doc, current) else { break; };
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
    let mut shapes: Vec<Vec<Ring>> = Vec::new();
    let mut current = doc.get_object(id)?;
    for _ in 0..64 {
        shapes.extend(vector_masks(doc, current.id).into_iter().map(|mask| world_rings(doc, mask)));
        let Some(parent) = parent_of(doc, current) else { break };
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
        match regions.entry(object.id).or_insert_with(|| clip_region(doc, object.id)) {
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
            let (w, h) = (coverage.width as usize, coverage.height as usize);
            if w == 0 || h == 0 || coverage.coverage.len() != w * h || w * h > 4_000_000 {
                return Some(prims);
            }
            let at = |u: f64, v: f64| {
                [
                    f64::from(quad[0][0]) + f64::from(quad[1][0] - quad[0][0]) * u + f64::from(quad[3][0] - quad[0][0]) * v,
                    f64::from(quad[0][1]) + f64::from(quad[1][1] - quad[0][1]) * u + f64::from(quad[3][1] - quad[0][1]) * v,
                ]
            };
            let level = |value: u8| ((u32::from(value) * 16 + 127) / 255) as u8;
            let mut runs: Vec<(u8, Ring)> = Vec::new();
            let mut open = std::collections::BTreeMap::new();
            let mut next = std::collections::BTreeMap::new();
            let mut finish = |(l, start, end): (u8, usize, usize), top: usize, bottom: usize| {
                let (u0, u1, v0, v1) = (start as f64 / w as f64, end as f64 / w as f64,
                    top as f64 / h as f64, bottom as f64 / h as f64);
                runs.push((l, vec![at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)]));
            };
            for y in 0..h {
                let row = &coverage.coverage[y * w..(y + 1) * w];
                let mut x = 0;
                while x < w {
                    let l = level(row[x]);
                    let start = x;
                    while x < w && level(row[x]) == l {
                        x += 1;
                    }
                    if l == 0 {
                        continue;
                    }
                    let key = (l, start, x);
                    next.insert(key, open.remove(&key).unwrap_or(y));
                }
                // Keep equal runs joined so the canvas does not antialias a seam on each row.
                for (key, top) in std::mem::take(&mut open) {
                    finish(key, top, y);
                }
                std::mem::swap(&mut open, &mut next);
            }
            for (key, top) in open {
                finish(key, top, h);
            }
            for (l, ring) in runs {
                let rings = match &region {
                    Some(region) => intersect(&[ring], region, FillRule::EvenOdd),
                    None => vec![ring],
                };
                if !rings.is_empty() {
                    prims.push(Prim::Fill { object: target, contours: to_contours(&rings), origin: object.pos, paint: solid(((u32::from(l) * 255) / 16) as u8), opacity: 1.0 });
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
    if object.locked {
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
    if target.locked {
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
