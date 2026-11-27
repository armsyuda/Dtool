use crate::resources::{ImageResource, ResourceId};
use crate::{Command, CommandError, CommandResult, Object, ObjectKind, Session};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const PLACE: &str = "image.place";

pub struct DecodedImage {
    pub resource: ImageResource,
    pub rgba: Vec<u8>,
}

pub type ImageDecoder = fn(&Path) -> Result<DecodedImage, CommandError>;
static DECODER: OnceLock<ImageDecoder> = OnceLock::new();

pub fn register_decoder(decoder: ImageDecoder) -> bool {
    DECODER.set(decoder).is_ok() || DECODER.get().is_some()
}

pub fn decode_path(path: &Path) -> Result<DecodedImage, CommandError> {
    DECODER.get().ok_or_else(|| CommandError::new("NOT_AVAILABLE"))?(path)
}

fn wants_link(params: &serde_json::Value) -> bool {
    params.get("link").and_then(serde_json::Value::as_bool).unwrap_or(false)
}

#[derive(Deserialize)]
struct Place {
    pixel_size: [u32; 2],
    rgba: Vec<u8>,
    pos: [f32; 2],
    size: Option<[f32; 2]>,
    dpi: Option<f32>,
    resource: Option<ResourceId>,
    source: Option<ImageResource>,
    name: Option<String>,
    picture_frame: Option<crate::ObjectId>,
    #[serde(default)]
    fill_frame: bool,
}

fn place(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let mut params = command.params.clone();
    let place_path = params.get("path").and_then(|value| value.as_str()).map(str::trim).filter(|path| !path.is_empty()).map(PathBuf::from);
    if let Some(path) = &place_path {
        if ["rgba", "pixel_size", "resource", "source"].iter().any(|key| params.get(*key).is_some()) {
            return Err(CommandError::invalid_params());
        }
        let decoded = decode_path(path)?;
        params["pixel_size"] = json!(decoded.resource.pixel_size);
        params["source"] = json!(decoded.resource);
        params["rgba"] = json!(decoded.rgba);
    }
    let link = wants_link(&command.params);
    let args: Place = serde_json::from_value(params).map_err(|_| CommandError::invalid_params())?;
    let [width, height] = args.pixel_size;
    let length = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4));
    if width == 0 || height == 0 || length != Some(args.rgba.len()) || args.pos.iter().any(|n| !n.is_finite()) || (args.resource.is_some() && args.source.is_some()) {
        return Err(CommandError::invalid_params());
    }
    let mut document = session.document.clone();
    let (mut resource, resource_id) = match args.resource {
        Some(id) => (document.image_resource(id).cloned().ok_or_else(CommandError::not_found)?, Some(id)),
        None => (args.source.unwrap_or_else(|| ImageResource { pixel_size: args.pixel_size, dpi: args.dpi.unwrap_or(72.0), ..ImageResource::default() }), None),
    };
    if resource.pixel_size != args.pixel_size { return Err(CommandError::invalid_params()); }
    if link && resource_id.is_some() {
        return Err(CommandError::invalid_params());
    }
    if let Some(id) = args.picture_frame {
        let frame = document.get_object(id).ok_or_else(CommandError::not_found)?;
        if frame.kind != ObjectKind::PictureFrame || frame.locked || resource_id.is_some() {
            return Err(CommandError::invalid_params());
        }
        let resource = crate::resources::replace_placement(&mut document, id, resource, args.rgba, link, place_path)?;
        if args.fill_frame
            && let Some(mut frame) = super::picture_frame::get_frame(&document, id)
        {
            frame.fit = super::picture_frame::ImageFit::MaxFit;
            frame.content = super::picture_frame::ContentTransform::default();
            frame.manual_layout = None;
            frame.image = Some(resource);
            super::picture_frame::put_frame(&mut document, frame);
        }
        return super::document_change::commit(session, PLACE, document, vec![id], json!({"id":id.0,"object_id":id.0,"resource":resource.0,"ids":[id.0]}));
    }
    if link {
        let path = place_path.clone().or(resource.path.clone()).ok_or_else(CommandError::invalid_params)?;
        resource = crate::resources::as_linked_resource(resource, path);
    }
    let dpi = args.dpi.unwrap_or(resource.dpi);
    if !dpi.is_finite() || dpi <= 0.0 || !document.dpi.is_finite() || document.dpi <= 0.0 { return Err(CommandError::invalid_params()); }
    let size = args.size.unwrap_or([width as f32 * document.dpi / dpi, height as f32 * document.dpi / dpi]);
    if size.iter().any(|n| !n.is_finite() || *n <= 0.0) { return Err(CommandError::invalid_params()); }
    let resource_id = resource_id.unwrap_or_else(|| document.add_image_resource(resource));
    let mut object = Object { name: args.name.unwrap_or_else(|| document.image_resource(resource_id).map(|r| r.display_name()).unwrap_or_default()), pos: args.pos, size, raster_data: Some(args.rgba.clone()), raster_size: Some((width, height)), kind: ObjectKind::Image, ..Object::default() };
    let center = [args.pos[0] + size[0] * 0.5, args.pos[1] + size[1] * 0.5];
    crate::artboard::edit::assign_new_object(&document, &mut object, center);
    let id = document.create_object(object);
    document.place_image(id, resource_id)?;
    document.pixel_layers.insert(id, crate::pixel::PixelLayer::from_rgba(width, height, &args.rgba));
    super::document_change::commit(session, PLACE, document, vec![id], json!({"id":id.0,"object_id":id.0,"resource":resource_id.0,"ids":[id.0]}))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == PLACE).then_some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    (command.name == PLACE).then(|| place(session, command))
}
