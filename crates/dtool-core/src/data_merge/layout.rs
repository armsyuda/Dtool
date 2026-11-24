use crate::document::Document;
use crate::object::ObjectId;
use super::{cell_size, descendants, clone_cell, Corner, MergeLayout, PageRepeat};

pub fn capacity(layout: &MergeLayout) -> usize {
    (layout.rows.max(1) * layout.columns.max(1)) as usize
}

pub fn slot_row_column(layout: &MergeLayout, slot: usize) -> (u32, u32) {
    let rows = layout.rows.max(1);
    let columns = layout.columns.max(1);
    let (row, column) = if layout.rows_first {
        (slot as u32 / columns, slot as u32 % columns)
    } else {
        (slot as u32 % rows, slot as u32 / rows)
    };
    match layout.origin {
        Corner::TopLeft => (row, column),
        Corner::TopRight => (row, columns - 1 - column),
        Corner::BottomLeft => (rows - 1 - row, column),
        Corner::BottomRight => (rows - 1 - row, columns - 1 - column),
    }
}

pub fn resolved_cell_size(layout: &MergeLayout, frame: [f32; 2], rows: u32, columns: u32) -> [f32; 2] {
    if layout.preserve_cell_size {
        cell_size(layout, frame, rows, columns)
    } else {
        let auto = |total: f32, count: u32, gutter: f32| {
            if count <= 1 { total } else { (total - gutter * (count as f32 - 1.0)) / count as f32 }
        };
        [auto(frame[0], columns, layout.gutter[0]), auto(frame[1], rows, layout.gutter[1])]
    }
}

pub fn cell_offset(layout: &MergeLayout, slot: usize, frame: [f32; 2]) -> [f32; 2] {
    let rows = layout.rows.max(1);
    let columns = layout.columns.max(1);
    let cell = resolved_cell_size(layout, frame, rows, columns);
    let (row, column) = slot_row_column(layout, slot);
    [
        column as f32 * (cell[0] + layout.gutter[0]),
        row as f32 * (cell[1] + layout.gutter[1]),
    ]
}

pub fn page_size(doc: &Document) -> [f32; 2] {
    doc.pages.first().map(|page| page.size).unwrap_or([800.0, 600.0])
}

pub fn page_origin(doc: &Document, page_index: usize) -> [f32; 2] {
    if let Some(board) = doc.pages.get(page_index).and_then(|page| doc.linked_artboard(page.id)) {
        let [x, y, _, _] = crate::geometry::object_world_bounds(board);
        return [x, y];
    }
    if doc.layout_artboard_origins.is_some()
        && let Some(origin) = flow_origins(doc).get(page_index)
    {
        return *origin;
    }
    let size = page_size(doc);
    if doc.setup.arrange_vertical {
        [0.0, page_index as f32 * size[1]]
    } else {
        [page_index as f32 * size[0], 0.0]
    }
}

pub fn has_right_cover(doc: &Document) -> bool {
    doc.setup.facing_pages && doc.setup.start_on_right && doc.pages.len() > 1
}

/// Consecutive explicit spreads and automatic single or facing pages.
pub fn spread_ranges(doc: &Document) -> Vec<std::ops::Range<usize>> {
    let count = doc.pages.len().max(1);
    let group_at = |index: usize| {
        doc.pages.get(index).filter(|_| doc.layout_artboard_origins.is_some())
            .and_then(|page| page.spread_group)
    };
    let mut spreads = Vec::new();
    let mut start = 0;
    while start < count {
        let end = if start == 0 && has_right_cover(doc) {
            start + 1
        } else if let Some(group) = group_at(start) {
            (start + 1..count).find(|&index| group_at(index) != Some(group)).unwrap_or(count)
        } else if doc.setup.facing_pages && start + 1 < count && group_at(start + 1).is_none()
        {
            start + 2
        } else {
            start + 1
        };
        spreads.push(start..end);
        start = end;
    }
    spreads
}

/// Spread placement for the layout workspace, independent of the saved vector positions.
pub(crate) fn flow_origins(doc: &Document) -> Vec<[f32; 2]> {
    let boards: std::collections::HashMap<_, _> = doc.objects.iter()
        .filter(|object| object.parent_id.is_none() && crate::artboard::is_artboard(object))
        .map(|object| (object.id, object)).collect();
    let sizes: Vec<_> = doc.pages.iter().map(|page| {
        page.artboard_id.and_then(|id| boards.get(&id)).filter(|board| board.page_id == Some(page.id))
            .map_or(page.size, |board| {
                let [_, _, w, h] = crate::geometry::object_world_bounds(board);
                [w, h]
            })
    }).collect();
    let gap = crate::artboard::edit::default_spacing(doc);
    let vertical = doc.setup.arrange_vertical;
    let spreads = spread_ranges(doc);
    let facing = doc.setup.facing_pages && sizes.len() > 1;
    let cover = has_right_cover(doc);
    let cover_width = if cover { sizes[0][0] } else { 0.0 };
    let spine = if vertical && facing {
        spreads.iter().skip(usize::from(cover)).map(|spread| sizes[spread.start][0])
            .fold(cover_width, f32::max)
    } else {
        cover_width
    };
    let mut origins = Vec::with_capacity(sizes.len());
    let mut cursor = 0.0;
    for spread in spreads.into_iter().filter(|spread| spread.end <= sizes.len()) {
        let start = spread.start;
        let left = sizes[start];
        let x = if cover && start == 0 {
            spine
        } else if vertical && facing {
            spine - left[0]
        } else {
            0.0
        };
        let [mut x, y] = if vertical { [x, cursor] } else { [cursor + x, 0.0] };
        let mut height = 0.0_f32;
        for [width, page_height] in &sizes[spread] {
            origins.push([x, y]);
            x += width;
            height = height.max(*page_height);
        }
        cursor = if vertical { cursor + height } else { x };
        cursor += gap;
    }
    origins
}

pub fn page_rect(doc: &Document, index: usize) -> [f32; 4] {
    let page = &doc.pages[index];
    if let Some(board) = doc.linked_artboard(page.id) {
        return crate::geometry::object_world_bounds(board);
    }
    let [x, y] = page_origin(doc, index);
    [x, y, page.size[0], page.size[1]]
}

pub fn object_page_index(doc: &Document, object_id: ObjectId) -> usize {
    let size = page_size(doc);
    let Some(object) = doc.get_object(object_id) else {
        return 0;
    };
    let page_id = crate::artboard::owner(doc, object_id).and_then(|board| board.page_id).or(object.page_id);
    if let Some(page_id) = page_id {
        if let Some(index) = doc.pages.iter().position(|page| page.id == page_id) {
            return index;
        }
    }
    if doc.layout_artboard_origins.is_some()
        && let Some(index) = crate::artboard::edit::page_at_point(doc, object.pos)
    {
        return index;
    }
    let total = doc.pages.len().max(1);
    let index = if doc.setup.arrange_vertical {
        if size[1] <= 0.0 { 0 } else { (object.pos[1] / size[1]).floor().max(0.0) as usize }
    } else if size[0] <= 0.0 {
        0
    } else {
        (object.pos[0] / size[0]).floor().max(0.0) as usize
    };
    index.min(total.saturating_sub(1))
}

pub fn template_page_indices(doc: &Document) -> Vec<usize> {
    let total = doc.pages.len().max(1);
    let mut pages = Vec::new();
    for src in doc.merge_sources.iter().filter(|src| src.enabled) {
        match src.pages {
            PageRepeat::All => {}
            PageRepeat::Range(start, end) | PageRepeat::Repeat(start, end) => {
                let from = start.saturating_sub(1).min(total);
                let to = end.min(total).max(from);
                for index in from..to {
                    if !pages.contains(&index) {
                        pages.push(index);
                    }
                }
            }
        }
    }
    if pages.is_empty() {
        (0..total).collect()
    } else {
        pages.sort_unstable();
        pages
    }
}

pub fn template_ids(doc: &Document, root: ObjectId) -> Vec<ObjectId> {
    let mut ids = vec![root];
    ids.extend(descendants(doc, root));
    ids
}

pub fn ensure_pages(doc: &mut Document, count: usize) {
    if count == 0 {
        return;
    }
    let size = page_size(doc);
    while doc.pages.len() < count {
        doc.push_page(size);
    }
}

pub fn default_layout(object_id: ObjectId) -> MergeLayout {
    MergeLayout {
        object_id,
        rows: 1,
        columns: 1,
        gutter: [0.0, 0.0],
        cell: [0.0, 0.0],
        preserve_cell_size: false,
        record_offset: 0,
        record_advance: 1,
        origin: Corner::TopLeft,
        rows_first: true,
    }
}

pub fn create_layout(doc: &mut Document, layout: MergeLayout) -> usize {
    doc.merge_layouts.push(layout);
    doc.merge_layouts.len() - 1
}

pub fn update_layout(doc: &mut Document, index: usize, layout: MergeLayout) -> bool {
    if let Some(slot) = doc.merge_layouts.get_mut(index) {
        *slot = layout;
        true
    } else {
        false
    }
}

pub fn find_layout_mut(doc: &mut Document, object_id: ObjectId) -> Option<&mut MergeLayout> {
    doc.merge_layouts.iter_mut().find(|layout| layout.object_id == object_id)
}

pub fn find_or_create_layout(doc: &mut Document, object_id: ObjectId) -> &mut MergeLayout {
    if let Some(index) = doc.merge_layouts.iter().position(|layout| layout.object_id == object_id) {
        return &mut doc.merge_layouts[index];
    }
    doc.merge_layouts.push(default_layout(object_id));
    let index = doc.merge_layouts.len() - 1;
    &mut doc.merge_layouts[index]
}

pub fn clone_template(out: &mut Document, template: &[ObjectId], offset: [f32; 2]) -> Vec<(ObjectId, ObjectId)> {
    clone_cell(out, template, offset)
}
