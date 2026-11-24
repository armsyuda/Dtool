use std::collections::{HashMap, HashSet};

use crate::artboard::is_artboard;
use crate::data_merge::layout::{object_page_index, page_rect};
use crate::document::PageId;
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, ObjectKind, Session};
use serde_json::json;

pub const FROM_ARTBOARDS: &str = "page.from_artboards";
pub const TO_ARTBOARDS: &str = "page.to_artboards";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), FROM_ARTBOARDS | TO_ARTBOARDS).then_some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command).map(|enabled| {
        if !enabled {
            return Err(CommandError::no_document());
        }
        match plan(&session.document, &command.name) {
            Some((document, pairs)) => commit(session, &command.name, document, pairs),
            None => Ok(json!({"links": [], "doc_version": session.generation, "changes": []})),
        }
    })
}

type Link = (PageId, ObjectId);

fn plan(doc: &Document, name: &str) -> Option<(Document, Vec<Link>)> {
    match name {
        FROM_ARTBOARDS => enter_layout(doc, false),
        TO_ARTBOARDS => enter_vector(doc),
        _ => None,
    }
}

pub(crate) fn enter_layout(doc: &Document, allow_plain_pages: bool) -> Option<(Document, Vec<Link>)> {
    let imported = from_artboards(doc);
    if imported.is_none() && (doc.pages.is_empty() || doc.layout_artboard_origins.is_some()
        || (!allow_plain_pages && !doc.pages.iter().any(|page| doc.linked_artboard(page.id).is_some())))
    {
        return None;
    }
    let (mut document, links) = imported.unwrap_or_else(|| (doc.clone(), Vec::new()));
    let origins: Vec<_> = document.pages.iter().filter_map(|page| document.linked_artboard(page.id)).map(|board| {
        let [x, y, _, _] = crate::geometry::object_world_bounds(board);
        (board.id.0, [x, y])
    }).collect();
    let before = document.clone();
    let saved = document.layout_artboard_origins.get_or_insert_with(Default::default);
    if doc.layout_artboard_origins.is_none() {
        saved.extend(origins);
    } else {
        let linked: HashSet<_> = links.iter().map(|(_, board)| board.0).collect();
        saved.extend(origins.into_iter().filter(|(id, _)| linked.contains(id)));
    }
    document.setup.arrange_vertical = true;
    if doc.layout_artboard_origins.is_none() && !has_authored_page_order(doc) {
        order_linked_pages(&mut document);
    }
    super::pages::preserve_content(&before, &mut document);
    super::masters::arrange_masters(&mut document);
    Some((document, links))
}

pub(super) fn enter_vector(doc: &Document) -> Option<(Document, Vec<Link>)> {
    let Some(origins) = &doc.layout_artboard_origins else { return to_artboards(doc); };
    let mut document = doc.clone();
    document.layout_artboard_origins = None;
    document.preserve_layout_page_order = has_authored_page_order(doc);
    let fixed: Vec<_> = doc.objects.iter().filter(|object| is_artboard(object) && !doc.master_pages.roots.contains(&object.id))
        .filter_map(|object| {
            let [x, y, w, h] = crate::geometry::object_world_bounds(object);
            if let Some(&[x, y]) = origins.get(&object.id.0) {
                Some([x, y, w, h])
            } else if doc.pages.iter().any(|page| page.artboard_id == Some(object.id)) {
                None
            } else {
                Some([x, y, w, h])
            }
        }).collect();
    let positions = grid_positions(doc, &fixed, |index| {
        doc.linked_artboard(doc.pages[index].id).is_some_and(|board| !origins.contains_key(&board.id.0))
    });
    super::pages::move_page_content(doc, &mut document, |document, index| {
        document.linked_artboard(document.pages[index].id).map_or_else(
            || crate::data_merge::layout::page_origin(document, index),
            |board| origins.get(&board.id.0).copied().unwrap_or(positions[index]),
        )
    });
    if let Some(converted) = to_artboards(&document) {
        return Some(converted);
    }
    super::masters::arrange_masters(&mut document);
    Some((document, Vec::new()))
}

pub fn commit(session: &mut Session, name: &str, document: Document, pairs: Vec<Link>) -> CommandResult {
    let selection = session.selection.ids.clone();
    let has_pages = !session.document.pages.is_empty() && !document.pages.is_empty();
    let mut view_target = None;
    let view_shift = (has_pages && (name == TO_ARTBOARDS || document.layout_artboard_origins.is_some())).then(|| {
        let before = &session.document;
        let positions = session.view_canvas_size.and_then(|size| {
            let target = crate::view::ViewTarget::at_view_center_with_hint(
                before, &session.view, size, session.view_target_hint,
            )?;
            let from = target.bounds(before)?;
            let mapped = match target {
                crate::view::ViewTarget::Page(id) => document.linked_artboard(id)
                    .map_or(target, |board| crate::view::ViewTarget::Artboard(board.id)),
                _ => target,
            };
            let to = mapped.bounds(&document)?;
            view_target = Some(mapped);
            Some(([from[0], from[1]], [to[0], to[1]]))
        }).or_else(|| selection.first().and_then(|id| {
            Some((before.get_object(*id)?.pos, document.get_object(*id)?.pos))
        }));
        let (old_origin, new_origin) = positions.unwrap_or_else(|| {
            let [old_x, old_y, _, _] = page_rect(before, 0);
            let [new_x, new_y, _, _] = page_rect(&document, 0);
            ([old_x, old_y], [new_x, new_y])
        });
        let from = crate::view::doc_to_view(&session.view, old_origin);
        let to = crate::view::doc_to_view(&session.view, new_origin);
        [from[0] - to[0], from[1] - to[1]]
    });
    let links: Vec<_> = pairs.into_iter().map(|(page, board)| json!({"page_id": page, "artboard_id": board})).collect();
    let result = super::document_change::commit(session, name, document, selection, json!({"links": links}))?;
    if let Some([dx, dy]) = view_shift {
        session.view.scroll[0] += dx;
        session.view.scroll[1] += dy;
        session.view_target_hint = view_target;
    }
    Ok(result)
}

fn from_artboards(doc: &Document) -> Option<(Document, Vec<Link>)> {
    let linked: HashSet<_> = doc.pages.iter().filter_map(|page| doc.linked_artboard(page.id).map(|board| board.id)).collect();
    let candidates = doc.objects.iter().filter(|board| {
        board.parent_id.is_none() && is_artboard(board) && !linked.contains(&board.id)
            && !doc.master_pages.roots.contains(&board.id)
    }).filter_map(|board| {
        let rect = crate::geometry::object_world_bounds(board);
        valid_rect(rect).then_some((board, rect))
    });
    let boards = reading_order(candidates.collect());
    if boards.is_empty() {
        return None;
    }

    let mut document = doc.clone();
    let mut links = Vec::with_capacity(boards.len());
    let backing = legacy_backing_links(doc, &boards);
    let placeholder = doc.pages.len() == 1 && linked.is_empty() && backing.is_empty()
        && doc.pages[0].name.is_empty() && doc.sections.len() <= 1
        && !doc.objects.iter().any(|object| object.parent_id.is_none() && !is_artboard(object));
    for (position, (board, bounds)) in boards.into_iter().enumerate() {
        let id = backing.get(&board.id).copied()
            .or_else(|| (placeholder && position == 0).then(|| doc.pages[0].id))
            .unwrap_or_else(|| document.push_page([bounds[2], bounds[3]]));
        let page = document.pages.iter_mut().find(|page| page.id == id).expect("page allocated");
        page.artboard_id = Some(board.id);
        page.size = [bounds[2], bounds[3]];
        if page.name.is_empty() {
            page.name.clone_from(&board.name);
        }
        document.artboard_backing_pages.insert(id);
        // Only the original board's descendants move to this page, even when boards overlap.
        crate::artboard::edit::set_subtree_page(&mut document, board.id, id);
        links.push((id, board.id));
    }
    Some((document, links))
}

fn legacy_backing_links(doc: &Document, boards: &[(&Object, [f32; 4])]) -> HashMap<ObjectId, PageId> {
    // Older page conversions stored only the backing flag and the board's page ID.
    // Reserve the original page before creating pages for neighbouring artboards.
    let mut assignments = HashMap::new();
    for (index, page) in doc.pages.iter().enumerate().filter(|(_, page)| {
        doc.artboard_backing_pages.contains(&page.id) && doc.linked_artboard(page.id).is_none()
    }) {
        let original = page_rect(doc, index);
        let mut matches = boards.iter().filter(|(board, _)| board.page_id == Some(page.id));
        let Some(first) = matches.next() else { continue };
        let second = matches.next();
        let board = match second {
            None => Some(first.0),
            Some(second) => {
                let mut exact = [first, second].into_iter().chain(matches).filter(|(_, bounds)| *bounds == original);
                exact.next().map(|(board, _)| *board).filter(|_| exact.next().is_none())
            }
        };
        if let Some(board) = board {
            assignments.insert(board.id, page.id);
        }
    }
    assignments
}

pub(super) fn initialize_artboard(doc: &mut Document) -> Option<()> {
    let [w, h] = doc.pages.first()?.size;
    let rect = [0.0, 0.0, w, h];
    if !valid_rect(rect) {
        return None;
    }
    add_linked_artboard(doc, 0, rect);
    Some(())
}

fn add_linked_artboard(document: &mut Document, index: usize, [x, y, w, h]: [f32; 4]) -> ObjectId {
    let page_id = document.pages[index].id;
    let name = crate::artboard::next_artboard_name(document);
    let id = document.create_object(Object {
        kind: ObjectKind::Artboard,
        name,
        pos: [x, y],
        size: [w, h],
        page_id: Some(page_id),
        blend_mode: crate::blend::BlendMode::PassThrough,
        ..Object::default()
    });
    document.pages[index].artboard_id = Some(id);
    document.artboard_backing_pages.insert(page_id);
    id
}

fn to_artboards(doc: &Document) -> Option<(Document, Vec<Link>)> {
    let eligible: Vec<_> = doc.pages.iter().enumerate().filter(|(index, page)| {
        doc.linked_artboard(page.id).is_none()
            && !crate::artboard::edit::is_backing_page(doc, *index)
            && valid_rect(page_rect(doc, *index))
    }).collect();
    if eligible.is_empty() {
        return None;
    }

    let mut document = doc.clone();
    let mut links = Vec::with_capacity(eligible.len());
    let fixed: Vec<_> = doc.objects.iter().filter(|object| is_artboard(object) && !doc.master_pages.roots.contains(&object.id))
        .map(crate::geometry::object_world_bounds).collect();
    let new_indices: HashSet<_> = eligible.iter().map(|(index, _)| *index).collect();
    let positions = grid_positions(doc, &fixed, |index| new_indices.contains(&index));
    for (index, page) in eligible {
        let [x, y, w, h] = page_rect(doc, index);
        let pos = positions[index];
        let delta = [pos[0] - x, pos[1] - y];
        let id = add_linked_artboard(&mut document, index, [pos[0], pos[1], w, h]);
        let roots: HashSet<_> = doc.objects.iter().filter(|object| {
            object.parent_id.is_none() && !is_artboard(object) && object_page_index(doc, object.id) == index
        }).map(|object| object.id).collect();
        let descendants: HashSet<_> = roots.iter().flat_map(|root| doc.subtree_ids(*root)).collect();
        for object in &mut document.objects {
            if roots.contains(&object.id) {
                object.parent_id = Some(id);
            }
            if descendants.contains(&object.id) {
                object.pos[0] += delta[0];
                object.pos[1] += delta[1];
            }
        }
        crate::artboard::edit::set_subtree_page(&mut document, id, page.id);
        links.push((page.id, id));
    }
    super::masters::arrange_masters(&mut document);
    Some((document, links))
}

fn reading_order(mut boards: Vec<(&Object, [f32; 4])>) -> Vec<(&Object, [f32; 4])> {
    boards.sort_by(|(a, left), (b, right)| left[1].total_cmp(&right[1]).then(a.id.0.cmp(&b.id.0)));
    let mut row_start = 0;
    while row_start < boards.len() {
        let [_, top, _, height] = boards[row_start].1;
        let in_row = boards[row_start..].iter().take_while(|(_, rect)| {
            rect[1] - top <= height.min(rect[3]) * 0.25
        }).count();
        let row_end = row_start + in_row.max(1);
        boards[row_start..row_end].sort_by(|(a, left), (b, right)| {
            left[0].total_cmp(&right[0]).then(left[1].total_cmp(&right[1])).then(a.id.0.cmp(&b.id.0))
        });
        row_start = row_end;
    }
    boards
}

fn has_authored_page_order(doc: &Document) -> bool {
    // Recognize spread and section layouts saved before the order flag existed.
    doc.preserve_layout_page_order || (doc.setup.facing_pages && doc.pages.len() > 1) || !doc.sections.is_empty()
        || doc.pages.iter().any(|page| page.spread_group.is_some())
}

fn order_linked_pages(doc: &mut Document) {
    let mut slots = Vec::new();
    let mut indices = HashMap::new();
    let mut boards = Vec::new();
    for (index, page) in doc.pages.iter().enumerate() {
        let Some(board) = doc.linked_artboard(page.id) else { continue };
        let bounds = crate::geometry::object_world_bounds(board);
        if valid_rect(bounds) {
            slots.push(index);
            indices.insert(board.id, index);
            boards.push((board, bounds));
        }
    }
    let sorted: Vec<_> = reading_order(boards).into_iter()
        .map(|(board, _)| doc.pages[indices[&board.id]].clone()).collect();
    for (index, page) in slots.into_iter().zip(sorted) {
        doc.pages[index] = page;
    }
}

fn grid_positions(doc: &Document, fixed: &[[f32; 4]], placing: impl Fn(usize) -> bool) -> Vec<[f32; 2]> {
    const COLUMNS: usize = 10;
    let gap = crate::artboard::edit::default_spacing(doc);
    let mut widths = [0.0_f32; COLUMNS];
    let mut heights = vec![0.0_f32; doc.pages.len().div_ceil(COLUMNS)];
    for index in 0..doc.pages.len() {
        let [_, _, width, height] = page_rect(doc, index);
        widths[index % COLUMNS] = widths[index % COLUMNS].max(width);
        heights[index / COLUMNS] = heights[index / COLUMNS].max(height);
    }
    let master_height = super::masters::master_row_height(doc)
        .unwrap_or_else(|| heights.first().copied().unwrap_or_default());
    let first_y = master_height + gap;
    let mut xs = [0.0; COLUMNS];
    for column in 1..COLUMNS {
        xs[column] = xs[column - 1] + widths[column - 1] + gap;
    }
    let ys: Vec<_> = heights.into_iter().scan(first_y, |origin, height| {
        let y = *origin;
        *origin += height + gap;
        Some(y)
    }).collect();
    let mut positions: Vec<_> = (0..doc.pages.len()).map(|index| [xs[index % COLUMNS], ys[index / COLUMNS]]).collect();
    let overlap = positions.iter().enumerate().filter(|(index, _)| placing(*index))
        .any(|(index, &[x, y])| {
            let [_, _, w, h] = page_rect(doc, index);
            fixed.iter().any(|&[a, b, width, height]| x < a + width && a < x + w && y < b + height && b < y + h)
        });
    if overlap {
        let bottom = fixed.iter().map(|rect| rect[1] + rect[3]).fold(first_y, f32::max);
        let offset = bottom + gap - first_y;
        for pos in &mut positions {
            pos[1] += offset;
        }
    }
    positions
}

fn valid_rect(rect: [f32; 4]) -> bool {
    rect.iter().all(|n| n.is_finite()) && rect[2] > 0.0 && rect[3] > 0.0
}
