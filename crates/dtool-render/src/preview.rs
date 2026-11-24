use dtool_core::{CommandError, Document, ViewState};
use dtool_core::commands::preview::{PreviewRequest, RenderedPreview, PREVIEW_MAX_BYTES, PREVIEW_MAX_PX};
use dtool_core::layer_style::raster::{self, Item};
use dtool_core::scene::{Scene, contours_bounds};
use dtool_core::view::{view_to_doc, visible_doc_quad};

const MAX_RENDER_PIXELS: usize = 32_000_000;

pub fn install() -> bool {
    let scene = crate::scene::install();
    let preview = dtool_core::commands::preview::register_provider(render);
    scene && preview
}

fn crc(bytes: &[u8]) -> u32 {
    let mut value = u32::MAX;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 { value = (value >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(value & 1)); }
    }
    !value
}

fn chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = output.len();
    output.extend_from_slice(kind); output.extend_from_slice(data);
    output.extend_from_slice(&crc(&output[start..]).to_be_bytes());
}

fn png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, CommandError> {
    let row = width as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for bytes in pixels.chunks_exact(row) { raw.push(0); raw.extend_from_slice(bytes); }
    let mut zlib = vec![0x78, 0x01];
    let count = raw.len().div_ceil(65535);
    for (index, block) in raw.chunks(65535).enumerate() {
        zlib.push(if index + 1 == count { 1 } else { 0 });
        let length = block.len() as u16;
        zlib.extend_from_slice(&length.to_le_bytes()); zlib.extend_from_slice(&(!length).to_le_bytes()); zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in raw { a = (a + u32::from(byte)) % 65521; b = (b + a) % 65521; }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut result = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new(); ihdr.extend_from_slice(&width.to_be_bytes()); ihdr.extend_from_slice(&height.to_be_bytes()); ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut result, b"IHDR", &ihdr); chunk(&mut result, b"IDAT", &zlib); chunk(&mut result, b"IEND", &[]);
    if result.len() > PREVIEW_MAX_BYTES { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    Ok(result)
}

pub fn render(document: &Document, view: &ViewState, request: &PreviewRequest) -> Result<RenderedPreview, CommandError> {
    if document.stories.is_empty() {
        return render_preview(document, view, request);
    }
    let library = dtool_text::FontLibrary::shared();
    dtool_text::frame::with_doc_layout_reuse(document, &library, || render_preview(document, view, request))
}

fn render_preview(document: &Document, view: &ViewState, request: &PreviewRequest) -> Result<RenderedPreview, CommandError> {
    if !request.max_px.is_finite() || request.max_px <= 0.0 { return Err(CommandError::invalid_params()); }
    if request.max_px > PREVIEW_MAX_PX { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    let (mut bounds, source) = if let Some(id) = request.id {
        let object = document.get_object(id).ok_or_else(CommandError::not_found)?;
        let bounds = if document.pixel_layers.contains(id)
            && dtool_core::geometry::object_matrix(object) == dtool_core::geometry::IDENTITY
        {
            [object.pos[0], object.pos[1], object.size[0].abs(), object.size[1].abs()]
        } else {
            dtool_core::geometry::object_world_bounds(object)
        };
        (bounds, "object")
    } else if let Some(size) = request.view_size {
        ([0.0, 0.0, size[0], size[1]], "current_view")
    } else {
        let page = document.pages.first().ok_or_else(CommandError::no_document)?;
        ([0.0, 0.0, page.size[0], page.size[1]], "document_bounds")
    };
    let group_preview = request.id.and_then(|id| document.get_object(id).filter(|object| object.is_group())
        .map(|_| document.subtree_ids(id)));
    let mut scene = if let Some(id) = request.id.filter(|_| group_preview.is_none()) {
        crate::scene::build_scene_for(document, &view.canvas_flags, Some(id))
    } else {
        dtool_core::scene::build_scene(document, &view.canvas_flags)
    };
    if let Some(id) = request.id {
        scene.prims.retain(|prim| raster::prim_object(prim).is_none_or(|object| {
            group_preview.as_ref().map_or(object == id, |members| members.contains(&object))
        }));
    }
    let items = raster::effect_items(document, &view.canvas_flags, raster::scene_items(&scene), &[]);
    let has_effects = items.iter().any(|item| matches!(item, Item::Layer(_)));
    if has_effects && let Some(id) = request.id
        && let Some([x, y, w, h]) = raster::effect_bounds(document, &[], id, &scene.prims)
    {
        let left = bounds[0].min(x);
        let top = bounds[1].min(y);
        bounds = [left, top, (bounds[0] + bounds[2]).max(x + w) - left,
            (bounds[1] + bounds[3]).max(y + h) - top];
    }
    if !bounds.iter().all(|value| value.is_finite()) || bounds[2] <= 0.0 || bounds[3] <= 0.0 { return Err(CommandError::invalid_params()); }
    let scale = request.max_px / f64::from(bounds[2].max(bounds[3]));
    let width = (f64::from(bounds[2]) * scale).floor().max(1.0) as u32;
    let height = (f64::from(bounds[3]) * scale).floor().max(1.0) as u32;
    let bytes = width as usize * height as usize * 4;
    if bytes + height as usize + bytes.div_ceil(65535) * 5 + 80 > PREVIEW_MAX_BYTES { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    if (width as usize * height as usize).saturating_mul(document.objects.len().max(1)) > MAX_RENDER_PIXELS { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    let mapped = if source == "current_view" { Some(view) } else { None };
    let pixels = if has_effects {
        render_effects(document, &scene, &items, bounds, [width, height], mapped)?
    } else {
        dtool_core::scene::rasterize_scene(document, &scene, bounds, width, height, mapped)
    };
    Ok(RenderedPreview { png: png(width, height, &pixels)?, width, height, bounds, source })
}

fn render_effects(
    document: &Document, scene: &Scene, items: &[Item], bounds: [f32; 4],
    [width, height]: [u32; 2], view: Option<&ViewState>,
) -> Result<Vec<u8>, CommandError> {
    let [_, _, w, h] = bounds;
    let (region, size) = if let Some(view) = view {
        let [left, top, right, bottom] = contours_bounds(&[visible_doc_quad(view, [w, h]).to_vec()])
            .ok_or_else(CommandError::invalid_params)?;
        let [rw, rh] = [right - left, bottom - top];
        let size = if view.rotation_deg.rem_euclid(360.0) == 0.0 {
            [width, height]
        } else {
            let ratio = (width as f32).max(height as f32) / rw.max(rh);
            [rw, rh].map(|span| (span * ratio).round().max(1.0) as u32)
        };
        ([left, top, rw, rh], size)
    } else {
        (bounds, [width, height])
    };
    let [rw, rh] = [region[2], region[3]];
    if !region.iter().all(|v| v.is_finite()) || rw <= 0.0 || rh <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    let density = f64::from(size[0]) / f64::from(rw);
    let work = items.iter().filter_map(|item| match item {
        Item::Layer(layer) => Some(f64::from(layer.raster_padding()) * density),
        _ => None,
    }).map(|padding| {
        let pad = 2.0 * (padding.ceil() + 1.0);
        (f64::from(size[0]) + pad) * (f64::from(size[1]) + pad)
    }).sum::<f64>();
    if !work.is_finite() || work > MAX_RENDER_PIXELS as f64 {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    let rendered = raster::rasterize_items_scaled(document, scene, items, region, size[0], size[1], 1.0);
    let Some(view) = view.filter(|view| view.rotation_deg.rem_euclid(360.0) != 0.0) else {
        return Ok(rendered);
    };
    let mut mapped = vec![0; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let [dx, dy] = view_to_doc(view, [(x as f32 + 0.5) * w / width as f32, (y as f32 + 0.5) * h / height as f32]);
            let sx = ((dx - region[0]) / rw * size[0] as f32) as u32;
            let sy = ((dy - region[1]) / rh * size[1] as f32) as u32;
            let src = (sy.min(size[1] - 1) as usize * size[0] as usize + sx.min(size[0] - 1) as usize) * 4;
            let dst = (y as usize * width as usize + x as usize) * 4;
            mapped[dst..dst + 4].copy_from_slice(&rendered[src..src + 4]);
        }
    }
    Ok(mapped)
}


#[cfg(test)]
mod tests {
    use super::*;
    use dtool_core::document::{Story, Units};
    use dtool_core::object::{Object, ObjectKind};
    use dtool_core::paint::{Colour, Paint};
    use dtool_core::pixel::PixelLayer;
    use dtool_core::raster_source::LayerMask;
    use dtool_core::selection::PixelSelection;
    use dtool_core::StoryId;

    fn request(max_px: f64) -> PreviewRequest {
        PreviewRequest { id: None, max_px, view_size: None }
    }

    #[test]
    fn preview_renders_rotated_shape_and_masked_image() {
        assert!(install());
        let mut document = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
        document.create_object(Object {
            pos: [10.0, 10.0],
            size: [40.0, 40.0],
            rotation_deg: 15.0,
            fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) },
            ..Object::default()
        });
        let pixels = vec![255u8; 8 * 8 * 4];
        let id = document.create_object(Object {
            pos: [80.0, 80.0],
            size: [80.0, 80.0],
            raster_data: Some(pixels.clone()),
            raster_size: Some((8, 8)),
            ..Object::default()
        });
        document.pixel_layers.insert(id, PixelLayer::from_rgba(8, 8, &pixels));
        document.layer_masks.push(LayerMask {
            object: id,
            coverage: PixelSelection::all(8, 8),
        });
        let story = StoryId(1);
        document.stories.push(Story { id: story, text: "Hi".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
        document.create_object(Object {
            pos: [10.0, 120.0],
            size: [80.0, 40.0],
            kind: ObjectKind::FrameText { story },
            ..Object::default()
        });
        let preview = render(&document, &dtool_core::ViewState::default(), &request(128.0)).expect("preview");
        assert!(preview.png.len() > 32);
        assert_eq!(&preview.png[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(preview.width > 0 && preview.height > 0);
    }
}
