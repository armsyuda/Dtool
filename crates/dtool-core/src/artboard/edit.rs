use super::{is_artboard, next_artboard_name, validate_rect};
use crate::context_toolbar::{ARTBOARD_ASPECT, ARTBOARD_HEIGHT, ARTBOARD_SOURCE, ARTBOARD_SPACING, ARTBOARD_TOOL, ARTBOARD_WIDTH};
use crate::data_merge::layout::{object_page_index, page_origin, page_rect};
use crate::geometry::object_world_bounds;
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, ObjectKind, Session};
use serde_json::{Value, json};

const TO_ARTBOARD: &str = "vector.convert_object_to_artboard";
const TO_OBJECT: &str = "vector.convert_artboard_to_object";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        "artboard.create" | "artboard.update" | "artboard.query" => Some(session.has_document),
        TO_ARTBOARD | TO_OBJECT => Some(session.has_document && conversion_target(session, command).is_some()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command).map(|_| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        match command.name.as_str() {
            "artboard.create" => create(session, &command.params),
            "artboard.update" => update(session, &command.params),
            "artboard.query" => query(&session.document, &command.params),
            _ => convert(session, command),
        }
    })
}

fn conversion_target<'a>(session: &'a Session, command: &Command) -> Option<&'a Object> {
    let id = match command.params.get("id") {
        Some(value) => ObjectId(value.as_u64()?),
        None => {
            let [id] = session.selection.ids.as_slice() else { return None };
            *id
        }
    };
    let object = session.document.get_object(id)?;
    if object.locked || object.parent_id.is_some() {
        return None;
    }
    match command.name.as_str() {
        TO_ARTBOARD if !is_artboard(object) && validate_rect(object_world_bounds(object)).is_ok() => Some(object),
        TO_OBJECT if object.kind == ObjectKind::Artboard => Some(object),
        _ => None,
    }
}

fn convert(session: &mut Session, command: &Command) -> CommandResult {
    use crate::blend::BlendMode;
    let source = conversion_target(session, command).ok_or_else(CommandError::invalid_params)?;
    let doc = &session.document;
    let mut document = doc.clone();
    let index = doc.objects.iter().position(|object| object.id == source.id).ok_or_else(CommandError::not_found)?;
    let id = if command.name == TO_ARTBOARD {
        let page = doc.pages.get(object_page_index(doc, source.id)).ok_or_else(CommandError::invalid_params)?;
        let [x, y, w, h] = object_world_bounds(source);
        let name = if source.name.trim().is_empty() { next_artboard_name(doc) } else { source.name.clone() };
        let id = document.create_object(Object {
            name, pos: [x, y], size: [w, h], kind: ObjectKind::Artboard,
            page_id: Some(page.id), blend_mode: BlendMode::PassThrough, ..Object::default()
        });
        let board = document.objects.pop().ok_or_else(CommandError::not_found)?;
        document.objects[index].parent_id = Some(id);
        document.objects.insert(index, board);
        set_subtree_page(&mut document, id, page.id);
        id
    } else {
        let has_children = document.objects.iter().any(|object| object.parent_id == Some(source.id));
        let object = &mut document.objects[index];
        if has_children {
            object.kind = ObjectKind::Group;
            if super::is_imported_artboard_name(&object.name) {
                object.name = format!("{} ({})", dtool_strings::text("artboard.group"), object.name);
            }
        } else {
            object.kind = ObjectKind::Shape;
            object.shape = None;
            if matches!(object.fill, crate::paint::Paint::None) {
                object.fill = crate::paint::Paint::Solid { colour: crate::paint::Colour::new(255, 255, 255, 255) };
            }
            object.blend_mode = BlendMode::Normal;
        }
        if let Some(page) = document.pages.iter_mut().find(|page| page.artboard_id == Some(source.id)) {
            let [_, _, w, h] = object_world_bounds(source);
            page.size = [w, h];
            page.artboard_id = None;
            document.artboard_backing_pages.remove(&page.id);
        }
        if let Some(origins) = &mut document.layout_artboard_origins {
            origins.remove(&source.id.0);
        }
        source.id
    };
    commit(session, &command.name, document, id)
}

fn query(doc: &Document, params: &Value) -> CommandResult {
    let id: Option<u64> = target_id(params)?;
    if id.is_some_and(|id| !doc.get_object(ObjectId(id)).is_some_and(|board| board.kind == ObjectKind::Artboard)) {
        return Err(CommandError::not_found());
    }
    let boards: Vec<_> = doc.objects().iter().filter(|board| board.kind == ObjectKind::Artboard
        && id.is_none_or(|id| board.id.0 == id)).map(|board| {
        let children: Vec<_> = doc.objects().iter().filter(|child| child.parent_id == Some(board.id))
            .map(|child| child.id.0).collect();
        json!({"id": board.id.0, "name": board.name, "page_id": board.page_id,
            "rect": [board.pos[0], board.pos[1], board.size[0], board.size[1]], "children": children})
    }).collect();
    Ok(json!({"artboards": boards}))
}

pub fn page_index(session: &Session) -> usize {
    session.selection.ids.first().map_or(0, |id| object_page_index(&session.document, *id))
}

pub fn page_at_point(doc: &Document, [x, y]: [f32; 2]) -> Option<usize> {
    let contains = |[left, top, width, height]: [f32; 4]| {
        x >= left && y >= top && x < left + width && y < top + height
    };
    if doc.layout_artboard_origins.is_some() {
        crate::scene::page_rects(doc).into_iter().position(contains)
    } else {
        (0..doc.pages.len()).map(|index| page_rect(doc, index)).position(contains)
    }
}

fn selected_roots(session: &Session, page: usize) -> impl Iterator<Item = &Object> {
    let doc = &session.document;
    session.selection.ids.iter().filter_map(|id| doc.get_object(*id)).filter(move |object| {
        object.parent_id.is_none() && !is_artboard(object) && !object.locked
            && object_page_index(doc, object.id) == page
    })
}

pub fn selection_bounds(session: &Session, page: usize) -> Option<[f32; 4]> {
    selected_roots(session, page).map(object_world_bounds).reduce(|a, b| {
        let x = a[0].min(b[0]);
        let y = a[1].min(b[1]);
        [x, y, (a[0] + a[2]).max(b[0] + b[2]) - x, (a[1] + a[3]).max(b[1] + b[3]) - y]
    }).and_then(|rect| validate_rect(rect).ok())
}

pub fn at_point(doc: &Document, point: [f32; 2]) -> Option<&Object> {
    crate::view::ViewTarget::artboards(doc).filter(|object| super::contains(object, point)).last()
}

pub(crate) fn assign_new_object(doc: &Document, object: &mut Object, point: [f32; 2]) {
    if let Some(board) = at_point(doc, point).filter(|board| !board.locked) {
        object.parent_id = Some(board.id);
        object.page_id = doc.pages.get(object_page_index(doc, board.id)).map(|page| page.id);
    } else if doc.layout_artboard_origins.is_some() && let Some(index) = page_at_point(doc, point) {
        object.page_id = Some(doc.pages[index].id);
    }
}

pub fn selected_artboard(session: &Session) -> Option<&Object> {
    let [id] = session.selection.ids.as_slice() else { return None };
    session.document.get_object(*id).filter(|object| object.kind == ObjectKind::Artboard)
}

pub fn is_backing_page(doc: &Document, index: usize) -> bool {
    doc.pages.get(index).is_some_and(|page| doc.artboard_backing_pages.contains(&page.id)
        && doc.objects().iter().any(|board| is_artboard(board) && board.page_id == Some(page.id)))
}

pub fn adjacent_rect(doc: &Document, board: &Object, direction: &str, spacing: f32) -> Option<[f32; 4]> {
    let other = doc.objects().iter().filter(|obj| is_artboard(obj) && obj.id != board.id);
    let [x, y, w, h] = object_world_bounds(board);
    let [bw, bh] = board.size.map(f32::abs);
    let (axis, step, mut rect) = match direction {
        "left" => (0, -1.0, [x - bw - spacing, y, bw, bh]),
        "right" => (0, 1.0, [x + w + spacing, y, bw, bh]),
        "top" => (1, -1.0, [x, y - bh - spacing, bw, bh]),
        "bottom" => (1, 1.0, [x, y + h + spacing, bw, bh]),
        _ => return None,
    };
    if !spacing.is_finite() || spacing < 0.0 { return None; }
    let dim = axis + 2;
    let boards: Vec<_> = other.map(object_world_bounds).collect();
    for _ in 0..=boards.len() {
        let edge = boards.iter().filter(|bounds| (0..2).all(|i| {
            rect[i] < bounds[i] + bounds[i + 2] && bounds[i] < rect[i] + rect[i + 2]
        })).map(|bounds| if step > 0.0 { bounds[axis] + bounds[dim] + spacing } else { bounds[axis] - rect[dim] - spacing })
            .reduce(|a, b| if step > 0.0 { a.max(b) } else { a.min(b) });
        match edge {
            Some(edge) => rect[axis] = edge,
            None => return validate_rect(rect).ok(),
        }
    }
    None
}

fn read<T: serde::de::DeserializeOwned>(params: &Value, key: &str) -> Result<T, CommandError> {
    serde_json::from_value(params[key].clone()).map_err(|_| CommandError::invalid_params())
}

fn target_id(params: &Value) -> Result<Option<u64>, CommandError> {
    read(params, if params.get("id").is_some() { "id" } else { "object_id" })
}

pub fn default_spacing(doc: &Document) -> f32 {
    (20.0 / 25.4 * 72.0) * crate::units::units_per_point(doc.units, doc.dpi)
}

fn rightmost_edge(doc: &Document, page: usize) -> Option<f32> {
    doc.objects().iter().filter(|object| is_artboard(object) && object_page_index(doc, object.id) == page)
        .map(|object| {
            let [x, _, width, _] = object_world_bounds(object);
            x + width
        }).filter(|edge| edge.is_finite()).reduce(f32::max)
}

pub(crate) fn duplicate_offset(doc: &Document, board: &Object) -> [f32; 2] {
    let [left, _, width, _] = object_world_bounds(board);
    let edge = rightmost_edge(doc, object_page_index(doc, board.id)).unwrap_or(left + width);
    [edge + default_spacing(doc) - left, 0.0]
}

pub(crate) fn name_duplicate(doc: &mut Document, id: ObjectId) {
    let Some(board) = doc.get_object(id).filter(|object| object.kind == ObjectKind::Artboard) else { return };
    let name = if board.name.trim().is_empty() || super::is_imported_artboard_name(&board.name) {
        next_artboard_name(doc)
    } else {
        (2..=doc.objects().len() + 2).map(|index| format!("{} {index}", board.name))
            .find(|name| !doc.objects().iter().any(|object| object.name == *name))
            .unwrap_or_else(|| next_artboard_name(doc))
    };
    let page_id = board.page_id;
    if let Some(board) = doc.find_object_mut(id.0) {
        board.name = name;
    }
    if let Some(page_id) = page_id {
        set_subtree_page(doc, id, page_id);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolSettings {
    pub source: String,
    pub size: [f32; 2],
    pub spacing: f32,
    pub aspect: bool,
}

impl ToolSettings {
    pub fn read(session: &Session, page: usize) -> Self {
        let options = &session.tool_options;
        let defaults = session.document.pages.get(page).map_or([100.0, 100.0], |page| page.size);
        let number = |id, fallback| options.explicit_number(ARTBOARD_TOOL, id)
            .map_or(fallback, |value| value as f32);
        Self {
            source: options.choice(ARTBOARD_TOOL, ARTBOARD_SOURCE).unwrap_or_else(|| "document".into()),
            size: [number(ARTBOARD_WIDTH, defaults[0]), number(ARTBOARD_HEIGHT, defaults[1])],
            spacing: number(ARTBOARD_SPACING, default_spacing(&session.document)),
            aspect: options.flag(ARTBOARD_TOOL, ARTBOARD_ASPECT).unwrap_or(false),
        }
    }
}

fn create(session: &mut Session, params: &Value) -> CommandResult {
    let doc = &session.document;
    let default_settings = ToolSettings::read(session, page_index(session));
    let source = match params.get("source").or_else(|| params.get("from")) {
        Some(value) => value.as_str().ok_or_else(CommandError::invalid_params)?,
        None => default_settings.source.as_str(),
    };
    let name = read::<Option<String>>(params, "name")?;
    if name.as_ref().is_some_and(|name| name.trim().is_empty()) {
        return Err(CommandError::invalid_params());
    }
    let canvas_rect = (source == "drag").then(|| read::<[f32; 4]>(params, "rect").and_then(validate_rect)).transpose()?;
    let requested_page: Option<usize> = read(params, "page_index")?;
    let containing_page = canvas_rect.and_then(|[x, y, _, _]| page_at_point(doc, [x, y]));
    let relative = if source == "adjacent" {
        let id = ObjectId(target_id(params)?.ok_or_else(CommandError::invalid_params)?);
        Some(doc.get_object(id).filter(|object| is_artboard(object)).ok_or_else(CommandError::not_found)?)
    } else { None };
    let index = relative.map(|board| object_page_index(doc, board.id))
        .or(requested_page).or(containing_page).unwrap_or_else(|| page_index(session));
    let page = doc.pages.get(index).ok_or_else(CommandError::invalid_params)?;
    let origin = page_origin(doc, index);
    let rect = match source {
        "page" if !is_backing_page(doc, index) => [origin[0], origin[1], page.size[0], page.size[1]],
        "adjacent" => {
            let board = relative.ok_or_else(CommandError::invalid_params)?;
            let direction = params["direction"].as_str().ok_or_else(CommandError::invalid_params)?;
            adjacent_rect(doc, board, direction, ToolSettings::read(session, index).spacing)
                .ok_or_else(CommandError::invalid_params)?
        }
        "drag" => canvas_rect.ok_or_else(CommandError::invalid_params)?,
        "selection" => selection_bounds(session, index).ok_or_else(CommandError::invalid_params)?,
        "document" | "custom" => {
            let settings = ToolSettings::read(session, index);
            let [w, h] = if source == "custom" { read(params, "size").map(|size: Option<[f32; 2]>| size.unwrap_or(settings.size))? } else { page.size };
            let spacing: Option<f32> = read(params, "spacing")?;
            let spacing = spacing.unwrap_or(settings.spacing);
            if !spacing.is_finite() || spacing < 0.0 {
                return Err(CommandError::invalid_params());
            }
            let right = rightmost_edge(doc, index);
            [right.map_or(origin[0], |edge| edge + spacing), origin[1], w, h]
        }
        _ => return Err(CommandError::invalid_params()),
    };
    let [x, y, w, h] = validate_rect(rect)?;
    let children: Vec<_> = if source == "adjacent" {
        Vec::new()
    } else if source == "selection" {
        selected_roots(session, index).map(|object| object.id).collect()
    } else {
        doc.objects().iter().filter(|object| {
            if object.parent_id.is_some() || is_artboard(object) || (object.locked && source != "page")
                || object_page_index(doc, object.id) != index {
                return false;
            }
            if source == "page" { return true; }
            let [left, top, width, height] = object_world_bounds(object);
            left >= x && top >= y && left + width <= x + w && top + height <= y + h
        }).map(|object| object.id).collect()
    };

    let mut document = doc.clone();
    let existing = (source == "page").then(|| doc.objects.iter().find(|board|
        board.kind == ObjectKind::Artboard && !board.locked && board.page_id == Some(page.id)
            && board.pos == [x, y] && board.size == [w, h] && board.rotation_deg == 0.0 && board.shear_deg == 0.0
    )).flatten();
    let id = existing.map(|board| board.id).unwrap_or_else(|| document.create_object(Object {
        name: name.unwrap_or_else(|| next_artboard_name(doc)), kind: ObjectKind::Artboard,
        blend_mode: crate::blend::BlendMode::PassThrough,
        pos: [x, y], size: [w, h], page_id: Some(page.id), ..Object::default()
    }));
    for child in children {
        if let Some(object) = document.find_object_mut(child.0) {
            object.parent_id = Some(id);
        }
    }
    if source == "page" {
        let board_name = document.get_object(id).ok_or_else(CommandError::not_found)?.name.clone();
        let converted = &mut document.pages[index];
        converted.artboard_id = Some(id);
        if converted.name.is_empty() {
            converted.name = board_name;
        }
        document.artboard_backing_pages.insert(page.id);
    }
    set_subtree_page(&mut document, id, page.id);
    commit(session, "artboard.create", document, id)
}

fn update(session: &mut Session, params: &Value) -> CommandResult {
    if let Some(ids) = read::<Option<Vec<ObjectId>>>(params, "ids")? {
        let width = read::<Option<f32>>(params, "width")?;
        let height = read::<Option<f32>>(params, "height")?;
        let name = read::<Option<String>>(params, "name")?;
        if ids.is_empty() || name.as_ref().is_some_and(|n| ids.len() != 1 || n.trim().is_empty()) {
            return Err(CommandError::invalid_params());
        }
        let mut document = session.document.clone();
        let mut changed = false;
        for id in &ids {
            let board = document.find_object_mut(id.0).filter(|board| board.kind == ObjectKind::Artboard && !board.locked)
                .ok_or_else(CommandError::invalid_params)?;
            let before = object_world_bounds(board);
            let size = [width.unwrap_or(board.size[0]), height.unwrap_or(board.size[1])];
            validate_rect([board.pos[0], board.pos[1], size[0], size[1]])?;
            changed |= size != board.size || name.as_ref().is_some_and(|name| &board.name != name);
            board.size = size;
            if let Some(name) = &name { board.name.clone_from(name); }
            let after = object_world_bounds(board);
            crate::commands::layer_kinds::follow_surface_resize(&mut document, Some(*id), before, after);
        }
        return if changed {
            crate::commands::document_change::commit(session, "artboard.update", document, ids.clone(), json!({ "ids": ids }))
        } else {
            Ok(json!({ "ids": ids, "changes": [], "doc_version": session.generation }))
        };
    }
    let id = ObjectId(target_id(params)?.ok_or_else(CommandError::invalid_params)?);
    let mut document = session.document.clone();
    let board = document.get_object(id).ok_or_else(CommandError::not_found)?;
    if board.kind != ObjectKind::Artboard || board.locked {
        return Err(CommandError::invalid_params());
    }
    let rect: Option<[f32; 4]> = read(params, "rect")?;
    let name: Option<String> = read(params, "name")?;
    if rect.is_none() && name.is_none() || name.as_ref().is_some_and(|name| name.trim().is_empty()) {
        return Err(CommandError::invalid_params());
    }
    let [x, y, w, h] = rect.map(validate_rect).transpose()?.unwrap_or([board.pos[0], board.pos[1], board.size[0], board.size[1]]);
    let [dx, dy] = [x - board.pos[0], y - board.pos[1]];
    if [x, y] == board.pos && [w, h] == board.size && name.as_ref().is_none_or(|name| name == &board.name) {
        return Ok(json!({"id": id.0, "doc_version": session.generation, "changes": []}));
    }
    let resize: Option<bool> = read(params, "resize")?;
    let move_contents = !resize.unwrap_or(false) && (dx != 0.0 || dy != 0.0);
    let mut before = object_world_bounds(board);
    if move_contents {
        let descendants = document.subtree_ids(id);
        for object in document.objects.iter_mut().filter(|object| object.id != id && descendants.contains(&object.id)) {
            object.pos[0] += dx;
            object.pos[1] += dy;
        }
        before[0] += dx;
        before[1] += dy;
    }
    let board = document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    board.pos = [x, y];
    board.size = [w, h];
    if let Some(name) = name {
        board.name = name;
    }
    let after = object_world_bounds(board);
    crate::commands::layer_kinds::follow_surface_resize(&mut document, Some(id), before, after);
    commit(session, "artboard.update", document, id)
}

pub(crate) fn set_subtree_page(document: &mut Document, root: ObjectId, page: crate::document::PageId) {
    let ids = document.subtree_ids(root);
    for object in document.objects.iter_mut().filter(|object| ids.contains(&object.id)) {
        object.page_id = Some(page);
    }
}

fn commit(session: &mut Session, command: &str, document: Document, id: ObjectId) -> CommandResult {
    crate::commands::document_change::commit(session, command, document, vec![id], json!({"id": id.0}))
}
