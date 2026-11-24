use std::collections::{HashMap, HashSet};

use crate::data_merge::layout::{object_page_index, page_origin, page_rect};
use crate::document::PageId;
use crate::longdoc::anchors::LinkTarget;
use crate::longdoc::page_import;
use crate::page_fold::{FoldAxis, PageFold};
use crate::{Command, CommandError, CommandResult, Document, ObjectId, Session};
use serde_json::{json, Value};

const LIST: &str = "page.list";
const ADD: &str = "page.add";
const RENAME: &str = "page.rename";
const MOVE: &str = "page.move";
const DUPLICATE: &str = "page.duplicate";
const DELETE: &str = "page.delete";
const RESIZE: &str = "page.resize";
const SPREAD_SET: &str = "page.spread.set";
pub const COLUMN_GUIDES_SET: &str = "page.column_guides.set";
const NAVIGATE: &str = "page.navigate";
pub const MAX_ADDED_PAGES: usize = 1000;

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        LIST | ADD | RENAME | DUPLICATE | COLUMN_GUIDES_SET => Some(session.has_document),
        RESIZE | SPREAD_SET | page_import::IMPORT => Some(session.has_document && session.document.layout_artboard_origins.is_some()),
        MOVE | DELETE => Some(session.has_document && session.document.pages.len() > 1),
        NAVIGATE => Some(session.has_document && viewport(session).is_some()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command).map(|_| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        match command.name.as_str() {
            LIST => Ok(list(&session.document)),
            ADD => add(session, command),
            MOVE => move_page(session, command),
            DUPLICATE => duplicate_page(session, command),
            DELETE => delete_page(session, command),
            RESIZE => resize(session, command),
            SPREAD_SET => set_spread(session, command),
            COLUMN_GUIDES_SET => set_column_guides(session, command),
            NAVIGATE => navigate(session, command),
            page_import::IMPORT => page_import::run(session, command),
            _ => rename(session, command),
        }
    })
}

fn list(doc: &Document) -> Value {
    let pages: Vec<_> = doc.pages.iter().zip(crate::scene::page_rects(doc)).enumerate().map(|(index, (page, [x, y, width, height]))| {
        json!({
            "id": page.id,
            "master_id": doc.master_pages.for_page(page.id).map(|assignment| assignment.master),
            "artboard_id": doc.linked_artboard(page.id).map(|board| board.id),
            "number": index + 1,
            "label": crate::longdoc::sections::section_page_label(doc, index),
            "name": page.name,
            "size": [width, height],
            "origin": [x, y],
            "fold": fold_info(page.fold, [width, height]),
            "column_guides": page.column_guides,
        })
    }).collect();
    let spreads: Vec<_> = crate::data_merge::layout::spread_ranges(doc).into_iter()
        .filter(|range| range.end <= doc.pages.len()).enumerate().map(|(index, range)| {
            let pages = &doc.pages[range];
            json!({
                "number": index + 1,
                "ids": pages.iter().map(|page| page.id).collect::<Vec<_>>(),
                "custom": doc.layout_artboard_origins.is_some() && pages[0].spread_group.is_some()
                    && !(index == 0 && crate::data_merge::layout::has_right_cover(doc)),
            })
        }).collect();
    json!({"pages": pages, "spreads": spreads})
}

fn set_column_guides(session: &mut Session, command: &Command) -> CommandResult {
    let id = page_id(command)?;
    let doc = &session.document;
    let index = page_index(doc, id)?;
    let params = command.params.get("column_guides").ok_or_else(CommandError::invalid_params)?;
    let layout: Option<crate::guides::column_layout::ColumnLayout> = serde_json::from_value(params.clone())
        .map_err(|_| CommandError::invalid_params())?;
    if layout.is_some_and(|grid| grid.geometry(page_rect(doc, index), doc).is_none()) {
        return Err(CommandError::invalid_params());
    }
    if layout == doc.pages[index].column_guides {
        return Ok(json!({"id": id, "column_guides": layout, "doc_version": session.generation, "changes": []}));
    }
    let mut doc = doc.clone();
    doc.pages[index].column_guides = layout;
    let mut reply = commit(session, COLUMN_GUIDES_SET, doc, id)?;
    reply["column_guides"] = json!(layout);
    Ok(reply)
}

pub(crate) fn next_spread_group(doc: &Document) -> Result<u64, CommandError> {
    doc.pages.iter().filter_map(|page| page.spread_group).max().unwrap_or(0)
        .checked_add(1).ok_or_else(CommandError::out_of_range)
}

fn set_spread(session: &mut Session, command: &Command) -> CommandResult {
    let before = &session.document;
    if before.layout_artboard_origins.is_none() {
        return Err(CommandError::invalid_params());
    }
    let id = page_id(command)?;
    let start = page_index(before, id)?;
    let mut doc = before.clone();
    let range = match command.params.get("count") {
        None | Some(Value::Null) => {
            if before.pages[start].spread_group.is_none() {
                return Ok(json!({"id": id, "changes": [], "doc_version": session.generation}));
            }
            let range = crate::data_merge::layout::spread_ranges(before).into_iter()
                .find(|range| range.contains(&start)).ok_or_else(CommandError::not_found)?;
            for page in &mut doc.pages[range.clone()] {
                page.spread_group = None;
            }
            range
        }
        Some(value) => {
            let end = value.as_u64().and_then(|n| usize::try_from(n).ok())
                .and_then(|n| start.checked_add(n))
                .filter(|&end| end > start && end <= doc.pages.len())
                .ok_or_else(CommandError::out_of_range)?;
            if start == 0 && end > 1 && crate::data_merge::layout::has_right_cover(before) {
                return Err(CommandError::invalid_params());
            }
            let group = next_spread_group(before)?;
            if before.pages[start].spread_group.is_some() {
                let old = crate::data_merge::layout::spread_ranges(before).into_iter()
                    .find(|range| range.start == start);
                if let Some(old) = old {
                    for page in &mut doc.pages[old] {
                        page.spread_group = None;
                    }
                }
            }
            for page in &mut doc.pages[start..end] {
                page.spread_group = Some(group);
            }
            start..end
        }
    };
    let ids: Vec<_> = doc.pages[range].iter().map(|page| page.id).collect();
    preserve_content(before, &mut doc);
    let selection = session.selection.ids.clone();
    super::document_change::commit(session, SPREAD_SET, doc, selection, json!({"id": id, "ids": ids}))
}

fn fold_info(fold: Option<PageFold>, size: [f32; 2]) -> Value {
    fold.and_then(|fold| fold.positions(size).map(|positions| json!({"axis": fold.axis, "positions": positions})))
        .unwrap_or(Value::Null)
}

fn parse_name(params: &Value) -> Result<String, CommandError> {
    let name = params.as_str().ok_or_else(CommandError::invalid_params)?.trim();
    if name.chars().count() > 128 || name.chars().any(char::is_control) {
        return Err(CommandError::invalid_params());
    }
    Ok(name.to_owned())
}

fn viewport(session: &Session) -> Option<[f32; 2]> {
    session.view_canvas_size.filter(|size| size.iter().all(|side| side.is_finite() && *side > 1.0))
}

pub fn viewed_page(session: &Session) -> Option<PageId> {
    page_at_view(&session.document, &session.view, viewport(session)?)
}

fn page_at_view(doc: &Document, view: &crate::ViewState, viewport: [f32; 2]) -> Option<PageId> {
    use crate::view::ViewTarget;
    match ViewTarget::at_view_center(doc, view, viewport)? {
        ViewTarget::Page(id) => Some(id),
        ViewTarget::Artboard(id) => doc.pages.iter().find(|page| page.artboard_id == Some(id)).map(|page| page.id),
    }
}

pub fn remember_view_change(session: &mut Session, before: &crate::ViewState) {
    if session.document.layout_artboard_origins.is_none()
        || (session.view.scroll == before.scroll && session.view.zoom == before.zoom && session.view.rotation_deg == before.rotation_deg)
    {
        return;
    }
    let Some(size) = viewport(session) else { return };
    let previous = page_at_view(&session.document, before, size);
    if previous.is_some() && previous != viewed_page(session) {
        session.previous_viewed_page = previous;
    }
}

pub fn viewed_index(session: &Session) -> Option<usize> {
    let id = viewed_page(session)?;
    page_index(&session.document, id).ok()
}

pub fn previous_viewed_page(session: &Session) -> Option<PageId> {
    let id = session.previous_viewed_page?;
    (Some(id) != viewed_page(session) && page_index(&session.document, id).is_ok()).then_some(id)
}

fn navigate(session: &mut Session, command: &Command) -> CommandResult {
    let size = viewport(session).ok_or_else(CommandError::invalid_params)?;
    let doc = &session.document;
    let current = viewed_index(session);
    let index = if let Some(id) = command.params.get("id") {
        let id: PageId = serde_json::from_value(id.clone()).map_err(|_| CommandError::invalid_params())?;
        page_index(doc, id)?
    } else if let Some(number) = command.params.get("number") {
        number.as_u64().and_then(|n| usize::try_from(n).ok()).and_then(|n| n.checked_sub(1))
            .ok_or_else(CommandError::out_of_range)?
    } else {
        match command.params["direction"].as_str() {
            Some("first") => 0,
            Some("last") => doc.pages.len().saturating_sub(1),
            Some("previous") => current.unwrap_or(0).saturating_sub(1),
            Some("next") => current.map_or(0, |index| index + 1).min(doc.pages.len().saturating_sub(1)),
            Some("previously_viewed") => page_index(doc, previous_viewed_page(session).ok_or_else(CommandError::not_found)?)?,
            _ => return Err(CommandError::invalid_params()),
        }
    };
    let page = doc.pages.get(index).ok_or_else(CommandError::out_of_range)?.id;
    let target = crate::view::ViewTarget::Page(page);
    let bounds = target.bounds(doc).ok_or_else(CommandError::invalid_params)?;
    if let Some(old) = current.filter(|old| *old != index) {
        session.previous_viewed_page = Some(doc.pages[old].id);
    }
    crate::view::center_region(&mut session.view, bounds, size);
    session.view_target_hint = Some(target);
    Ok(json!({"id": page, "number": index + 1, "scroll": session.view.scroll, "zoom": session.view.zoom}))
}

fn add(session: &mut Session, command: &Command) -> CommandResult {
    let name = command.params.get("name").map(parse_name).transpose()?.unwrap_or_default();
    let count = match command.params.get("count") {
        None => 1,
        Some(count) => count.as_u64().and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_ADDED_PAGES).contains(n)).ok_or_else(CommandError::invalid_params)?,
    };

    let original = &session.document;
    let target = |key| -> Result<Option<PageId>, CommandError> {
        command.params.get(key).map(|value| serde_json::from_value(value.clone())
            .map_err(|_| CommandError::invalid_params())).transpose()
    };
    let (index, size_index) = match (target("before_id")?, target("after_id")?) {
        (Some(before), None) => {
            let index = page_index(original, before)?;
            (index, Some(index))
        }
        (None, Some(after)) => {
            let index = page_index(original, after)?;
            (index + 1, Some(index))
        }
        (None, None) => (original.pages.len(), None),
        _ => return Err(CommandError::invalid_params()),
    };
    let size = size_index.map_or_else(|| crate::data_merge::layout::page_size(original), |index| {
        let [_, _, width, height] = page_rect(original, index);
        [width, height]
    });
    let master = command.params.get("master_id").map(|id| serde_json::from_value::<ObjectId>(id.clone())
        .map_err(|_| CommandError::invalid_params())).transpose()?;
    if master.is_some_and(|id| !original.master_pages.roots.contains(&id) || original.get_object(id).is_none()) {
        return Err(CommandError::not_found());
    }

    let mut doc = original.clone();
    let custom = match command.params.get("spread") {
        None => spread_at_insertion(original, index).map(|group| (group, index..index)),
        Some(value) if original.layout_artboard_origins.is_some() => match value.as_str() {
            Some("new") => Some((next_spread_group(original)?, index..index)),
            Some("extend") => {
                let anchor = size_index.unwrap_or_else(|| original.pages.len().saturating_sub(1));
                if anchor == 0 && original.setup.facing_pages && original.setup.start_on_right {
                    return Err(CommandError::validation("panel.pages.spread_cover"));
                }
                let range = crate::data_merge::layout::spread_ranges(original).into_iter()
                    .find(|range| range.contains(&anchor) && range.end <= original.pages.len())
                    .ok_or_else(CommandError::not_found)?;
                Some((next_spread_group(original)?, range))
            }
            _ => return Err(CommandError::invalid_params()),
        },
        _ => return Err(CommandError::invalid_params()),
    };
    if let Some((group, range)) = &custom {
        for page in &mut doc.pages[range.clone()] {
            page.spread_group = Some(*group);
        }
    }
    let mut ids = Vec::with_capacity(count);
    doc.preserve_layout_page_order |= original.layout_artboard_origins.is_some();
    for offset in 0..count {
        let id = doc.alloc_page_id();
        let mut page = crate::document::Page::new(id, size);
        page.name.clone_from(&name);
        page.spread_group = custom.as_ref().map(|(group, _)| *group);
        insert_page(&mut doc, index + offset, page);
        ids.push(id);
    }
    preserve_content(original, &mut doc);
    if let Some(master) = master {
        for (offset, &id) in ids.iter().enumerate() {
            super::masters::apply_to_page(&mut doc, master, id, index + offset);
        }
    }
    let selection = session.selection.ids.clone();
    super::document_change::commit(session, ADD, doc, selection, json!({"id": ids[0], "ids": ids}))
}

pub(crate) fn spread_at_insertion(doc: &Document, index: usize) -> Option<u64> {
    if doc.layout_artboard_origins.is_none()
        || (index == 1 && doc.setup.facing_pages && doc.setup.start_on_right)
    {
        return None;
    }
    let left = doc.pages.get(index.checked_sub(1)?)?.spread_group?;
    (doc.pages.get(index)?.spread_group == Some(left)).then_some(left)
}

fn insert_page(doc: &mut Document, index: usize, page: crate::document::Page) {
    doc.pages.insert(index, page);
    for section in doc.sections.iter_mut().skip(1) {
        if section.start_page > index {
            section.start_page += 1;
        }
    }
}

fn page_index(doc: &Document, id: PageId) -> Result<usize, CommandError> {
    doc.pages.iter().position(|page| page.id == id).ok_or_else(CommandError::not_found)
}

pub fn can_resize(doc: &Document, id: PageId) -> bool {
    doc.layout_artboard_origins.is_some() && doc.linked_artboard(id).is_none_or(|board| {
        !board.locked && board.rotation_deg == 0.0 && board.shear_deg == 0.0
    })
}

fn resize(session: &mut Session, command: &Command) -> CommandResult {
    let id = page_id(command)?;
    let source = &session.document;
    let index = page_index(source, id)?;
    let size: [f32; 2] = serde_json::from_value(command.params["size"].clone())
        .map_err(|_| CommandError::invalid_params())?;
    if !can_resize(source, id) || size.iter().any(|side| !(0.01..=1_000_000.0).contains(side)) {
        return Err(CommandError::invalid_params());
    }
    let [_, _, width, height] = page_rect(source, index);
    let old_fold = source.pages[index].fold;
    let fold = match command.params.get("fold") {
        None => old_fold,
        Some(Value::Null) => None,
        Some(value) => {
            if !value.is_object() {
                return Err(CommandError::invalid_params());
            }
            let axis = match value.get("axis") {
                Some(axis) => serde_json::from_value(axis.clone()).map_err(|_| CommandError::invalid_params())?,
                None => FoldAxis::Vertical,
            };
            let fold = match value.get("positions") {
                Some(positions) => {
                    let positions = serde_json::from_value(positions.clone()).map_err(|_| CommandError::invalid_params())?;
                    PageFold::from_positions(axis, positions, size).ok_or_else(CommandError::invalid_params)?
                }
                None => PageFold::equal(axis),
            };
            Some(fold)
        }
    };
    let resizing = [width, height] != size;
    if !resizing && fold == old_fold {
        return Ok(json!({"id": id, "size": size, "fold": fold_info(fold, size), "doc_version": session.generation, "changes": []}));
    }
    let mut doc = source.clone();
    doc.pages[index].fold = fold;
    if resizing {
        doc.pages[index].size = size;
        if let Some(board) = source.linked_artboard(id) {
            let next = doc.find_object_mut(board.id.0).ok_or_else(CommandError::not_found)?;
            next.size = size;
            let after = crate::geometry::object_world_bounds(next);
            super::layer_kinds::follow_surface_resize(&mut doc, Some(board.id), crate::geometry::object_world_bounds(board), after);
        }
        let [x, y, w, h] = super::layer_kinds::page_rect(source, index).ok_or_else(CommandError::invalid_params)?;
        super::layer_kinds::follow_surface_resize(&mut doc, None, [x, y, w, h], [x, y, size[0], size[1]]);
        preserve_content(source, &mut doc);
    }
    let mut result = commit(session, RESIZE, doc, id)?;
    result["size"] = json!(size);
    result["fold"] = fold_info(fold, size);
    Ok(result)
}

fn move_page(session: &mut Session, command: &Command) -> CommandResult {
    let source = &session.document;
    let id = PageId(crate::text_cmd::param_u64(&command.params, "id")?);
    let from = page_index(source, id)?;
    let to = command.params["number"].as_u64().and_then(|n| usize::try_from(n).ok())
        .and_then(|n| n.checked_sub(1)).filter(|&n| n < source.pages.len())
        .ok_or_else(CommandError::invalid_params)?;
    if from == to {
        return Ok(json!({"id": id, "number": to + 1, "doc_version": session.generation, "changes": []}));
    }
    let mut doc = source.clone();
    let mut page = doc.pages.remove(from);
    if doc.layout_artboard_origins.is_some() {
        doc.preserve_layout_page_order = true;
        let stays_in_spread = page.spread_group.is_some() &&
            crate::data_merge::layout::spread_ranges(source).into_iter()
                .any(|range| range.contains(&from) && range.contains(&to));
        if !stays_in_spread {
            page.spread_group = spread_at_insertion(&doc, to);
        }
    }
    doc.pages.insert(to, page);
    if doc.layout_artboard_origins.is_some() && crate::data_merge::layout::has_right_cover(&doc) {
        doc.pages[0].spread_group = None;
    }
    preserve_content(source, &mut doc);
    let result = commit(session, MOVE, doc, id)?;
    let mut result = result;
    result["number"] = json!(to + 1);
    Ok(result)
}

fn page_id(command: &Command) -> Result<PageId, CommandError> {
    crate::text_cmd::param_u64(&command.params, "id").map(PageId)
}

fn content_ids(doc: &Document, index: usize) -> HashSet<ObjectId> {
    doc.objects.iter().filter(|object| {
        !super::masters::owns_source(doc, object.id) && object_page_index(doc, object.id) == index
    }).map(|object| object.id).collect()
}

fn duplicate_page(session: &mut Session, command: &Command) -> CommandResult {
    let before = &session.document;
    let source_id = page_id(command)?;
    let index = page_index(before, source_id)?;
    let source = &before.pages[index];
    let [x, y, width, height] = page_rect(before, index);
    let members = content_ids(before, index);
    let mut bundle = crate::clipboard::collect_bundle_members(before, &members);
    let excerpts = crate::longdoc::pages::restrict_page_copy(before, &mut bundle);
    let mut doc = before.clone();
    doc.preserve_layout_page_order |= before.layout_artboard_origins.is_some();
    let id = doc.alloc_page_id();
    let mut page = crate::document::Page::new(id, [width, height]);
    page.name.clone_from(&source.name);
    page.fold = source.fold;
    page.column_guides = source.column_guides;
    if index != 0 || !crate::data_merge::layout::has_right_cover(before) {
        page.spread_group = source.spread_group;
    }
    insert_page(&mut doc, index + 1, page);
    preserve_content(before, &mut doc);
    let board = before.linked_artboard(source_id);
    let offset = if board.is_some() && doc.layout_artboard_origins.is_none() {
        let right = before.objects.iter().filter(|object| crate::artboard::is_artboard(object)
            && !before.master_pages.roots.contains(&object.id)).map(|object| {
                let [x, _, w, _] = crate::geometry::object_world_bounds(object);
                x + w
            }).fold(x + width, f32::max);
        [right + crate::artboard::edit::default_spacing(before) - x, 0.0]
    } else {
        let [new_x, new_y] = page_origin(&doc, index + 1);
        [new_x - x, new_y - y]
    };
    let mapping = crate::clipboard::copy_bundle_in_document(&mut doc, &bundle, offset);
    for (&source, &copy) in &mapping {
        if let Some(object) = doc.find_object_mut(copy.0) {
            object.page_id = Some(id);
        }
        if let Some(mut frame) = super::picture_frame::get_frame(before, source) {
            frame.object = copy;
            frame.image = doc.placed_resource(copy).or(frame.image);
            super::picture_frame::put_frame(&mut doc, frame);
        }
    }
    doc.text_wraps.entries.extend(before.text_wraps.entries.iter().filter_map(|entry| {
        let mut copy = entry.clone();
        copy.object = *mapping.get(&entry.object)?;
        Some(copy)
    }));
    doc.layer_meta.extend(before.layer_meta.iter().filter_map(|entry| {
        let mut copy = entry.clone();
        copy.object = *mapping.get(&entry.object)?;
        Some(copy)
    }));
    if let Some(board) = board.and_then(|board| mapping.get(&board.id).copied()) {
        doc.pages[index + 1].artboard_id = Some(board);
        doc.artboard_backing_pages.insert(id);
        crate::artboard::edit::name_duplicate(&mut doc, board);
    }
    if let Some(assignment) = before.master_pages.for_page(source_id) {
        let mut copy = assignment.clone();
        copy.page = id;
        copy.group = assignment.group.and_then(|group| mapping.get(&group).copied());
        copy.links = assignment.links.iter().filter_map(|&(source, instance)| {
            Some((source, *mapping.get(&instance)?))
        }).collect();
        doc.master_pages.assignments.push(copy);
    }
    super::masters::lock_positions(&mut doc);
    if !before.anchors.is_empty() || !before.hyperlinks.is_empty()
        || !before.index.marks.is_empty() || !before.notes.notes.is_empty()
        || !before.toc.tocs.is_empty() || !before.index.indexes.is_empty()
    {
        use crate::longdoc::page_import::navigation::{copy, PageTransfer};
        copy(before, before, &mut doc, &bundle, &mapping, &excerpts,
            PageTransfer::Duplicate { original: source_id, copy: id })?;
    }
    let selection = session.selection.ids.clone();
    let mut guides = session.guides.clone();
    for original in session.guides.items.iter().filter(|guide| guide.page_id == Some(source_id)) {
        let mut copy = original.clone();
        copy.id = guides.next_id();
        copy.page_id = Some(id);
        if copy.name.as_deref().is_some_and(|name| name == crate::guides::Guides::default_name(original.id)) {
            copy.name = Some(crate::guides::Guides::default_name(copy.id));
        }
        guides.insert(copy).map_err(|_| CommandError::invalid_params())?;
    }
    super::document_change::commit_guides(session, DUPLICATE, doc, selection, guides, json!({"id": id}))
}

fn delete_page(session: &mut Session, command: &Command) -> CommandResult {
    let before = &session.document;
    if before.pages.len() <= 1 {
        return Err(CommandError::new("LAST_PAGE"));
    }
    let ids: Vec<PageId> = match command.params.get("ids") {
        Some(value) if command.params.get("id").is_none() => {
            serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?
        }
        None => vec![page_id(command)?],
        _ => return Err(CommandError::invalid_params()),
    };
    let removed: HashSet<_> = ids.iter().copied().collect();
    if ids.is_empty() || ids.len() != removed.len() {
        return Err(CommandError::invalid_params());
    }
    let mut indices = ids.iter().map(|id| page_index(before, *id)).collect::<Result<Vec<_>, _>>()?;
    if indices.len() >= before.pages.len() {
        return Err(CommandError::new("LAST_PAGE"));
    }
    indices.sort_unstable();
    let mut doc = before.clone();
    doc.pages.retain(|page| !removed.contains(&page.id));
    let doomed = remove_page_content(before, &mut doc, &indices, false)?;
    for section in &mut doc.sections {
        section.start_page -= indices.partition_point(|&index| index + 1 < section.start_page);
    }
    doc.sections.retain(|section| section.start_page <= doc.pages.len());
    doc.sections.reverse();
    doc.sections.dedup_by_key(|section| section.start_page);
    doc.sections.reverse();
    preserve_content(before, &mut doc);
    super::masters::lock_positions(&mut doc);
    doc.validate_refs().map_err(|_| CommandError::new("PAGE_OBJECT_REFERENCED"))?;
    let selection = session.selection.ids.iter().copied().filter(|object| !doomed.contains(object)).collect();
    let mut guides = session.guides.clone();
    guides.items.retain(|guide| !guide.page_id.is_some_and(|id| removed.contains(&id)));
    super::document_change::commit_guides(session, DELETE, doc, selection, guides, json!({"id": ids[0], "ids": ids}))
}

pub(crate) fn remove_page_content(before: &Document, doc: &mut Document, indices: &[usize], replacing: bool) -> Result<HashSet<ObjectId>, CommandError> {
    let removed: HashSet<_> = indices.iter().map(|&index| before.pages[index].id).collect();
    let doomed: HashSet<_> = before.objects.iter().filter(|object| {
        !super::masters::owns_source(before, object.id)
            && indices.binary_search(&object_page_index(before, object.id)).is_ok()
    }).map(|object| object.id).collect();
    let referenced_path = before.objects.iter().any(|object| !doomed.contains(&object.id)
        && object.path_text_data().is_some_and(|data| doomed.contains(&data.path)));
    let shared_story = replacing && before.frame_chains.iter().any(|chain| {
        chain.frames.iter().any(|id| doomed.contains(id)) && chain.frames.iter().any(|id| !doomed.contains(id))
    });
    if referenced_path || shared_story {
        return Err(CommandError::new("PAGE_OBJECT_REFERENCED"));
    }
    doc.artboard_backing_pages.retain(|id| !removed.contains(id));
    doc.master_pages.assignments.retain(|assignment| !removed.contains(&assignment.page));
    remove_layout_objects(doc, &doomed);
    for object in &mut doc.objects {
        if object.parent_id.is_some_and(|id| doomed.contains(&id)) {
            object.parent_id = None;
        }
    }
    if doc.pixel_selection.as_ref().is_some_and(|selection| selection.page_id.is_some_and(|id| removed.contains(&id))) {
        doc.pixel_selection = None;
    }
    doc.spare_channels.retain(|(_, selection)| !selection.page_id.is_some_and(|id| removed.contains(&id)));
    Ok(doomed)
}

pub(super) fn remove_layout_objects(doc: &mut Document, doomed: &HashSet<ObjectId>) {
    doc.symbols.instances.retain(|instance| !doomed.contains(&instance.root));
    super::arrange::delete_objects(doc, doomed);
    doc.picture_frames.retain(|frame| !doomed.contains(&frame.object));
    doc.text_wraps.entries.retain(|entry| !doomed.contains(&entry.object));
    doc.layer_meta.retain(|entry| !doomed.contains(&entry.object));
    doc.merge_layouts.retain(|layout| !doomed.contains(&layout.object_id));
    doc.slices.retain(|slice| !matches!(slice.source, crate::slice::SliceSource::Linked(object) if doomed.contains(&object)));
}

pub(crate) fn preserve_content(before: &Document, after: &mut Document) {
    if !after.hyperlinks.is_empty()
        && !before.pages.iter().map(|page| page.id).eq(after.pages.iter().map(|page| page.id))
    {
        let numbers: HashMap<_, _> = after.pages.iter().enumerate()
            .map(|(index, page)| (page.id, index + 1)).collect();
        after.hyperlinks.retain_mut(|link| {
            let LinkTarget::Page(number) = &mut link.target else { return true };
            let Some(previous) = number.checked_sub(1).and_then(|index| before.pages.get(index)) else {
                return true;
            };
            let Some(&next) = numbers.get(&previous.id) else { return false };
            *number = next;
            true
        });
    }
    let flow = after.layout_artboard_origins.is_some()
        .then(|| crate::data_merge::layout::flow_origins(after));
    move_page_content(before, after, |after, index| {
        flow.as_ref().map_or_else(|| page_origin(after, index), |origins| origins[index])
    });
    crate::longdoc::xref::refresh_xrefs(after);
}

pub(crate) fn normalize_layout_order(document: &mut Document) {
    if document.layout_artboard_origins.is_some() && !document.setup.arrange_vertical {
        let before = document.clone();
        document.setup.arrange_vertical = true;
        preserve_content(&before, document);
    }
}

pub(crate) fn move_page_content(
    before: &Document, after: &mut Document, origin: impl Fn(&Document, usize) -> [f32; 2],
) {
    let new_origins: HashMap<_, _> = after.pages.iter().enumerate()
        .map(|(index, page)| (page.id, origin(after, index))).collect();
    let shifts: Vec<_> = before.pages.iter().zip(crate::scene::page_rects(before)).map(|(page, [x, y, _, _])| {
        new_origins.get(&page.id).map(|moved| (page.id, [moved[0] - x, moved[1] - y]))
    }).collect();
    for object in &mut after.objects {
        if super::masters::owns_source(before, object.id) {
            continue;
        }
        let index = object_page_index(before, object.id);
        if let Some(Some((id, [dx, dy]))) = shifts.get(index).copied() {
            object.page_id = Some(id);
            object.pos[0] += dx;
            object.pos[1] += dy;
        }
    }
}

fn rename(session: &mut Session, command: &Command) -> CommandResult {
    let id = PageId(crate::text_cmd::param_u64(&command.params, "id")?);
    let name = parse_name(&command.params["name"])?;
    let page = session.document.pages.iter().find(|page| page.id == id)
        .ok_or_else(CommandError::not_found)?;
    if page.name == name {
        return Ok(json!({"id": id, "doc_version": session.generation, "changes": []}));
    }
    let mut doc = session.document.clone();
    doc.pages.iter_mut().find(|page| page.id == id).ok_or_else(CommandError::not_found)?.name = name;
    commit(session, RENAME, doc, id)
}

fn commit(session: &mut Session, name: &str, doc: Document, id: PageId) -> CommandResult {
    let selection = session.selection.ids.clone();
    super::document_change::commit(session, name, doc, selection, json!({"id": id}))
}
