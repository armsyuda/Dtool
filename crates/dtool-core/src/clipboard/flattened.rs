use dtool_protocol::{CommandError, CommandResult};

use crate::selection::PixelSurface;
use crate::view::ViewTarget;
use crate::{CanvasFlags, Document, Object, ObjectKind, Session};

pub const COPY_FLATTENED: &str = "edit.copy_flattened";
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn copy(session: &mut Session) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let doc = &session.document;
    let scale = crate::units::to_px(1.0, doc.units, doc.dpi)
        .filter(|scale| scale.is_finite() && *scale > 0.0).ok_or_else(invalid_image)?;
    let target;
    let mut mask_region = None;
    let (bounds, [width, height]) = if let Some(mask) = &doc.pixel_selection
        && let Some([x, y, width, height]) = mask.bounds()
    {
        let surface = PixelSurface::for_page(doc, mask.page_id)
            .filter(|surface| surface.matches(doc, mask)).ok_or_else(invalid_image)?;
        let [left, top] = surface.pixel_to_doc([x as f32, y as f32]);
        target = Some(ViewTarget::Page(surface.page_id));
        mask_region = Some((mask, x, y));
        ([left, top, width as f32 / scale, height as f32 / scale], [width, height])
    } else {
        target = session.view_canvas_size
            .and_then(|size| ViewTarget::at_view_center_with_hint(doc, &session.view, size, session.view_target_hint))
            .or(session.view_target_hint);
        let bounds = target.and_then(|surface| surface.bounds(doc)).or_else(|| ViewTarget::first(doc))
            .ok_or_else(invalid_image)?;
        let size = [bounds[2], bounds[3]].map(|side| (side * scale).round().max(1.0) as u32);
        (bounds, size)
    };
    let bytes = (width as usize).checked_mul(height as usize).and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes > 0 && *bytes <= MAX_IMAGE_BYTES).ok_or_else(invalid_image)?;

    let flags = CanvasFlags::default();
    let mut scene = crate::scene::build_scene(doc, &flags);
    if let Some(ViewTarget::Artboard(board)) = target {
        let members = doc.subtree_ids(board);
        scene.prims.retain(|prim| crate::layer_style::raster::prim_object(prim).is_some_and(|id| members.contains(&id)));
    }
    let effect_scale = crate::effect_raster::effect_scale(doc, width as f32 / bounds[2]);
    let mut rgba = crate::layer_style::raster::render_scene_region(
        doc, &flags, &[], &scene, bounds, [width, height], effect_scale,
    );
    if rgba.len() != bytes { return Err(invalid_image()); }
    if let Some((mask, x, y)) = mask_region {
        for (row, pixels) in rgba.chunks_exact_mut(width as usize * 4).enumerate() {
            for (column, pixel) in pixels.chunks_exact_mut(4).enumerate() {
                let coverage = u16::from(mask.get(x + column as u32, y + row as u32));
                pixel[3] = ((u16::from(pixel[3]) * coverage + 127) / 255) as u8;
            }
        }
    }

    let size = [bounds[2], bounds[3]];
    let mut source = Document::new(size, doc.units, doc.dpi, 1);
    let mut resource = crate::path_brush::encode_resource([width as usize, height as usize], &rgba, doc.dpi)
        .ok_or_else(invalid_image)?;
    let png = resource.data.clone().ok_or_else(invalid_image)?;
    resource.name = "clipboard.png".into();
    let resource_id = source.add_image_resource(resource);
    let id = source.create_object(Object {
        name: "clipboard.png".into(),
        kind: ObjectKind::Image, pos: [bounds[0], bounds[1]], size, ..Default::default()
    });
    source.place_image(id, resource_id)?;
    source.pixel_layers.insert(id, crate::pixel::PixelLayer::from_rgba(width, height, &rgba));
    let mut bundle = super::collect_bundle(&source, &[id]);
    bundle.source_dimensions = Some((doc.units, doc.dpi));
    session.clipboard.set_flattened(bundle, png);
    Ok(serde_json::json!({
        "doc_version": session.generation, "width": width, "height": height,
        "changes": [{ "name": COPY_FLATTENED }]
    }))
}

fn invalid_image() -> CommandError {
    CommandError::new("CLIPBOARD_IMAGE_INVALID")
}
