use std::collections::{HashMap, HashSet};

use crate::data_merge::layout::{object_page_index, page_origin, page_rect};
use crate::document::PageId;
use crate::longdoc::anchors::LinkTarget;
use crate::longdoc::page_import;
use crate::page_fold::{FoldAxis, PageFold};
use crate::{Command, CommandError, CommandResult, Document, ObjectId, Session};
use serde_json::{json, Value};

pub mod spreads;

const LIST: &str = "page.list";
const ADD: &str = "page.add";
const RENAME: &str = "page.rename";
const SET_TAG: &str = "page.tag.set";
const MOVE_OPTIONS: &str = "page.move_options.set";
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
        RESIZE | SPREAD_SET | SET_TAG | MOVE_OPTIONS | spreads::REFLOW | page_import::IMPORT => {
            Some(session.has_document && session.document.layout_artboard_origins.is_some())
        }
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
            SET_TAG => set_tag(session, command),
            MOVE_OPTIONS => set_move_options(session, command),
            SPREAD_SET => set_spread(session, command),
            spreads::REFLOW => spreads::set_reflow(session, command),
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
            "master_ids": doc.master_pages.on_page(page.id).map(|assignment| assignment.master).collect::<Vec<_>>(),
            "artboard_id": doc.linked_artboard(page.id).map(|board| board.id),
            "number": index + 1,
            "label": crate::longdoc::sections::section_page_label(doc, index),
            "name": page.name,
            "tag_colour": page.tag_colour,
            "size": [width, height],
            "origin": [x, y],
            "fold": fold_info(page.fold, [width, height]),
            "column_guides": page.column_guides,
        })
    }).collect();
    let spreads: Vec<_> = crate::data_merge::layout::spread_ranges(doc).into_iter()
        .filter(|range| range.end <= doc.pages.len()).enumerate().map(|(index, range)| {
            let pages = &doc.pages[range.clone()];
            let fixed = spreads::fixed_spread(doc, &range);
            json!({
                "number": index + 1,
                "ids": pages.iter().map(|page| page.id).collect::<Vec<_>>(),
                "spine": crate::data_merge::layout::spread_spine(doc, &range),
                "custom": spreads::is_custom(doc, &range)
                    && !(index == 0 && crate::data_merge::layout::has_right_cover(doc)),
                "slots": fixed.map(|spread| &spread.slots)
            })
        }).collect();
    json!({"pages": pages, "spreads": spreads, "reflow": doc.fixed_spreads.is_none(),
        "anchor_to_spine": doc.page_move_anchor_to_spine, "master_policy": doc.page_move_master_policy})
}

fn set_move_options(session: &mut Session, command: &Command) -> CommandResult {
    let before = &session.document;
    if before.layout_artboard_origins.is_none() {
        return Err(CommandError::invalid_params());
    }
    let params = &command.params;
    if params.get("anchor_to_spine").is_none() && params.get("master_policy").is_none() {
        return Err(CommandError::invalid_params());
    }
    let anchor = params.get("anchor_to_spine")
        .map(|value| value.as_bool().ok_or_else(CommandError::invalid_params)).transpose()?
        .unwrap_or(before.page_move_anchor_to_spine);
    let policy = params.get("master_policy").map(|value| serde_json::from_value(value.clone()))
        .transpose().map_err(|_| CommandError::invalid_params())?.unwrap_or(before.page_move_master_policy);
    if before.page_move_anchor_to_spine == anchor && before.page_move_master_policy == policy {
        return Ok(json!({"anchor_to_spine": anchor, "master_policy": policy,
            "doc_version": session.generation, "changes": []}));
    }
    let mut doc = before.clone();
    doc.page_move_anchor_to_spine = anchor;
    doc.page_move_master_policy = policy;
    super::document_change::commit(session, MOVE_OPTIONS, doc, session.selection.ids.clone(),
        json!({"anchor_to_spine": anchor, "master_policy": policy}))
}

fn set_tag(session: &mut Session, command: &Command) -> CommandResult {
    let before = &session.document;
    if before.layout_artboard_origins.is_none() {
        return Err(CommandError::invalid_params());
    }
    let colour = super::layer_meta::read_tag_colour(&command.params)?;
    let ids: Vec<PageId> = serde_json::from_value(command.params["ids"].clone())
        .map_err(|_| CommandError::invalid_params())?;
    let indices = selected_page_indices(before, &ids)?;
    let reply = json!({"ids": ids, "tag_colour": colour});
    if indices.iter().all(|&index| before.pages[index].tag_colour == colour) {
        return Ok(json!({"ids": ids, "tag_colour": colour, "doc_version": session.generation, "changes": []}));
    }
    let mut doc = before.clone();
    for index in indices {
        doc.pages[index].tag_colour.clone_from(&colour);
    }
    super::document_change::commit(session, SET_TAG, doc, session.selection.ids.clone(), reply)
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
                page.spread_spine = None;
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
                        page.spread_spine = None;
                    }
                }
            }
            for page in &mut doc.pages[start..end] {
                page.spread_group = Some(group);
                page.spread_spine = None;
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
    let mut count = match command.params.get("count") {
        None => 1,
        Some(count) => count.as_u64().and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_ADDED_PAGES).contains(n)).ok_or_else(CommandError::invalid_params)?,
    };

    let original = &session.document;
    let target = |key| -> Result<Option<PageId>, CommandError> {
        command.params.get(key).map(|value| serde_json::from_value(value.clone())
            .map_err(|_| CommandError::invalid_params())).transpose()
    };
    let fill = command.params.get("vacancy").map(|value| {
        if count != 1 || ["before_id", "after_id", "spread", "conform_layout"].iter()
            .any(|key| command.params.get(key).is_some()) {
            return Err(CommandError::invalid_params());
        }
        spreads::fill_target(original, value, count)
    }).transpose()?;
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
    let index = fill.as_ref().map_or(index, |fill| fill.index);
    let mut size = size_index.map_or_else(|| crate::data_merge::layout::page_size(original), |index| {
        let [_, _, width, height] = page_rect(original, index);
        [width, height]
    });
    if let Some(fill) = &fill {
        size = fill.size;
    }
    let master = command.params.get("master_id").map(|id| serde_json::from_value::<ObjectId>(id.clone())
        .map_err(|_| CommandError::invalid_params())).transpose()?;
    if master.is_some_and(|id| !original.master_pages.roots.contains(&id) || original.get_object(id).is_none()) {
        return Err(CommandError::not_found());
    }
    let conform_layout = command.params.get("conform_layout").map(|value|
        value.as_bool().ok_or_else(CommandError::invalid_params)).transpose()?.unwrap_or(false);
    let mut spread_template = None;
    let mut template_spine = None;
    if conform_layout {
        let root = master.and_then(|id| original.get_object(id)).ok_or_else(CommandError::invalid_params)?;
        if command.params["spread"] == "new"
            && let Some(spread) = original.master_pages.spread(root.id)
        {
            let pages = spread.pages.iter().map(|&id| {
                let object = original.get_object(id).ok_or_else(CommandError::not_found)?;
                valid_page_size(object.size).then_some((id, object.size)).ok_or_else(CommandError::invalid_params)
            }).collect::<Result<Vec<_>, _>>()?;
            if command.params.get("count").is_some() && count != pages.len() {
                return Err(CommandError::invalid_params());
            }
            count = pages.len();
            spread_template = Some(pages);
            template_spine = Some(spread.spine);
        } else {
            size = root.size;
            if !valid_page_size(size) {
                return Err(CommandError::invalid_params());
            }
        }
    }

    let mut doc = original.clone();
    let custom = match command.params.get("spread") {
        None => fill.as_ref().map(|fill| fill.group).or_else(|| spread_at_insertion(original, index))
            .map(|group| (group, index..index)),
        Some(value) if original.layout_artboard_origins.is_some() => match value.as_str() {
            Some("new") => Some((next_spread_group(original)?, index..index)),
            Some("extend") => {
                let anchor = size_index.unwrap_or_else(|| original.pages.len().saturating_sub(1));
                if anchor == 0 && crate::data_merge::layout::has_right_cover(original) {
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
        let page_size = spread_template.as_ref().map_or(size, |pages| pages[offset].1);
        let mut page = crate::document::Page::new(id, page_size);
        page.name.clone_from(&name);
        page.spread_group = custom.as_ref().map(|(group, _)| *group);
        page.spread_spine = template_spine;
        if let Some(fill) = &fill {
            fill.set_page(&mut doc, offset, id, page_size);
        }
        insert_page(&mut doc, index + offset, page);
        ids.push(id);
    }
    preserve_content(original, &mut doc);
    if let Some(master) = master {
        let targets: Vec<_> = ids.iter().copied().enumerate().map(|(offset, id)| (index + offset, id)).collect();
        let sources = if let Some(template) = spread_template {
            template.into_iter().map(|(id, _)| id).collect()
        } else {
            super::masters::application_sources(&doc, master, &targets, None)?
        };
        for (&id, source) in ids.iter().zip(sources) {
            super::masters::apply_to_page(&mut doc, source, id);
        }
        super::masters::spread_flow::stitch_new(original, &mut doc);
    }
    let selection = session.selection.ids.clone();
    super::document_change::commit(session, ADD, doc, selection, json!({"id": ids[0], "ids": ids}))
}

pub(crate) fn spread_at_insertion(doc: &Document, index: usize) -> Option<u64> {
    if doc.layout_artboard_origins.is_none()
        || (index == 1 && crate::data_merge::layout::has_right_cover(doc))
    {
        return None;
    }
    let left = doc.pages.get(index.checked_sub(1)?)?.spread_group?;
    (doc.pages.get(index)?.spread_group == Some(left)).then_some(left)
}

pub(super) fn insert_page(doc: &mut Document, index: usize, page: crate::document::Page) {
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

pub fn valid_page_size(size: [f32; 2]) -> bool {
    size.iter().all(|side| (0.01..=1_000_000.0).contains(side))
}

fn resize(session: &mut Session, command: &Command) -> CommandResult {
    let id = page_id(command)?;
    let source = &session.document;
    let index = page_index(source, id)?;
    let size: [f32; 2] = serde_json::from_value(command.params["size"].clone())
        .map_err(|_| CommandError::invalid_params())?;
    if !can_resize(source, id) || !valid_page_size(size) {
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
        resize_surfaces(source, &mut doc, &[(index, id, size)])?;
    }
    let mut result = commit(session, RESIZE, doc, id)?;
    result["size"] = json!(size);
    result["fold"] = fold_info(fold, size);
    Ok(result)
}

pub(super) fn resize_surfaces(
    source: &Document, doc: &mut Document, targets: &[(usize, PageId, [f32; 2])],
) -> Result<(), CommandError> {
    let rects = crate::scene::page_rects(source);
    for &(index, id, size) in targets {
        if !valid_page_size(size) || !can_resize(source, id) {
            return Err(CommandError::invalid_params());
        }
        let [x, y, w, h] = rects.get(index).copied().ok_or_else(CommandError::invalid_params)?;
        doc.pages[index].size = size;
        if let Some(board) = source.linked_artboard(id) {
            let next = doc.find_object_mut(board.id.0).ok_or_else(CommandError::not_found)?;
            next.size = size;
            let after = crate::geometry::object_world_bounds(next);
            super::layer_kinds::follow_surface_resize(doc, Some(board.id), crate::geometry::object_world_bounds(board), after);
        }
        super::layer_kinds::follow_surface_resize(doc, None, [x, y, w, h], [x, y, size[0], size[1]]);
    }
    if !targets.is_empty() {
        preserve_content(source, doc);
    }
    Ok(())
}

fn move_page(session: &mut Session, command: &Command) -> CommandResult {
    let vacancy = command.params.get("vacancy");
    let destination = command.params.get("destination");
    if let Some(value) = vacancy.or(destination) {
        if command.params.get("number").is_some() || (vacancy.is_some() && destination.is_some()) {
            return Err(CommandError::invalid_params());
        }
        let ids = match (command.params.get("id"), command.params.get("ids")) {
            (Some(_), None) => vec![page_id(command)?],
            (None, Some(ids)) => serde_json::from_value(ids.clone()).map_err(|_| CommandError::invalid_params())?,
            _ => return Err(CommandError::invalid_params()),
        };
        return if vacancy.is_some() {
            spreads::move_into_slot(session, ids, value)
        } else {
            spreads::move_relative(session, ids, value)
        };
    }
    if let Some(ids) = command.params.get("ids") {
        if command.params.get("id").is_some() {
            return Err(CommandError::invalid_params());
        }
        let ids = serde_json::from_value(ids.clone()).map_err(|_| CommandError::invalid_params())?;
        return move_pages(session, command, ids);
    }
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
            let group = spread_at_insertion(&doc, to);
            if page.spread_group != group {
                page.spread_spine = None;
            }
            page.spread_group = group;
        }
    }
    doc.pages.insert(to, page);
    if doc.layout_artboard_origins.is_some() && doc.fixed_spreads.is_none() && crate::data_merge::layout::has_right_cover(&doc) {
        doc.pages[0].spread_group = None;
    }
    preserve_content(source, &mut doc);
    super::masters::update_after_move(source, &mut doc)?;
    let selection = moved_page_selection(session, &doc);
    super::document_change::commit(session, MOVE, doc, selection, json!({"id": id, "number": to + 1}))
}

fn move_pages(session: &mut Session, command: &Command, ids: Vec<PageId>) -> CommandResult {
    let source = &session.document;
    let indices = selected_page_indices(source, &ids)?;
    let selected: HashSet<_> = ids.iter().copied().collect();
    let to = command.params["number"].as_u64().and_then(|n| usize::try_from(n).ok())
        .and_then(|n| n.checked_sub(1)).filter(|&n| n <= source.pages.len() - ids.len())
        .ok_or_else(CommandError::invalid_params)?;
    let ordered_ids: Vec<_> = indices.iter().map(|&index| source.pages[index].id).collect();
    if indices.iter().copied().eq(to..to + indices.len()) {
        return Ok(json!({"id": ordered_ids[0], "ids": ordered_ids, "number": to + 1,
            "doc_version": session.generation, "changes": []}));
    }
    if let [id] = ordered_ids.as_slice() {
        let single = Command::with_params(MOVE, json!({"id": id, "number": to + 1}));
        let mut result = move_page(session, &single)?;
        result["ids"] = json!(ordered_ids);
        return Ok(result);
    }

    let mut doc = source.clone();
    let mut moved: Vec<_> = indices.iter().map(|&index| source.pages[index].clone()).collect();
    doc.pages.retain(|page| !selected.contains(&page.id));
    if doc.layout_artboard_origins.is_some() {
        doc.preserve_layout_page_order = true;
        let destination = spread_at_insertion(&doc, to);
        let remaining: HashSet<_> = doc.pages.iter().filter_map(|page| page.spread_group).collect();
        for page in &mut moved {
            let group = destination.or_else(|| page.spread_group.filter(|group| !remaining.contains(group)));
            if page.spread_group != group {
                page.spread_spine = None;
            }
            page.spread_group = group;
        }
    }
    doc.pages.splice(to..to, moved);
    if doc.layout_artboard_origins.is_some() && doc.fixed_spreads.is_none() && crate::data_merge::layout::has_right_cover(&doc) {
        doc.pages[0].spread_group = None;
    }
    preserve_content(source, &mut doc);
    super::masters::update_after_move(source, &mut doc)?;
    let selection = moved_page_selection(session, &doc);
    super::document_change::commit(session, MOVE, doc, selection,
        json!({"id": ordered_ids[0], "ids": ordered_ids, "number": to + 1}))
}

fn moved_page_selection(session: &Session, doc: &Document) -> Vec<ObjectId> {
    session.selection.ids.iter().copied().filter(|&id| doc.get_object(id).is_some()).collect()
}

fn page_id(command: &Command) -> Result<PageId, CommandError> {
    crate::text_cmd::param_u64(&command.params, "id").map(PageId)
}

pub(crate) fn selected_page_indices(doc: &Document, ids: &[PageId]) -> Result<Vec<usize>, CommandError> {
    let selected: HashSet<_> = ids.iter().copied().collect();
    if selected.is_empty() || selected.len() != ids.len() {
        return Err(CommandError::invalid_params());
    }
    let indices: Vec<_> = doc.pages.iter().enumerate()
        .filter_map(|(index, page)| selected.contains(&page.id).then_some(index))
        .collect();
    if indices.len() != ids.len() {
        return Err(CommandError::not_found());
    }
    Ok(indices)
}

fn duplicate_page(session: &mut Session, command: &Command) -> CommandResult {
    let before = &session.document;
    let ids: Vec<PageId> = match command.params.get("ids") {
        Some(value) if command.params.get("id").is_none() =>
            serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?,
        None => vec![page_id(command)?],
        _ => return Err(CommandError::invalid_params()),
    };
    let indices = selected_page_indices(before, &ids)?;
    let target = |key| -> Result<Option<usize>, CommandError> {
        command.params.get(key).map(|value| {
            let page = value.as_u64().map(PageId).ok_or_else(CommandError::invalid_params)?;
            page_index(before, page)
        }).transpose()
    };
    let fill = command.params.get("vacancy").map(|value| {
        if command.params.get("before_id").is_some() || command.params.get("after_id").is_some() {
            return Err(CommandError::invalid_params());
        }
        spreads::fill_target(before, value, indices.len())
    }).transpose()?;
    let (at, adjacent_duplicate) = match (target("before_id")?, target("after_id")?) {
        (Some(index), None) => (index, false),
        (None, Some(index)) => (index + 1, false),
        (None, None) => fill.as_ref().map_or((indices[indices.len() - 1] + 1, indices.len() == 1),
            |fill| (fill.index, false)),
        _ => return Err(CommandError::invalid_params()),
    };
    let layout = before.layout_artboard_origins.is_some();
    if !layout && !adjacent_duplicate {
        return Err(CommandError::invalid_params());
    }
    let rects = crate::scene::page_rects(before);
    let page_of: HashMap<_, _> = before.objects.iter().filter_map(|object| {
        if super::masters::owns_source(before, object.id) {
            return None;
        }
        let index = object_page_index(before, object.id);
        indices.binary_search(&index).ok().map(|offset| (object.id, offset))
    }).collect();
    let members = page_of.keys().copied().collect();
    let mut bundle = crate::clipboard::collect_bundle_members(before, &members);
    let excerpts = crate::longdoc::pages::restrict_page_copy(before, &mut bundle);
    let mut doc = before.clone();
    doc.preserve_layout_page_order |= layout;
    let destination_group = fill.as_ref().map(|fill| fill.group).or_else(|| spread_at_insertion(before, at));
    let mut copied_groups = HashMap::new();
    let mut group_counts = HashMap::new();
    if layout && !adjacent_duplicate && destination_group.is_none() {
        for &index in &indices {
            if let Some(group) = before.pages[index].spread_group {
                *group_counts.entry(group).or_insert(0usize) += 1;
            }
        }
    }
    let mut new_ids = Vec::with_capacity(indices.len());
    for (offset, &index) in indices.iter().enumerate() {
        let source = &before.pages[index];
        let [_, _, width, height] = rects[index];
        let id = doc.alloc_page_id();
        let mut page = crate::document::Page::new(id, [width, height]);
        page.name.clone_from(&source.name);
        page.tag_colour.clone_from(&source.tag_colour);
        page.fold = source.fold;
        page.column_guides = source.column_guides;
        page.spread_group = if adjacent_duplicate && !(index == 0 && crate::data_merge::layout::has_right_cover(before)) {
            source.spread_group
        } else if let Some(group) = destination_group {
            Some(group)
        } else if let Some(group) = source.spread_group.filter(|group| group_counts.get(group).is_some_and(|&count| count > 1)) {
            let new_group = if let Some(&group) = copied_groups.get(&group) {
                group
            } else {
                let next = next_spread_group(&doc)?;
                copied_groups.insert(group, next);
                next
            };
            Some(new_group)
        } else {
            None
        };
        if destination_group.is_none() && page.spread_group.is_some() {
            page.spread_spine = source.spread_spine;
        }
        if let Some(fill) = &fill {
            fill.set_page(&mut doc, offset, id, [width, height]);
        }
        insert_page(&mut doc, at + offset, page);
        new_ids.push(id);
    }
    if layout && doc.fixed_spreads.is_none() && crate::data_merge::layout::has_right_cover(&doc) {
        doc.pages[0].spread_group = None;
    }
    preserve_content(before, &mut doc);
    let shifts: Vec<_> = indices.iter().enumerate().map(|(offset, &index)| {
        let [x, y, width, _] = rects[index];
        if !layout && before.linked_artboard(before.pages[index].id).is_some() {
            let right = before.objects.iter().filter(|object| crate::artboard::is_artboard(object)
                && !before.master_pages.roots.contains(&object.id)).map(|object| {
                    let [x, _, w, _] = crate::geometry::object_world_bounds(object);
                    x + w
                }).fold(x + width, f32::max);
            [right + crate::artboard::edit::default_spacing(before) - x, 0.0]
        } else {
            let [new_x, new_y] = page_origin(&doc, at + offset);
            [new_x - x, new_y - y]
        }
    }).collect();
    let mapping = crate::clipboard::copy_bundle_in_document(&mut doc, &bundle, [0.0; 2]);
    for (&source, &copy) in &mapping {
        if let Some(&offset) = page_of.get(&source) && let Some(object) = doc.find_object_mut(copy.0) {
            object.page_id = Some(new_ids[offset]);
            object.pos[0] += shifts[offset][0];
            object.pos[1] += shifts[offset][1];
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
    let mut page_map = HashMap::new();
    for (offset, (&index, &id)) in indices.iter().zip(&new_ids).enumerate() {
        let source_id = before.pages[index].id;
        page_map.insert(source_id, id);
        if let Some(board) = before.linked_artboard(source_id).and_then(|board| mapping.get(&board.id).copied()) {
            doc.pages[at + offset].artboard_id = Some(board);
            doc.artboard_backing_pages.insert(id);
            crate::artboard::edit::name_duplicate(&mut doc, board);
        }
        for assignment in before.master_pages.on_page(source_id) {
            let mut copy = assignment.clone();
            copy.page = id;
            copy.group = assignment.group.and_then(|group| mapping.get(&group).copied());
            copy.links = assignment.links.iter().filter_map(|&(source, instance)| {
                Some((source, *mapping.get(&instance)?))
            }).collect();
            doc.master_pages.assignments.push(copy);
        }
    }
    super::masters::lock_positions(&mut doc);
    if !before.anchors.is_empty() || !before.hyperlinks.is_empty()
        || !before.index.marks.is_empty() || !before.notes.notes.is_empty()
        || !before.toc.tocs.is_empty() || !before.index.indexes.is_empty()
    {
        use crate::longdoc::page_import::navigation::{copy, PageTransfer};
        copy(before, before, &mut doc, &bundle, &mapping, &excerpts,
            PageTransfer::Duplicate(&page_map))?;
    }
    let selection = session.selection.ids.clone();
    let mut guides = session.guides.clone();
    for (original, &id) in session.guides.items.iter().filter_map(|guide| {
        Some((guide, page_map.get(&guide.page_id?)?))
    }) {
        let mut copy = original.clone();
        copy.id = guides.next_id();
        copy.page_id = Some(id);
        if copy.name.as_deref().is_some_and(|name| name == crate::guides::Guides::default_name(original.id)) {
            copy.name = Some(crate::guides::Guides::default_name(copy.id));
        }
        guides.insert(copy).map_err(|_| CommandError::invalid_params())?;
    }
    super::document_change::commit_guides(session, DUPLICATE, doc, selection, guides,
        json!({"id": new_ids[0], "ids": new_ids}))
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
    prepare_page_deletion(session, ids, false)?.commit(session, DELETE)
}

pub(crate) struct PageDeletion {
    document: Document,
    selection: Vec<ObjectId>,
    guides: crate::guides::Guides,
    ids: Vec<PageId>,
}

impl PageDeletion {
    pub(crate) fn commit(self, session: &mut Session, command: &str) -> CommandResult {
        super::document_change::commit_guides(session, command, self.document, self.selection, self.guides,
            json!({"id": self.ids[0], "ids": self.ids}))
    }
}

pub(crate) fn prepare_page_deletion(session: &Session, ids: Vec<PageId>, moving: bool) -> Result<PageDeletion, CommandError> {
    let before = &session.document;
    let indices = selected_page_indices(before, &ids)?;
    if indices.len() >= before.pages.len() {
        return Err(CommandError::new("LAST_PAGE"));
    }
    let removed: HashSet<_> = ids.iter().copied().collect();
    let mut doc = before.clone();
    doc.pages.retain(|page| !removed.contains(&page.id));
    let doomed = remove_page_content(before, &mut doc, &indices, moving)?;
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
    Ok(PageDeletion { document: doc, selection, guides, ids })
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
    doc.slices.retain(|slice| !matches!(slice.source, crate::slice::SliceSource::Linked(object) | crate::slice::SliceSource::Layer(object) if doomed.contains(&object)));
}

pub(crate) fn preserve_content(before: &Document, after: &mut Document) {
    spreads::reconcile_fixed_spreads(after);
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
    if after.page_move_anchor_to_spine && flow.is_some() {
        spreads::anchor_content_to_spine(before, after);
    }
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
