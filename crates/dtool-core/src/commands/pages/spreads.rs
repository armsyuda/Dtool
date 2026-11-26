use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::data_merge::layout::{page_rect, spread_ranges, spread_spine};
use crate::document::PageId;
use crate::{Command, CommandError, CommandResult, Document, Session};

pub const REFLOW: &str = "page.reflow.set";

#[derive(Clone, Copy)]
struct SpineAnchor {
    leading: bool,
    offset: f32,
}

fn page_spine_anchors(doc: &Document, rects: &[[f32; 4]]) -> HashMap<PageId, SpineAnchor> {
    let widths: HashMap<_, _> = doc.pages.iter().zip(rects).map(|(page, rect)| (page.id, rect[2])).collect();
    let mut anchors = HashMap::new();
    let cover = crate::data_merge::layout::has_right_cover(doc);
    for spread in spread_ranges(doc).into_iter().filter(|range| range.end <= doc.pages.len()) {
        let fixed = fixed_spread(doc, &spread);
        if fixed.is_none() && cover && spread.start == 0 {
            anchors.insert(doc.pages[0].id, SpineAnchor { leading: false, offset: 0.0 });
            continue;
        }
        let slots: Vec<_> = if let Some(fixed) = fixed {
            fixed.slots.iter().map(|slot| (slot.page,
                slot.page.and_then(|id| widths.get(&id).copied()).unwrap_or(slot.size[0]))).collect()
        } else {
            doc.pages[spread.clone()].iter().map(|page| (Some(page.id), widths[&page.id])).collect()
        };
        if slots.len() < 2 && !(doc.setup.facing_pages && doc.pages.len() > 1) {
            continue;
        }
        let spine = spread_spine(doc, &spread);
        let spine_x: f32 = slots.iter().take(spine).map(|(_, width)| *width).sum();
        let mut x = 0.0;
        for (index, (page, width)) in slots.into_iter().enumerate() {
            if let Some(id) = page {
                anchors.insert(id, SpineAnchor { leading: index < spine, offset: spine_x - x });
            }
            x += width;
        }
    }
    anchors
}

pub(super) fn anchor_content_to_spine(before: &Document, after: &mut Document) {
    let old_rects = crate::scene::page_rects(before);
    let old = page_spine_anchors(before, &old_rects);
    let new = page_spine_anchors(after, &crate::scene::page_rects(after));
    let crossings: HashMap<_, _> = old.into_iter().filter_map(|(id, old)| {
        let new = new.get(&id)?;
        (old.leading != new.leading).then_some((id, old.offset + new.offset))
    }).collect();
    if crossings.is_empty() {
        return;
    }
    let artboards: HashSet<_> = before.pages.iter().filter_map(|page| before.linked_artboard(page.id).map(|board| board.id)).collect();
    let parents: HashSet<_> = before.objects.iter().filter_map(|object| object.parent_id).collect();
    let mut shifts = HashMap::new();
    for object in &before.objects {
        if artboards.contains(&object.id) || object.parent_id.is_some_and(|id| !artboards.contains(&id))
            || super::super::masters::owns_source(before, object.id)
        {
            continue;
        }
        let index = crate::data_merge::layout::object_page_index(before, object.id);
        if let Some(&sum) = before.pages.get(index).and_then(|page| crossings.get(&page.id))
            && let Some([x, _, width, _]) = crate::command::selection_bounds(before, &[object.id])
        {
            // Use the far edge after crossing, without flipping the artwork or changing group spacing.
            let delta = sum - 2.0 * (x - old_rects[index][0]) - width;
            if delta.is_finite() {
                shifts.insert(object.id, delta);
                if parents.contains(&object.id) {
                    shifts.extend(before.subtree_ids(object.id).into_iter().map(|id| (id, delta)));
                }
            }
        }
    }
    for object in &mut after.objects {
        if let Some(delta) = shifts.get(&object.id) {
            object.pos[0] += delta;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixedSpread {
    pub spine: usize,
    pub cover: bool,
    automatic: bool,
    pub slots: Vec<SpreadSlot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpreadSlot {
    pub page: Option<PageId>,
    pub size: [f32; 2],
}

pub fn is_custom(doc: &Document, spread: &Range<usize>) -> bool {
    doc.layout_artboard_origins.is_some()
        && doc.pages.get(spread.start).is_some_and(|page| page.spread_group.is_some())
        && fixed_spread(doc, spread).is_none_or(|spread| !spread.automatic)
}

pub fn fixed_spread<'a>(doc: &'a Document, spread: &Range<usize>) -> Option<&'a FixedSpread> {
    if doc.layout_artboard_origins.is_none() {
        return None;
    }
    let group = doc.pages.get(spread.start)?.spread_group?;
    doc.fixed_spreads.as_ref()?.get(&group)
}

pub(crate) fn replace_slots(doc: &mut Document, group: u64, range: Range<usize>, spine: usize, cover: bool) -> bool {
    let Some(fixed) = &mut doc.fixed_spreads else { return false; };
    let mut slots = Vec::with_capacity(range.len() + usize::from(cover));
    let pages = &doc.pages[range];
    if cover {
        slots.push(SpreadSlot { page: None, size: pages[0].size });
    }
    slots.extend(pages.iter().map(|page| SpreadSlot { page: Some(page.id), size: page.size }));
    let next = FixedSpread { spine: spine.max(1), cover, automatic: cover, slots };
    if fixed.get(&group) == Some(&next) {
        return false;
    }
    fixed.insert(group, next);
    true
}

pub(super) fn set_reflow(session: &mut Session, command: &Command) -> CommandResult {
    let enabled = command.params["enabled"].as_bool().ok_or_else(CommandError::invalid_params)?;
    let before = &session.document;
    if before.layout_artboard_origins.is_none() {
        return Err(CommandError::invalid_params());
    }
    if enabled == before.fixed_spreads.is_none() {
        return Ok(json!({"enabled": enabled, "changes": [], "doc_version": session.generation}));
    }
    let mut doc = before.clone();
    if let Some(fixed) = doc.fixed_spreads.take() {
        for page in &mut doc.pages {
            if page.spread_group.and_then(|group| fixed.get(&group)).is_some_and(|spread| spread.automatic) {
                page.spread_group = None;
                page.spread_spine = None;
            }
        }
    } else {
        let mut fixed = BTreeMap::new();
        let cover = crate::data_merge::layout::has_right_cover(before);
        let rects = crate::scene::page_rects(before);
        for range in spread_ranges(before).into_iter().filter(|range| range.end <= before.pages.len()) {
            let spine = spread_spine(before, &range);
            let is_cover = cover && range.start == 0;
            let previous = doc.pages[range.start].spread_group;
            let group = match previous {
                Some(group) if !fixed.contains_key(&group) => group,
                _ => super::next_spread_group(&doc)?,
            };
            let mut slots = Vec::with_capacity(range.len() + usize::from(is_cover));
            if is_cover {
                slots.push(SpreadSlot { page: None, size: before.pages[0].size });
            }
            for index in range {
                let page = &mut doc.pages[index];
                page.spread_group = Some(group);
                let [_, _, width, height] = rects[index];
                slots.push(SpreadSlot { page: Some(page.id), size: [width, height] });
            }
            fixed.insert(group, FixedSpread { spine, cover: is_cover, automatic: previous.is_none(), slots });
        }
        doc.fixed_spreads = Some(fixed);
        doc.preserve_layout_page_order = true;
    }
    super::preserve_content(before, &mut doc);
    let selection = session.selection.ids.clone();
    super::super::document_change::commit(session, REFLOW, doc, selection, json!({"enabled": enabled}))
}

/// Maintain positions when an ordinary page command changes memberships or order.
pub(super) fn reconcile_fixed_spreads(doc: &mut Document) {
    if doc.layout_artboard_origins.is_none() || doc.fixed_spreads.is_none() {
        return;
    }
    let ranges = spread_ranges(doc);
    let Some(mut fixed) = doc.fixed_spreads.take() else { return; };
    let mut used = HashSet::new();
    let mut highest = doc.pages.iter().filter_map(|page| page.spread_group).chain(fixed.keys().copied()).max().unwrap_or(0);
    let page_count = doc.pages.len();
    for range in ranges.into_iter().filter(|range| range.end <= page_count) {
        let previous = doc.pages[range.start].spread_group;
        let group = if let Some(group) = previous.filter(|group| !used.contains(group)) {
            group
        } else {
            let Some(next) = highest.checked_add(1) else { continue; };
            highest = next;
            next
        };
        used.insert(group);
        let pages = &mut doc.pages[range];
        let spread = fixed.entry(group).or_insert_with(|| FixedSpread {
            spine: pages[0].spread_spine.unwrap_or(1),
            cover: false,
            automatic: previous.is_none(),
            slots: Vec::new(),
        });
        let members: HashMap<_, _> = pages.iter().map(|page| (page.id, page.size)).collect();
        for slot in &mut spread.slots {
            if let Some(id) = slot.page {
                if let Some(size) = members.get(&id) {
                    slot.size = *size;
                } else {
                    slot.page = None;
                }
            }
        }
        let present: HashSet<_> = spread.slots.iter().filter_map(|slot| slot.page).collect();
        let mut ordered = pages.iter().filter(|page| present.contains(&page.id));
        for slot in spread.slots.iter_mut().filter(|slot| slot.page.is_some()) {
            if let Some(page) = ordered.next() {
                *slot = SpreadSlot { page: Some(page.id), size: page.size };
            }
        }
        let mut index = usize::from(spread.cover);
        for page in pages {
            page.spread_group = Some(group);
            if let Some(offset) = spread.slots[index..].iter().position(|slot| slot.page == Some(page.id)) {
                index += offset + 1;
            } else {
                let old_count = spread.slots.len();
                let insert = spread.slots[index..].iter().position(|slot| slot.page.is_some())
                    .map_or(old_count, |offset| index + offset);
                spread.slots.insert(insert, SpreadSlot { page: Some(page.id), size: page.size });
                if old_count > 1 && insert < spread.spine {
                    spread.spine += 1;
                }
                index = insert + 1;
            }
        }
        if spread.cover {
            spread.slots[0].size = spread.slots[1].size;
        }
        spread.spine = spread.spine.clamp(1, spread.slots.len().saturating_sub(1).max(1));
    }
    fixed.retain(|group, _| used.contains(group));
    doc.fixed_spreads = Some(fixed);
}

pub(super) struct FillTarget {
    pub group: u64,
    pub slot: usize,
    pub index: usize,
    pub size: [f32; 2],
}

pub fn can_fill(doc: &Document, anchor: PageId, number: usize, count: usize) -> bool {
    find_target(doc, anchor, number, count).is_ok()
}

pub(super) fn fill_target(doc: &Document, value: &Value, count: usize) -> Result<FillTarget, CommandError> {
    let anchor: PageId = serde_json::from_value(value["anchor_id"].clone())
        .map_err(|_| CommandError::invalid_params())?;
    let number = value["number"].as_u64().and_then(|n| usize::try_from(n).ok())
        .ok_or_else(CommandError::invalid_params)?;
    find_target(doc, anchor, number, count)
}

fn find_target(doc: &Document, anchor: PageId, number: usize, count: usize) -> Result<FillTarget, CommandError> {
    let slot = number.checked_sub(1).ok_or_else(CommandError::invalid_params)?;
    let page_index = super::page_index(doc, anchor)?;
    let range = spread_ranges(doc).into_iter().find(|r| r.contains(&page_index))
        .ok_or_else(CommandError::not_found)?;
    let spread = fixed_spread(doc, &range).ok_or_else(CommandError::invalid_params)?;
    let end = slot.checked_add(count).ok_or_else(CommandError::out_of_range)?;
    if count == 0 || (spread.cover && slot == 0) || spread.slots.get(slot..end)
        .is_none_or(|slots| slots.iter().any(|slot| slot.page.is_some())) {
        return Err(CommandError::out_of_range());
    }
    let group = doc.pages[page_index].spread_group.ok_or_else(CommandError::invalid_params)?;
    let index = range.start + spread.slots[..slot].iter().filter(|slot| slot.page.is_some()).count();
    Ok(FillTarget { group, slot, index, size: spread.slots[slot].size })
}

impl FillTarget {
    pub fn set_page(&self, doc: &mut Document, offset: usize, page: PageId, size: [f32; 2]) {
        if let Some(spread) = doc.fixed_spreads.as_mut().and_then(|spreads| spreads.get_mut(&self.group)) {
            spread.slots[self.slot + offset] = SpreadSlot { page: Some(page), size };
        }
    }
}

pub(super) fn move_into_slot(session: &mut Session, ids: Vec<PageId>, value: &Value) -> CommandResult {
    let indices = super::selected_page_indices(&session.document, &ids)?;
    let target = fill_target(&session.document, value, indices.len())?;
    move_placed(session, indices, target.index, target.group, Some(&target))
}

pub(super) fn move_relative(session: &mut Session, ids: Vec<PageId>, value: &Value) -> CommandResult {
    let doc = &session.document;
    let indices = super::selected_page_indices(doc, &ids)?;
    let anchor: PageId = serde_json::from_value(value["anchor_id"].clone())
        .map_err(|_| CommandError::invalid_params())?;
    let after = value["after"].as_bool().ok_or_else(CommandError::invalid_params)?;
    let new_spread = value["new_spread"].as_bool().ok_or_else(CommandError::invalid_params)?;
    let index = super::page_index(doc, anchor)?;
    let spread = spread_ranges(doc).into_iter().find(|range| range.contains(&index))
        .ok_or_else(CommandError::not_found)?;
    let fixed = fixed_spread(doc, &spread).ok_or_else(CommandError::invalid_params)?;
    if indices.contains(&index) || (fixed.cover && !new_spread) {
        return Err(CommandError::invalid_params());
    }
    let (index, group) = if new_spread {
        if fixed.cover && !after {
            return Err(CommandError::invalid_params());
        }
        (if after { spread.end } else { spread.start }, super::next_spread_group(doc)?)
    } else {
        (index + usize::from(after), doc.pages[index].spread_group.ok_or_else(CommandError::invalid_params)?)
    };
    move_placed(session, indices, index, group, None)
}

fn move_placed(
    session: &mut Session, indices: Vec<usize>, index: usize, group: u64, fill: Option<&FillTarget>,
) -> CommandResult {
    let before = &session.document;
    let insertion = index - indices.partition_point(|&source| source < index);
    let mut doc = before.clone();
    let mut moved: Vec<_> = indices.iter().map(|&index| before.pages[index].clone()).collect();
    let ids: Vec<_> = moved.iter().map(|page| page.id).collect();
    let selected: HashSet<_> = ids.iter().copied().collect();
    if let Some(fill) = fill {
        for spread in doc.fixed_spreads.iter_mut().flat_map(|fixed| fixed.values_mut()) {
            for slot in &mut spread.slots {
                if slot.page.is_some_and(|id| selected.contains(&id)) {
                    slot.page = None;
                }
            }
        }
        for (offset, page) in moved.iter().enumerate() {
            fill.set_page(&mut doc, offset, page.id, page.size);
        }
    }
    for page in &mut moved {
        if page.spread_group != Some(group) {
            page.spread_spine = None;
        }
        page.spread_group = Some(group);
    }
    doc.pages.retain(|page| !selected.contains(&page.id));
    doc.pages.splice(insertion..insertion, moved);
    super::preserve_content(before, &mut doc);
    super::super::masters::update_after_move(before, &mut doc)?;
    let selection = super::moved_page_selection(session, &doc);
    super::super::document_change::commit(session, super::MOVE, doc, selection,
        json!({"id": ids[0], "ids": ids, "number": insertion + 1}))
}
