use crate::command::Session;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::resources::ResourceId;
use crate::undo::Change;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::document::Document;
use std::collections::HashMap;

pub const FRAME_IMAGE_ADD: &str = "frame.image.add";
pub const FRAME_IMAGE_FIT: &str = "frame.image.fit.set";
pub const FRAME_IMAGE_ANCHOR: &str = "frame.image.anchor.set";
pub const FRAME_IMAGE_CONTENT: &str = "frame.image.content.set";
pub const FRAME_IMAGE_QUERY: &str = "frame.image.query";
pub const FRAME_CONVERT: &str = "frame.convert";
pub const FRAME_RESTORE: &str = "frame.image.restore";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFit {
    Native,
    MinFit,
    MaxFit,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameShape {
    Rect,
    Ellipse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor9 {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ContentTransform {
    pub offset: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
}

impl Default for ContentTransform {
    fn default() -> Self {
        Self { offset: [0.0, 0.0], scale: [1.0, 1.0], rotation: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ManualLayout {
    /// Frame dimensions at the transition from automatic fit.
    pub(crate) fit_size: [f32; 2],
    /// Frame dimensions when the user last positioned the content by its anchor.
    pub(crate) anchor_size: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PictureFrame {
    pub object: ObjectId,
    pub shape: FrameShape,
    pub fit: ImageFit,
    pub anchor: Anchor9,
    pub content: ContentTransform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_layout: Option<ManualLayout>,
    pub image: Option<ResourceId>,
}

impl PictureFrame {
    pub fn new(object: ObjectId, shape: FrameShape) -> Self {
        Self {
            object,
            shape,
            fit: ImageFit::MinFit,
            anchor: Anchor9::Center,
            content: ContentTransform::default(),
            manual_layout: None,
            image: None,
        }
    }
}

pub fn frames(document: &Document) -> HashMap<ObjectId, PictureFrame> {
    document.picture_frames.iter().map(|frame| (frame.object, frame.clone())).collect()
}

pub fn set_frames(document: &mut Document, frames: HashMap<ObjectId, PictureFrame>) {
    let mut list: Vec<PictureFrame> = frames.into_values().collect();
    list.sort_by_key(|frame| frame.object.0);
    document.picture_frames = list;
}

pub fn get_frame(document: &Document, id: ObjectId) -> Option<PictureFrame> {
    document.picture_frames.iter().find(|frame| frame.object == id).cloned()
}

pub fn put_frame(document: &mut Document, frame: PictureFrame) {
    match document.picture_frames.iter_mut().find(|item| item.object == frame.object) {
        Some(slot) => *slot = frame,
        None => {
            document.picture_frames.push(frame);
            document.picture_frames.sort_by_key(|item| item.object.0);
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameLayout {
    pub shape: FrameShape,
    pub content: [f32; 4],
}

pub fn frame_image_size(document: &Document, object: &Object, frame: &PictureFrame) -> Option<[f32; 2]> {
    if let Some(image) = document.placed_resource(object.id).or(frame.image).and_then(|id| document.image_resource(id)) {
        return Some(image.native_size_pt());
    }
    if let Some((w, h)) = object.raster_size {
        return Some([w as f32, h as f32]);
    }
    crate::raster_source::dimensions(document, object).map(|[w, h]| [w as f32, h as f32])
}

pub fn frame_layout(document: &Document, object: &Object) -> Option<FrameLayout> {
    let frame = document.picture_frames.iter().find(|frame| frame.object == object.id)?;
    let size = frame_image_size(document, object, frame)?;
    let pos = [object.pos[0].min(object.pos[0] + object.size[0]), object.pos[1].min(object.pos[1] + object.size[1])];
    let (fit_size, anchor_size) = frame.manual_layout.map_or((object.size, object.size),
        |layout| (layout.fit_size, layout.anchor_size));
    let content = positioned_content(pos, fit_size, anchor_size, size, frame.fit, frame.anchor, frame.content);
    Some(FrameLayout { shape: frame.shape, content })
}

pub fn frame_contains(shape: FrameShape, u: f32, v: f32) -> bool {
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return false;
    }
    match shape {
        FrameShape::Rect => true,
        FrameShape::Ellipse => (u - 0.5).powi(2) + (v - 0.5).powi(2) <= 0.25,
    }
}

pub fn contains_point(document: &Document, object: &Object, point: [f32; 2]) -> bool {
    if object.kind != crate::object::ObjectKind::PictureFrame {
        return false;
    }
    let Some(frame) = document.picture_frames.iter().find(|frame| frame.object == object.id) else { return false };
    let Some([x, y]) = crate::geometry::doc_to_object_local(object, point) else { return false };
    let [width, height] = object.size.map(f32::abs);
    let left = object.pos[0] + object.size[0].min(0.0);
    let top = object.pos[1] + object.size[1].min(0.0);
    width > 0.0 && height > 0.0
        && crate::artboard::owner(document, object.id).is_none_or(|board| crate::artboard::contains(board, point))
        && frame_contains(frame.shape, (x - left) / width, (y - top) / height)
}

pub const FRAME_OUTLINE_SEGMENTS: usize = 96;

pub fn frame_outline(shape: FrameShape, [fx, fy, fw, fh]: [f32; 4]) -> Vec<[f32; 2]> {
    match shape {
        FrameShape::Rect => vec![[fx, fy], [fx + fw, fy], [fx + fw, fy + fh], [fx, fy + fh]],
        FrameShape::Ellipse => (0..FRAME_OUTLINE_SEGMENTS)
            .map(|i| {
                let a = i as f32 / FRAME_OUTLINE_SEGMENTS as f32 * std::f32::consts::TAU;
                [fx + fw * 0.5 * (1.0 + a.cos()), fy + fh * 0.5 * (1.0 + a.sin())]
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameGeometry {
    pub shape: FrameShape,
    pub frame: [f32; 4],
    pub content: [f32; 4],
    pub rotation: f32,
}

pub fn frame_geometry(document: &Document, object: &Object) -> Option<FrameGeometry> {
    let frame = document.picture_frames.iter().find(|frame| frame.object == object.id)?;
    let layout = frame_layout(document, object)?;
    let pos = [object.pos[0].min(object.pos[0] + object.size[0]), object.pos[1].min(object.pos[1] + object.size[1])];
    let rotation = if frame.content.rotation.is_finite() { frame.content.rotation } else { 0.0 };
    Some(FrameGeometry { shape: layout.shape, frame: [pos[0], pos[1], object.size[0].abs(), object.size[1].abs()], content: layout.content, rotation })
}

fn rotate_about(point: [f32; 2], centre: [f32; 2], degrees: f32) -> [f32; 2] {
    if degrees == 0.0 {
        return point;
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let d = [point[0] - centre[0], point[1] - centre[1]];
    [centre[0] + d[0] * cos - d[1] * sin, centre[1] + d[0] * sin + d[1] * cos]
}

impl FrameGeometry {
    pub fn content_point(&self, uv: [f32; 2]) -> [f32; 2] {
        let [cx, cy, cw, ch] = self.content;
        rotate_about([cx + uv[0] * cw, cy + uv[1] * ch], [cx + cw * 0.5, cy + ch * 0.5], self.rotation)
    }

    pub fn content_corners(&self) -> [[f32; 2]; 4] {
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]].map(|uv| self.content_point(uv))
    }

    pub fn shows_content(&self, uv: [f32; 2]) -> bool {
        let [fx, fy, fw, fh] = self.frame;
        if !(fw > 0.0 && fh > 0.0) {
            return false;
        }
        let p = self.content_point(uv);
        frame_contains(self.shape, (p[0] - fx) / fw, (p[1] - fy) / fh)
    }

    pub fn outline(&self) -> Vec<[f32; 2]> {
        frame_outline(self.shape, self.frame)
    }

    pub fn image_polygon(&self, quad: &[[f32; 2]; 4]) -> Vec<([f32; 2], [f32; 2])> {
        let corners = self.content_corners();
        let Some(map) = affine_from([corners[0], corners[1], corners[3]], [quad[0], quad[1], quad[3]]) else { return Vec::new() };
        let outline: Vec<[f32; 2]> = self.outline().into_iter().map(|p| apply_affine(&map, p)).collect();
        crate::geometry::clip_convex_polygon(outline, quad)
            .into_iter()
            .filter_map(|p| quad_uv(quad, p).map(|uv| (p, [uv[0].clamp(0.0, 1.0), uv[1].clamp(0.0, 1.0)])))
            .collect()
    }
}

pub fn quad_uv(quad: &[[f32; 2]; 4], p: [f32; 2]) -> Option<[f32; 2]> {
    let [a, b, _, d] = *quad;
    let e1 = [b[0] - a[0], b[1] - a[1]];
    let e2 = [d[0] - a[0], d[1] - a[1]];
    let det = e1[0] * e2[1] - e1[1] * e2[0];
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    let r = [p[0] - a[0], p[1] - a[1]];
    Some([(r[0] * e2[1] - r[1] * e2[0]) / det, (e1[0] * r[1] - e1[1] * r[0]) / det])
}

fn affine_from(src: [[f32; 2]; 3], dst: [[f32; 2]; 3]) -> Option<[f32; 6]> {
    let s1 = [src[1][0] - src[0][0], src[1][1] - src[0][1]];
    let s2 = [src[2][0] - src[0][0], src[2][1] - src[0][1]];
    let det = s1[0] * s2[1] - s1[1] * s2[0];
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    let d1 = [dst[1][0] - dst[0][0], dst[1][1] - dst[0][1]];
    let d2 = [dst[2][0] - dst[0][0], dst[2][1] - dst[0][1]];
    let i = [s2[1] / det, -s2[0] / det, -s1[1] / det, s1[0] / det];
    let a = d1[0] * i[0] + d2[0] * i[2];
    let c = d1[0] * i[1] + d2[0] * i[3];
    let b = d1[1] * i[0] + d2[1] * i[2];
    let d = d1[1] * i[1] + d2[1] * i[3];
    let e = dst[0][0] - a * src[0][0] - c * src[0][1];
    let f = dst[0][1] - b * src[0][0] - d * src[0][1];
    Some([a, b, c, d, e, f])
}

fn apply_affine(m: &[f32; 6], p: [f32; 2]) -> [f32; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

pub fn parse_fit(value: &Value) -> Option<ImageFit> {
    match value.as_str()?.trim().to_ascii_lowercase().as_str() {
        "native" | "none" => Some(ImageFit::Native),
        "min" | "minfit" | "contain" => Some(ImageFit::MinFit),
        "max" | "maxfit" | "cover" => Some(ImageFit::MaxFit),
        "stretch" | "fill" => Some(ImageFit::Stretch),
        _ => None,
    }
}

pub fn parse_anchor(value: &Value) -> Option<Anchor9> {
    match value.as_str()?.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "top_left" | "nw" => Some(Anchor9::TopLeft),
        "top" | "top_center" | "n" => Some(Anchor9::Top),
        "top_right" | "ne" => Some(Anchor9::TopRight),
        "left" | "center_left" | "w" => Some(Anchor9::Left),
        "center" | "middle" => Some(Anchor9::Center),
        "right" | "center_right" | "e" => Some(Anchor9::Right),
        "bottom_left" | "sw" => Some(Anchor9::BottomLeft),
        "bottom" | "bottom_center" | "s" => Some(Anchor9::Bottom),
        "bottom_right" | "se" => Some(Anchor9::BottomRight),
        _ => None,
    }
}

pub fn parse_shape(value: &Value) -> Option<FrameShape> {
    match value.as_str()?.trim().to_ascii_lowercase().as_str() {
        "rect" | "rectangle" | "box" => Some(FrameShape::Rect),
        "ellipse" | "oval" | "circle" => Some(FrameShape::Ellipse),
        _ => None,
    }
}

fn anchor_origin(anchor: Anchor9) -> [f32; 2] {
    match anchor {
        Anchor9::TopLeft => [0.0, 0.0],
        Anchor9::Top => [0.5, 0.0],
        Anchor9::TopRight => [1.0, 0.0],
        Anchor9::Left => [0.0, 0.5],
        Anchor9::Center => [0.5, 0.5],
        Anchor9::Right => [1.0, 0.5],
        Anchor9::BottomLeft => [0.0, 1.0],
        Anchor9::Bottom => [0.5, 1.0],
        Anchor9::BottomRight => [1.0, 1.0],
    }
}

pub fn content_rect(frame_pos: [f32; 2], frame_size: [f32; 2], image_size: [f32; 2], fit: ImageFit, anchor: Anchor9, content: ContentTransform) -> [f32; 4] {
    positioned_content(frame_pos, frame_size, frame_size, image_size, fit, anchor, content)
}

pub(crate) fn fitted_size(frame_size: [f32; 2], image_size: [f32; 2], fit: ImageFit) -> [f32; 2] {
    let frame_w = frame_size[0].abs().max(0.0);
    let frame_h = frame_size[1].abs().max(0.0);
    let image_w = image_size[0].abs().max(0.0);
    let image_h = image_size[1].abs().max(0.0);
    let (width, height) = match fit {
        ImageFit::Native => (image_w, image_h),
        ImageFit::Stretch => (frame_w, frame_h),
        ImageFit::MinFit => {
            let scale = if image_w <= 0.0 || image_h <= 0.0 {
                1.0
            } else {
                (frame_w / image_w).min(frame_h / image_h)
            };
            (image_w * scale, image_h * scale)
        }
        ImageFit::MaxFit => {
            let scale = if image_w <= 0.0 || image_h <= 0.0 {
                1.0
            } else {
                (frame_w / image_w).max(frame_h / image_h)
            };
            (image_w * scale, image_h * scale)
        }
    };
    [width, height]
}

fn positioned_content(
    frame_pos: [f32; 2], fit_size: [f32; 2], anchor_size: [f32; 2], image_size: [f32; 2],
    fit: ImageFit, anchor: Anchor9, content: ContentTransform,
) -> [f32; 4] {
    let [frame_w, frame_h] = anchor_size.map(f32::abs);
    let [mut width, mut height] = fitted_size(fit_size, image_size, fit);
    width *= if content.scale[0].is_finite() { content.scale[0] } else { 1.0 };
    height *= if content.scale[1].is_finite() { content.scale[1] } else { 1.0 };
    let origin = anchor_origin(anchor);
    let mut x = frame_pos[0] + (frame_w - width) * origin[0] + content.offset[0];
    let mut y = frame_pos[1] + (frame_h - height) * origin[1] + content.offset[1];
    if !x.is_finite() { x = frame_pos[0]; }
    if !y.is_finite() { y = frame_pos[1]; }
    [x, y, width.max(0.0), height.max(0.0)]
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        FRAME_IMAGE_ADD => Some(session.has_document),
        FRAME_CONVERT => Some(session.has_document && convert_targets(session, command).is_ok()),
        FRAME_IMAGE_FIT | FRAME_IMAGE_ANCHOR | FRAME_IMAGE_CONTENT | FRAME_IMAGE_QUERY => {
            Some(session.has_document && target_id(session, command).is_some_and(|id| session.document.get_object(id).is_some()))
        }
        FRAME_RESTORE => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        FRAME_IMAGE_ADD => Some(add(session, command)),
        FRAME_IMAGE_FIT => Some(set_fit(session, command)),
        FRAME_IMAGE_ANCHOR => Some(set_anchor(session, command)),
        FRAME_IMAGE_CONTENT => Some(set_content(session, command)),
        FRAME_IMAGE_QUERY => Some(query(session, command)),
        FRAME_CONVERT => Some(convert(session, command)),
        _ => None,
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != FRAME_RESTORE {
        return None;
    }
    if let Some(value) = command.params.get("document") {
        match serde_json::from_value::<crate::document::Document>(value.clone()) {
            Ok(document) => session.document = document,
            Err(_) => return Some(Err(CommandError::invalid_params())),
        }
    }
    if let Some(value) = command.params.get("selection") {
        if let Ok(ids) = serde_json::from_value::<Vec<u64>>(value.clone()) {
            session.selection.ids = ids.into_iter().map(ObjectId).collect();
        }
    }
    if let Some(value) = command.params.get("frames") {
        match serde_json::from_value::<Vec<PictureFrame>>(value.clone()) {
            Ok(items) => set_frames(&mut session.document, items.into_iter().map(|frame| (frame.object, frame)).collect()),
            Err(_) => return Some(Err(CommandError::invalid_params())),
        }
    }
    Some(Ok(()))
}

fn target_id(session: &Session, command: &Command) -> Option<ObjectId> {
    for key in ["object_id", "object", "node", "frame", "id"] {
        if let Some(value) = command.params.get(key) {
            if let Some(id) = value.as_u64() {
                return Some(ObjectId(id));
            }
        }
    }
    session.selection.ids.first().copied()
}

fn frames_value(map: &HashMap<ObjectId, PictureFrame>) -> Value {
    serde_json::to_value(map.values().cloned().collect::<Vec<_>>()).unwrap_or(json!([]))
}

fn restore_command(session: &Session, document: bool, frames: &HashMap<ObjectId, PictureFrame>) -> Command {
    let mut params = json!({
        "frames": frames_value(frames),
        "selection": session.selection.ids.iter().map(|id| id.0).collect::<Vec<_>>()
    });
    if document {
        params["document"] = serde_json::to_value(&session.document).unwrap_or(json!({}));
    }
    Command::with_params(FRAME_RESTORE, params)
}

fn commit(session: &mut Session, name: &str, before_document: bool, before_frames: HashMap<ObjectId, PictureFrame>, after_frames: HashMap<ObjectId, PictureFrame>, extra: Value) -> CommandResult {
    set_frames(&mut session.document, after_frames.clone());
    let undo = restore_command(session, before_document, &before_frames);
    if before_document {
        session.document.version += 1;
    }
    session.undo.push(Change {
        name: name.to_string(),
        redo: restore_command(session, before_document, &after_frames),
        undo,
    });
    session.generation += 1;
    let mut reply = json!({
        "doc_version": session.generation,
        "changes": [{ "name": name }]
    });
    if let (Some(target), Some(source)) = (reply.as_object_mut(), extra.as_object()) {
        target.extend(source.clone());
    }
    Ok(reply)
}

fn frame_payload(session: &Session, frame: &PictureFrame) -> Value {
    let rect = session.document.get_object(frame.object).and_then(|object| frame_layout(&session.document, object))
        .map(|layout| layout.content);
    json!({
        "id": frame.object.0,
        "object_id": frame.object.0,
        "shape": match frame.shape { FrameShape::Rect => "rect", FrameShape::Ellipse => "ellipse" },
        "fit": match frame.fit { ImageFit::Native => "native", ImageFit::MinFit => "min", ImageFit::MaxFit => "max", ImageFit::Stretch => "stretch" },
        "manual": frame.manual_layout.is_some(),
        "anchor": format!("{:?}", frame.anchor),
        "content": frame.content,
        "image": session.document.placed_resource(frame.object).or(frame.image).map(|id| id.0),
        "content_rect": rect
    })
}

fn add(session: &mut Session, command: &Command) -> CommandResult {
    let shape = command
        .params
        .get("shape")
        .and_then(parse_shape)
        .unwrap_or(FrameShape::Rect);
    let pos = command
        .params
        .get("pos")
        .and_then(|value| serde_json::from_value::<[f32; 2]>(value.clone()).ok())
        .unwrap_or([0.0, 0.0]);
    let size = command
        .params
        .get("size")
        .and_then(|value| serde_json::from_value::<[f32; 2]>(value.clone()).ok())
        .unwrap_or([100.0, 100.0]);
    if pos.iter().any(|n| !n.is_finite()) || size.iter().any(|n| !n.is_finite() || *n <= 0.0) {
        return Err(CommandError::invalid_params());
    }
    let before_frames = frames(&session.document);
    let mut document = session.document.clone();
    let name = command.params.get("name").and_then(Value::as_str).unwrap_or("Frame").to_string();
    let mut object = Object { name, pos, size, kind: crate::object::ObjectKind::PictureFrame, ..Object::default() };
    let center = [pos[0] + size[0] * 0.5, pos[1] + size[1] * 0.5];
    crate::artboard::edit::assign_new_object(&document, &mut object, center);
    let id = document.create_object(object);
    let mut frame = PictureFrame::new(id, shape);
    if let Some(path) = command.params.get("path").and_then(Value::as_str).map(str::trim).filter(|path| !path.is_empty()) {
        let decoded = crate::commands::image_place::decode_path(std::path::Path::new(path))?;
        let link = command.params.get("link").and_then(Value::as_bool).unwrap_or(false);
        let resource = crate::resources::replace_placement(&mut document, id, decoded.resource, decoded.rgba, link, Some(std::path::PathBuf::from(path)))?;
        frame.image = Some(resource);
    } else if let Some(resource) = command.params.get("resource").and_then(Value::as_u64) {
        let resource = ResourceId(resource);
        document.place_image(id, resource)?;
        frame.image = Some(resource);
    }
    let mut after_frames = before_frames.clone();
    after_frames.insert(id, frame.clone());
    let before_document = session.document.clone();
    let before_selection = session.selection.ids.clone();
    session.document = document;
    session.selection.ids = vec![id];
    set_frames(&mut session.document, after_frames.clone());
    session.undo.push(Change {
        name: FRAME_IMAGE_ADD.to_string(),
        redo: restore_command(session, true, &after_frames),
        undo: Command::with_params(FRAME_RESTORE, json!({
            "frames": frames_value(&before_frames),
            "selection": before_selection.iter().map(|item| item.0).collect::<Vec<_>>(),
            "document": serde_json::to_value(&before_document).unwrap_or(json!({}))
        })),
    });
    session.document.version += 1;
    session.generation += 1;
    let mut reply = json!({
        "doc_version": session.generation,
        "changes": [{ "name": FRAME_IMAGE_ADD }],
        "id": id.0,
        "object_id": id.0,
        "ids": [id.0]
    });
    if let (Some(target), Some(source)) = (reply.as_object_mut(), frame_payload(session, &frame).as_object()) {
        target.extend(source.clone());
    }
    Ok(reply)
}

fn require_frame(session: &Session, command: &Command) -> Result<PictureFrame, CommandError> {
    let id = target_id(session, command).ok_or_else(CommandError::invalid_params)?;
    session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    get_frame(&session.document, id).ok_or_else(CommandError::not_found)
}

fn set_fit(session: &mut Session, command: &Command) -> CommandResult {
    let mut frame = require_frame(session, command)?;
    let before = frames(&session.document);
    if command.params["fit"] == "manual" {
        freeze_layout(session, &mut frame)?;
    } else {
        frame.fit = command.params.get("fit").and_then(parse_fit).ok_or_else(CommandError::invalid_params)?;
        frame.manual_layout = None;
    }
    let mut after = before.clone();
    after.insert(frame.object, frame.clone());
    commit(session, FRAME_IMAGE_FIT, false, before, after, frame_payload(session, &frame))
}

fn set_anchor(session: &mut Session, command: &Command) -> CommandResult {
    let mut frame = require_frame(session, command)?;
    let anchor = command.params.get("anchor").and_then(parse_anchor).ok_or_else(CommandError::invalid_params)?;
    let before = frames(&session.document);
    frame.anchor = anchor;
    if let Some(layout) = &mut frame.manual_layout {
        layout.anchor_size = session.document.get_object(frame.object).ok_or_else(CommandError::not_found)?.size.map(f32::abs);
    }
    let mut after = before.clone();
    after.insert(frame.object, frame.clone());
    commit(session, FRAME_IMAGE_ANCHOR, false, before, after, frame_payload(session, &frame))
}

fn freeze_layout(session: &Session, frame: &mut PictureFrame) -> Result<(), CommandError> {
    if frame.manual_layout.is_none() {
        let size = session.document.get_object(frame.object).ok_or_else(CommandError::not_found)?.size.map(f32::abs);
        frame.manual_layout = Some(ManualLayout { fit_size: size, anchor_size: size });
    }
    Ok(())
}

fn set_content(session: &mut Session, command: &Command) -> CommandResult {
    let mut frame = require_frame(session, command)?;
    let pair = |key| command.params.get(key)
        .map(|value| serde_json::from_value::<[f32; 2]>(value.clone()).map_err(|_| CommandError::invalid_params()))
        .transpose();
    let offset = pair("offset")?;
    let scale = pair("scale")?;
    let rotation = command.params.get("rotation")
        .map(|value| value.as_f64().map(|angle| angle as f32).ok_or_else(CommandError::invalid_params)).transpose()?;
    let reset = command.params.get("reset")
        .map(|value| value.as_bool().ok_or_else(CommandError::invalid_params)).transpose()?.unwrap_or(false);
    let has_edits = offset.is_some() || scale.is_some() || rotation.is_some();
    if reset == has_edits
        || offset.is_some_and(|offset| offset.iter().any(|n| !n.is_finite()))
        || scale.is_some_and(|scale| scale.iter().any(|n| !n.is_finite() || *n <= 0.0))
        || rotation.is_some_and(|angle| !angle.is_finite())
    {
        return Err(CommandError::invalid_params());
    }
    if reset {
        frame.manual_layout = None;
        frame.content = ContentTransform::default();
    } else {
        freeze_layout(session, &mut frame)?;
        frame.content.offset = offset.unwrap_or(frame.content.offset);
        frame.content.scale = scale.unwrap_or(frame.content.scale);
        frame.content.rotation = rotation.unwrap_or(frame.content.rotation);
    }
    let before = frames(&session.document);
    let mut after = before.clone();
    after.insert(frame.object, frame.clone());
    commit(session, FRAME_IMAGE_CONTENT, false, before, after, frame_payload(session, &frame))
}

fn query(session: &Session, command: &Command) -> CommandResult {
    let frame = require_frame(session, command)?;
    Ok(frame_payload(session, &frame))
}

fn convertible(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Image | ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::PictureFrame)
}

pub(crate) fn restore_legacy_frame_kinds(document: &mut Document) {
    let legacy: Vec<_> = document.picture_frames.iter().filter_map(|frame| {
        let object = document.get_object(frame.object)?;
        (object.kind != ObjectKind::PictureFrame && convertible(&object.kind)).then_some(frame.object)
    }).collect();
    let master_before = legacy.iter().any(|&id| super::masters::owns_source(document, id))
        .then(|| document.clone());
    for id in legacy {
        if let Some(object) = document.find_object_mut(id.0) {
            object.kind = ObjectKind::PictureFrame;
        }
    }
    if let Some(before) = master_before {
        super::masters::inherit_changes(&before, document, &Command::new(FRAME_CONVERT));
    }
}

fn convert_targets(session: &Session, command: &Command) -> Result<Vec<ObjectId>, CommandError> {
    let explicit = ["object_id", "object", "node", "frame", "id"].iter().find_map(|key| command.params.get(key));
    let ids = match command.params.get("nodes") {
        Some(value) => value.as_array().ok_or_else(CommandError::invalid_params)?
            .iter().map(|item| item.as_u64().map(ObjectId).ok_or_else(CommandError::invalid_params))
            .collect::<Result<_, _>>()?,
        None => match explicit {
            Some(value) => vec![value.as_u64().map(ObjectId).ok_or_else(CommandError::invalid_params)?],
            None => session.selection.ids.clone(),
        },
    };
    if ids.is_empty() {
        return Err(CommandError::invalid_params());
    }
    for id in &ids {
        let object = session.document.get_object(*id).ok_or_else(CommandError::not_found)?;
        if !convertible(&object.kind) {
            return Err(CommandError::invalid_params());
        }
    }
    Ok(ids)
}

fn convert(session: &mut Session, command: &Command) -> CommandResult {
    let ids = convert_targets(session, command)?;
    let shape = command.params.get("shape").and_then(parse_shape).unwrap_or(FrameShape::Rect);
    let mut document = session.document.clone();
    let mut first = None;
    for id in &ids {
        let mut frame = get_frame(&document, *id).unwrap_or_else(|| PictureFrame::new(*id, shape));
        frame.shape = shape;
        frame.image = document.placed_resource(*id).or(frame.image);
        document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?.kind = ObjectKind::PictureFrame;
        if first.is_none() {
            first = Some(frame.clone());
        }
        put_frame(&mut document, frame);
    }
    let extra = first.map(|frame| frame_payload(session, &frame)).unwrap_or(json!({}));
    super::document_change::commit(session, FRAME_CONVERT, document, session.selection.ids.clone(), extra)
}
