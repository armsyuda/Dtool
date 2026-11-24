use crate::command::Session;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::paint::Paint;
use crate::raster_source;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const NODE_MERGE: &str = "node.merge";
pub const NODE_RASTERIZE: &str = "node.rasterize";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        NODE_MERGE => Some(session.has_document && target_count(session, command) >= 2),
        NODE_RASTERIZE => Some(crate::rasterise::roots(session, command).is_ok()),
        crate::rasterise::RASTERISE | crate::rasterise::RASTERISE_TRIM => Some(crate::rasterise::can_execute(session, command)),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        NODE_MERGE => Some(merge(session, command)),
        NODE_RASTERIZE => Some(rasterize(session, command)),
        crate::rasterise::RASTERISE | crate::rasterise::RASTERISE_TRIM => Some(crate::rasterise::run(session, command)),
        _ => None,
    }
}

fn target_count(session: &Session, command: &Command) -> usize {
    super::arrange::listed_ids(command).ok().flatten().unwrap_or_else(|| session.selection.ids.clone())
        .into_iter()
        .filter(|id| session.document.get_object(*id).is_some())
        .fold(Vec::new(), |mut unique, id| {
            if !unique.contains(&id) {
                unique.push(id);
            }
            unique
        })
        .len()
}

fn scale(session: &Session, command: &Command) -> Result<f32, CommandError> {
    let dpi = match command.params.get("dpi") {
        Some(value) => {
            let dpi = value.as_f64().ok_or_else(CommandError::invalid_params)? as f32;
            if !dpi.is_finite() || dpi <= 0.0 {
                return Err(CommandError::invalid_params());
            }
            dpi
        }
        None => session.document.dpi,
    };
    if session.document.dpi.is_finite() && session.document.dpi > 0.0 {
        Ok(dpi / session.document.dpi)
    } else {
        Ok(1.0)
    }
}

fn keep_original(command: &Command) -> bool {
    command.params.get("keep_original").and_then(Value::as_bool).unwrap_or(false)
}

fn identity_map(from: &[ObjectId], to: ObjectId) -> Vec<Value> {
    from.iter().map(|id| json!({ "from": id.0, "to": to.0 })).collect()
}

fn survivor_of(session: &Session, ids: &[ObjectId]) -> ObjectId {
    session
        .document
        .objects
        .iter()
        .map(|object| object.id)
        .find(|id| ids.contains(id))
        .unwrap_or(ids[0])
}

fn layer_pixels(document: &crate::document::Document, object: &Object) -> Option<([usize; 2], Vec<u8>)> {
    if let Some(pixels) = raster_source::rgba(document, object) {
        return Some(pixels);
    }
    if let Some(pixels) = crate::scene::raster_rgba(object) {
        return Some(pixels);
    }
    match &object.fill {
        Paint::Solid { colour } => {
            let width = object.size[0].abs().round().max(1.0) as usize;
            let height = object.size[1].abs().round().max(1.0) as usize;
            let mut data = Vec::with_capacity(width * height * 4);
            for _ in 0..(width * height) {
                data.extend_from_slice(&[colour.r, colour.g, colour.b, colour.a]);
            }
            Some(([width, height], data))
        }
        _ => None,
    }
}

fn over(dst: &mut [u8], src: [u8; 4], opacity: f32) {
    let src_a = (src[3] as f32 / 255.0) * opacity.clamp(0.0, 1.0);
    if src_a <= 0.0 {
        return;
    }
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return;
    }
    for index in 0..3 {
        let src_c = src[index] as f32 / 255.0;
        let dst_c = dst[index] as f32 / 255.0;
        let value = (src_c * src_a + dst_c * dst_a * (1.0 - src_a)) / out_a;
        dst[index] = (value * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
}

fn blit(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    origin: [f32; 2],
    scale: f32,
    object: &Object,
    source: &[u8],
    source_size: [usize; 2],
) {
    let [source_width, source_height] = source_size;
    if source_width == 0 || source_height == 0 {
        return;
    }
    let x0 = ((object.pos[0] - origin[0]) * scale).round() as i32;
    let y0 = ((object.pos[1] - origin[1]) * scale).round() as i32;
    let x1 = (((object.pos[0] + object.size[0] - origin[0]) * scale).round() as i32).max(x0 + 1);
    let y1 = (((object.pos[1] + object.size[1] - origin[1]) * scale).round() as i32).max(y0 + 1);
    for y in y0.max(0)..y1.min(dest_height as i32) {
        for x in x0.max(0)..x1.min(dest_width as i32) {
            let u = (x - x0) as f32 / (x1 - x0) as f32;
            let v = (y - y0) as f32 / (y1 - y0) as f32;
            let sx = ((u * source_width as f32).floor() as usize).min(source_width - 1);
            let sy = ((v * source_height as f32).floor() as usize).min(source_height - 1);
            let start = (sy * source_width + sx) * 4;
            let pixel = [source[start], source[start + 1], source[start + 2], source[start + 3]];
            let dest_start = (y as usize * dest_width as usize + x as usize) * 4;
            over(&mut dest[dest_start..dest_start + 4], pixel, object.opacity);
        }
    }
}

fn composite(
    document: &crate::document::Document,
    ids: &[ObjectId],
    origin: [f32; 2],
    size: [f32; 2],
    scale: f32,
) -> (u32, u32, Vec<u8>) {
    let width = ((size[0] * scale).round() as u32).max(1);
    let height = ((size[1] * scale).round() as u32).max(1);
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for object in &document.objects {
        if !ids.contains(&object.id) {
            continue;
        }
        if let Some((source_size, source)) = layer_pixels(document, object) {
            blit(&mut pixels, width, height, origin, scale, object, &source, source_size);
        }
    }
    (width, height, pixels)
}

fn apply_raster(
    document: &mut crate::document::Document,
    id: ObjectId,
    pos: [f32; 2],
    size: [f32; 2],
    width: u32,
    height: u32,
    rgba: Vec<u8>,
) {
    if let Some(object) = document.objects.iter_mut().find(|object| object.id == id) {
        object.pos = pos;
        object.size = size;
        object.raster_data = Some(rgba.clone());
        object.raster_size = Some((width, height));
        if matches!(object.kind, ObjectKind::Group | ObjectKind::Shape) {
            object.kind = ObjectKind::Image;
        }
    }
    document.pixel_layers.insert(id, crate::pixel::PixelLayer::from_rgba(width, height, &rgba));
}

fn retarget(document: &mut crate::document::Document, from: &[ObjectId], to: ObjectId) {
    let survivor_has_image = document.placed_images.iter().any(|placed| placed.object_id == to);
    let mut moved_image = false;
    document.placed_images.retain_mut(|placed| {
        if !from.contains(&placed.object_id) {
            return true;
        }
        if !survivor_has_image && !moved_image {
            placed.object_id = to;
            moved_image = true;
            true
        } else {
            false
        }
    });
    let survivor_has_mask = document.layer_masks.iter().any(|mask| mask.object == to);
    let mut moved_mask = false;
    document.layer_masks.retain_mut(|mask| {
        if !from.contains(&mask.object) {
            return true;
        }
        if !survivor_has_mask && !moved_mask {
            mask.object = to;
            moved_mask = true;
            true
        } else {
            false
        }
    });
}

fn remove_ids(document: &mut crate::document::Document, ids: &[ObjectId]) {
    document.objects.retain(|object| !ids.contains(&object.id));
    for id in ids {
        document.pixel_layers.remove(*id);
        document.placed_images.retain(|placed| placed.object_id != *id);
        document.layer_masks.retain(|mask| mask.object != *id);
    }
}

fn resource_of(document: &crate::document::Document, id: ObjectId) -> Option<u64> {
    document.placed_resource(id).map(|resource| resource.0)
}

fn merge(session: &mut Session, command: &Command) -> CommandResult {
    let roots = super::arrange::target_ids(session, command)?;
    if roots.len() < 2 {
        return Err(CommandError::invalid_params());
    }
    let scale = scale(session, command)?;
    let participating = super::arrange::descendants(session, &roots);
    let survivor = survivor_of(session, &roots);
    let (origin, size) = super::arrange::bounds(session, &participating);
    if size[0] <= 0.0 || size[1] <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    let before = super::arrange::snapshot(session);
    let (width, height, rgba) = composite(&session.document, &participating, origin, size, scale);
    let consumed: Vec<ObjectId> = participating.iter().copied().filter(|id| *id != survivor).collect();
    retarget(&mut session.document, &consumed, survivor);
    apply_raster(&mut session.document, survivor, origin, size, width, height, rgba);
    remove_ids(&mut session.document, &consumed);
    session.selection.ids = vec![survivor];
    let resource = resource_of(&session.document, survivor);
    let mut extra = json!({
        "id": survivor.0,
        "object_id": survivor.0,
        "survivor": survivor.0,
        "ids": [survivor.0],
        "map": identity_map(&participating, survivor)
    });
    if let Some(resource) = resource {
        extra["resource"] = json!(resource);
    }
    super::arrange::commit(session, NODE_MERGE, before, extra)
}

fn rasterize(session: &mut Session, command: &Command) -> CommandResult {
    let roots = crate::rasterise::roots(session, command)?;
    let relative_scale = scale(session, command)?;
    let canvas_scale = crate::units::to_px(1.0, session.document.units, session.document.dpi)
        .ok_or_else(CommandError::invalid_params)?;
    let pixel_scale = relative_scale * canvas_scale;
    if !pixel_scale.is_finite() || pixel_scale <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    let keep = keep_original(command);
    let mut document = session.document.clone();
    let mut map = Vec::new();
    let mut ids = Vec::new();
    let mut first = roots[0];
    let mut first_resource = None;
    for root in roots {
        let (mut object, pixels, descendants) = crate::rasterise::render(&session.document, root, pixel_scale, false)?;
        object.raster_data = Some(pixels.to_rgba());
        let resource = resource_of(&session.document, root);
        let id = if keep {
            let created = document.create_object(object);
            map.push(json!({ "from": root.0, "to": root.0 }));
            map.push(json!({ "from": created.0, "to": created.0 }));
            created
        } else {
            let consumed: Vec<_> = document.objects.iter().map(|object| object.id)
                .filter(|id| descendants.contains(id)).collect();
            retarget(&mut document, &consumed, root);
            document.detach_objects(&descendants);
            document.layer_masks.retain(|mask| mask.object != root);
            *document.find_object_mut(root.0).ok_or_else(CommandError::not_found)? = object;
            map.push(json!({ "from": root.0, "to": root.0 }));
            map.extend(consumed.iter().map(|id| json!({ "from": id.0, "to": root.0 })));
            root
        };
        document.pixel_layers.insert(id, pixels);
        ids.push(id);
        if first_resource.is_none() {
            first = id;
            first_resource = resource;
        }
    }
    crate::frame::prune_chains(&mut document);
    let mut extra = json!({
        "id": first.0,
        "object_id": first.0,
        "ids": ids,
        "map": map,
        "keep_original": keep
    });
    if let Some(resource) = first_resource {
        extra["resource"] = json!(resource);
    }
    crate::commands::document_change::commit(session, NODE_RASTERIZE, document, ids, extra)
}
