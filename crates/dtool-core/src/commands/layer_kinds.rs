use crate::command::Session;
use crate::commands::arrange::{commit_snapshot, snapshot};
use crate::document::Document;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::paint::{Colour, Paint, PatternPaint};
use crate::resources::ResourceId;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const LAYER_FILL_ADD: &str = "layer.fill.add";
pub const LAYER_PATTERN_ADD: &str = "layer.pattern.add";
pub const LAYER_PIXEL_ADD: &str = "layer.pixel.add";
pub const PIXEL_LAYER_NAME: &str = "Pixel Layer";
const MAX_PIXEL_SIDE: u32 = 32768;
pub const FILL_LAYER_NAME: &str = "Fill Layer";
pub const PATTERN_LAYER_NAME: &str = "Pattern Layer";
const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        LAYER_FILL_ADD => Some(session.has_document && layer_surface(session, &command.params).is_ok()
            && fill_paint(&command.params).is_ok()),
        LAYER_PATTERN_ADD => Some(session.has_document && pattern_resource(&session.document, &command.params).is_ok()),
        LAYER_PIXEL_ADD => Some(session.has_document && layer_surface(session, &command.params)
            .and_then(|surface| surface.pixel_dimensions(&session.document)).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        LAYER_FILL_ADD => Some(add_fill(session, &command.params)),
        LAYER_PATTERN_ADD => Some(add_pattern(session, &command.params)),
        LAYER_PIXEL_ADD => Some(add_pixel(session, &command.params)),
        _ => None,
    }
}

pub fn page_rect(document: &Document, index: usize) -> Option<[f32; 4]> {
    crate::scene::page_rects(document).get(index).copied()
}

fn page_index(document: &Document, params: &Value) -> Result<usize, CommandError> {
    let index = match params.get("page") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().ok_or_else(CommandError::invalid_params)? as usize,
    };
    let rect = page_rect(document, index).ok_or_else(CommandError::invalid_params)?;
    if rect[2] > 0.0 && rect[3] > 0.0 && rect.iter().all(|v| v.is_finite()) { Ok(index) } else { Err(CommandError::invalid_params()) }
}

fn fill_paint(params: &Value) -> Result<Paint, CommandError> {
    match (params.get("paint"), params.get("colour")) {
        (Some(paint), None) => {
            let paint: Paint = serde_json::from_value(paint.clone()).map_err(|_| CommandError::invalid_params())?;
            if paint == Paint::None { Err(CommandError::invalid_params()) } else { Ok(paint) }
        }
        (None, Some(Value::String(hex))) => match crate::paint::normalize_legacy_paint_alias(Some(hex)) {
            Ok(Paint::None) | Err(_) => Err(CommandError::invalid_params()),
            Ok(paint) => Ok(paint),
        },
        (None, None) => Ok(Paint::Solid { colour: Colour::new(255, 255, 255, 255) }),
        _ => Err(CommandError::invalid_params()),
    }
}

fn pattern_resource(document: &Document, params: &Value) -> Result<ResourceId, CommandError> {
    let id = ResourceId(params.get("resource").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?);
    document.image_resource(id).ok_or_else(CommandError::not_found)?;
    page_index(document, params)?;
    Ok(id)
}

fn layer(document: &Document, index: usize, name: &str, fill: Paint) -> Object {
    let rect = page_rect(document, index).unwrap_or([0.0, 0.0, 0.0, 0.0]);
    Object {
        name: name.to_string(),
        pos: [rect[0], rect[1]],
        size: [rect[2], rect[3]],
        fill,
        kind: ObjectKind::Shape,
        page_id: document.pages.get(index).map(|page| page.id),
        ..Object::default()
    }
}

fn add_fill(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let surface = layer_surface(session, params)?;
    let paint = fill_paint(params)?;
    let before = snapshot(session);
    let mut object = layer(&session.document, surface.page, FILL_LAYER_NAME, paint);
    object.fill_layer = true;
    surface.place(&mut object);
    let id = insert_above_selection(&mut session.document, &session.selection.ids, object);
    session.selection.ids = vec![id];
    commit_snapshot(session, LAYER_FILL_ADD, before, json!({"id": id.0, "ids": [id.0], "page": surface.page, "follows_page": true}))
}

fn add_pattern(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let resource = pattern_resource(&session.document, params)?;
    let index = page_index(&session.document, params)?;
    let paint = Paint::Pattern(PatternPaint { resource_id: resource.0.to_string(), matrix: IDENTITY });
    let before = snapshot(session);
    let object = layer(&session.document, index, PATTERN_LAYER_NAME, paint);
    let id = insert_above_selection(&mut session.document, &session.selection.ids, object);
    if let Err(error) = session.document.place_image(id, resource) {
        crate::commands::arrange::restore(session, &before);
        return Err(error);
    }
    session.selection.ids = vec![id];
    commit_snapshot(session, LAYER_PATTERN_ADD, before, json!({"id": id.0, "ids": [id.0], "page": index, "resource": resource.0, "follows_page": true}))
}

struct LayerSurface {
    page: usize,
    board: Option<ObjectId>,
    rect: [f32; 4],
}

impl LayerSurface {
    fn place(&self, object: &mut Object) {
        let [x, y, width, height] = self.rect;
        object.parent_id = self.board;
        object.pos = [x, y];
        object.size = [width, height];
    }

    fn pixel_dimensions(&self, document: &Document) -> Result<[u32; 2], CommandError> {
        pixel_dimensions(document, [self.rect[2], self.rect[3]])
    }
}

fn layer_surface(session: &Session, params: &Value) -> Result<LayerSurface, CommandError> {
    use crate::view::ViewTarget;
    let doc = &session.document;
    let active = params.get("current_surface").and_then(Value::as_bool) == Some(true) && params.get("page").is_none();
    let mut page = 0;
    let mut board = None;
    if active {
        let in_view = || session.view_canvas_size.and_then(|size| {
            ViewTarget::at_view_center_with_hint(doc, &session.view, size, session.view_target_hint)
        }).or(session.view_target_hint);
        if crate::commands::persona::layout_active() && doc.layout_artboard_origins.is_some() {
            board = match in_view() {
                Some(ViewTarget::Artboard(id)) if doc.master_pages.roots.contains(&id) => doc.get_object(id),
                _ => None,
            };
            if let Some(id) = crate::commands::pages::viewed_page(session) {
                page = doc.pages.iter().position(|candidate| candidate.id == id).unwrap_or(0);
            }
        } else {
            let target = session.selection.key().and_then(|id| crate::artboard::owner_or_self(doc, id))
                .map(|board| ViewTarget::Artboard(board.id)).or_else(in_view);
            match target {
                Some(ViewTarget::Artboard(id)) => board = doc.get_object(id).filter(|object| crate::artboard::is_artboard(object)),
                Some(ViewTarget::Page(id)) => page = doc.pages.iter().position(|candidate| candidate.id == id).unwrap_or(0),
                None => {}
            }
        }
    } else {
        page = page_index(doc, params)?;
    }
    if board.is_some_and(|board| board.locked || !board.pickable) {
        return Err(CommandError::new("LOCKED"));
    }
    if let Some(board) = board {
        page = crate::data_merge::layout::object_page_index(doc, board.id);
    }
    let rect = board.map(crate::geometry::object_world_bounds)
        .or_else(|| page_rect(doc, page)).ok_or_else(CommandError::invalid_params)?;
    Ok(LayerSurface { page, board: board.map(|board| board.id), rect })
}

pub(crate) fn pixel_dimensions(document: &Document, [width, height]: [f32; 2]) -> Result<[u32; 2], CommandError> {
    let side = |value: f32| -> Result<u32, CommandError> {
        let rounded = crate::units::to_px(value.abs(), document.units, document.dpi)
            .ok_or_else(CommandError::invalid_params)?.round().max(1.0);
        if rounded >= 1.0 && rounded <= MAX_PIXEL_SIDE as f32 { Ok(rounded as u32) } else { Err(CommandError::invalid_params()) }
    };
    Ok([side(width)?, side(height)?])
}

pub(crate) fn pixel_object(document: &Document, index: usize, size: [u32; 2]) -> Object {
    let mut object = layer(document, index, PIXEL_LAYER_NAME, Paint::None);
    object.kind = ObjectKind::Image;
    object.raster_size = Some((size[0], size[1]));
    object
}

fn add_pixel(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let surface = layer_surface(session, params)?;
    let [width, height] = surface.pixel_dimensions(&session.document)?;
    let before = snapshot(session);
    let mut object = pixel_object(&session.document, surface.page, [width, height]);
    surface.place(&mut object);
    let id = insert_above_selection(&mut session.document, &session.selection.ids, object);
    session.document.pixel_layers.insert(id, crate::pixel::PixelLayer::new(width, height));
    session.selection.ids = vec![id];
    commit_snapshot(session, LAYER_PIXEL_ADD, before, json!({"id": id.0, "ids": [id.0], "page": surface.page, "pixel_size": [width, height]}))
}

fn root_of(document: &Document, id: ObjectId) -> ObjectId {
    let mut current = id;
    let mut steps = 0;
    while let Some(parent) = document.get_object(current).and_then(|object| object.parent_id) {
        current = parent;
        steps += 1;
        if steps > document.objects.len() { break; }
    }
    current
}

pub fn insert_above_selection(document: &mut Document, selection: &[ObjectId], object: Object) -> ObjectId {
    let anchor = document.objects.iter().enumerate().filter(|(_, item)| selection.contains(&item.id)).map(|(index, item)| (index, item.id)).last();
    let id = document.create_object(object);
    let Some((_, anchor)) = anchor else { return id };
    let root = root_of(document, anchor);
    let Some(last) = document.objects.iter().rposition(|item| item.id != id && root_of(document, item.id) == root) else { return id };
    let created = document.objects.pop().expect("created object");
    document.objects.insert(last + 1, created);
    id
}

pub fn is_fill_layer(object: &Object) -> bool {
    object.shape.is_none() && matches!(object.kind, ObjectKind::Shape)
        && (object.fill_layer || object.name == FILL_LAYER_NAME)
}

fn is_full_surface_layer(object: &Object) -> bool {
    object.shape.is_none()
        && object.rotation_deg == 0.0
        && object.shear_deg == 0.0
        && matches!(object.kind, ObjectKind::Shape)
        && (is_fill_layer(object) || (object.name == PATTERN_LAYER_NAME && matches!(object.fill, Paint::Pattern(_))))
}

pub fn is_page_layer(object: &Object) -> bool {
    object.parent_id.is_none() && is_full_surface_layer(object)
}

pub(crate) fn follow_surface_resize(document: &mut Document, parent: Option<ObjectId>, before: [f32; 4], after: [f32; 4]) -> bool {
    if before == after { return false; }
    let mut changed = false;
    for object in document.objects.iter_mut().filter(|object| object.parent_id == parent && is_full_surface_layer(object)) {
        if object.pos == [before[0], before[1]] && object.size == [before[2], before[3]] {
            object.pos = [after[0], after[1]];
            object.size = [after[2], after[3]];
            changed = true;
        }
    }
    changed
}

pub(crate) struct FillLayerFollowers(Vec<(ObjectId, crate::view::ViewTarget)>);

impl FillLayerFollowers {
    pub(crate) fn collect(document: &Document) -> Self {
        use crate::view::ViewTarget;
        let mut followers = Vec::new();
        for object in document.objects().iter().filter(|object| is_full_surface_layer(object)) {
            let target = if let Some(parent) = object.parent_id {
                document.get_object(parent).filter(|parent| crate::artboard::is_artboard(parent))
                    .map(|parent| ViewTarget::Artboard(parent.id))
            } else {
                let index = crate::data_merge::layout::object_page_index(document, object.id);
                document.pages.get(index).map(|page| ViewTarget::Page(page.id))
            };
            if let Some(target) = target
                && let Some([x, y, w, h]) = layer_target_bounds(document, target)
                && object.pos == [x, y] && object.size == [w, h]
            {
                followers.push((object.id, target));
            }
        }
        Self(followers)
    }

    pub(crate) fn update(self, document: &mut Document) {
        for (id, target) in self.0 {
            if let Some([x, y, w, h]) = layer_target_bounds(document, target)
                && let Some(object) = document.find_object_mut(id.0)
            {
                object.pos = [x, y];
                object.size = [w, h];
            }
        }
    }
}

fn layer_target_bounds(document: &Document, target: crate::view::ViewTarget) -> Option<[f32; 4]> {
    match target {
        crate::view::ViewTarget::Page(id) => {
            let index = document.pages.iter().position(|page| page.id == id)?;
            page_rect(document, index)
        }
        crate::view::ViewTarget::Artboard(id) => document.get_object(id).map(crate::geometry::object_world_bounds),
    }
}

pub fn follow_page_resize(document: &mut Document, old_rects: &[[f32; 4]]) -> Vec<ObjectId> {
    let new_rects = crate::scene::page_rects(document);
    let mut moved = Vec::new();
    for object in document.objects.iter_mut().filter(|object| is_page_layer(object)) {
        let Some(index) = old_rects.iter().position(|r| [r[0], r[1]] == object.pos && [r[2], r[3]] == object.size) else { continue };
        let Some(rect) = new_rects.get(index) else { continue };
        if [rect[0], rect[1]] != object.pos || [rect[2], rect[3]] != object.size {
            object.pos = [rect[0], rect[1]];
            object.size = [rect[2], rect[3]];
            moved.push(object.id);
        }
    }
    moved
}
