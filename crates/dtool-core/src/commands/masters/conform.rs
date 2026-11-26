use std::ops::Range;

use crate::data_merge::layout::{has_right_cover, spread_ranges, spread_spine};
use crate::document::{Page, PageId};
use crate::{CommandError, Document, ObjectId};

use super::super::pages::{self, spreads};

type SourcePage = (ObjectId, [f32; 2]);

struct SpreadPlan {
    range: Range<usize>,
    /// Original page indices in master order. Vacancies here become blank publication pages.
    pages: Vec<Option<usize>>,
    keep_cover: bool,
}

pub(super) struct Conformed {
    pub targets: Vec<(usize, PageId)>,
    pub sources: Vec<ObjectId>,
    pub inserted: Vec<PageId>,
    pub topology_changed: bool,
}

pub(super) struct ConformPlan {
    source: Vec<SourcePage>,
    spine: usize,
    spreads: Vec<SpreadPlan>,
    pub(super) conflict: bool,
}

impl ConformPlan {
    pub(super) fn new(doc: &Document, root: ObjectId, targets: &[(usize, PageId)]) -> Result<Self, CommandError> {
        if doc.layout_artboard_origins.is_none() || targets.is_empty() {
            return Err(CommandError::invalid_params());
        }
        if !doc.master_pages.roots.contains(&root) || doc.master_pages.spread_id(root) != root {
            return Err(CommandError::not_found());
        }
        let source: Vec<_> = doc.master_pages.spread_pages(&root).iter().map(|&id| {
            let size = doc.get_object(id).ok_or_else(CommandError::not_found)?.size;
            pages::valid_page_size(size).then_some((id, size)).ok_or_else(CommandError::invalid_params)
        }).collect::<Result<_, _>>()?;
        let spine = doc.master_pages.spread(root).map_or(0, |spread| spread.spine);
        if source.is_empty() || (source.len() > 1 && !(1..source.len()).contains(&spine)) {
            return Err(CommandError::invalid_params());
        }
        let rects = crate::scene::page_rects(doc);
        let count = source.len();
        let mut plans = Vec::new();
        let mut conflict = false;
        let mut added = 0;
        let cover = has_right_cover(doc);
        let mut cursor = 0;
        for range in spread_ranges(doc) {
            if targets.get(cursor).is_none_or(|&(index, _)| !range.contains(&index)) {
                continue;
            }
            while targets.get(cursor).is_some_and(|&(index, _)| range.contains(&index)) {
                cursor += 1;
            }
            let keep_cover = cover && range.start == 0 && count == 1;
            let mut mapped = vec![None; range.len().div_ceil(count) * count];
            let left = if cover && range.start == 0 {
                0
            } else if let Some(fixed) = spreads::fixed_spread(doc, &range) {
                fixed.slots.iter().take(fixed.spine).filter(|slot| slot.page.is_some()).count()
            } else {
                spread_spine(doc, &range).min(range.len())
            };
            if range.len() < count {
                let left = left.min(spine).max(range.len().saturating_sub(count - spine));
                let right = range.len() - left;
                for (slot, index) in mapped[spine - left..spine + right].iter_mut().zip(range.clone()) {
                    *slot = Some(index);
                }
            } else {
                for (slot, index) in mapped.iter_mut().zip(range.clone()) {
                    *slot = Some(index);
                }
            }
            for (slot, &index) in mapped.iter().enumerate() {
                let Some(index) = index else { continue; };
                let (master, size) = source[slot % count];
                let page = doc.pages[index].id;
                let [_, _, width, height] = rects[index];
                if size != [width, height] && !pages::can_resize(doc, page) {
                    return Err(CommandError::validation("panel.master.conform_unavailable"));
                }
                conflict |= doc.master_pages.on_page(page).any(|binding| binding.master != master);
            }
            added += mapped.len() - range.len();
            if added > pages::MAX_ADDED_PAGES {
                return Err(CommandError::out_of_range());
            }
            plans.push(SpreadPlan { range, pages: mapped, keep_cover });
        }
        if cursor != targets.len() {
            return Err(CommandError::invalid_params());
        }
        Ok(Self { source, spine, spreads: plans, conflict })
    }

    pub(super) fn apply(
        self, before: &Document, doc: &mut Document,
    ) -> Result<Conformed, CommandError> {
        let mut assignments = Vec::new();
        let mut inserted = Vec::new();
        let mut topology_changed = false;
        let mut extra = 0;
        for plan in self.spreads {
            let mut at = plan.range.start + extra;
            let original_group = before.pages[plan.range.start].spread_group.filter(|&group|
                before.pages.get(plan.range.end).is_none_or(|page| page.spread_group != Some(group)) &&
                plan.range.start.checked_sub(1).is_none_or(|index| before.pages[index].spread_group != Some(group)));
            for (row, chunk) in plan.pages.chunks(self.source.len()).enumerate() {
                let group = if plan.keep_cover && before.fixed_spreads.is_none() {
                    None
                } else if row == 0 && original_group.is_some() {
                    original_group
                } else {
                    Some(pages::next_spread_group(doc)?)
                };
                let spine = (self.spine > 0).then_some(self.spine);
                let start = at;
                for (&original, &(source, size)) in chunk.iter().zip(&self.source) {
                    let id = if let Some(index) = original {
                        let id = before.pages[index].id;
                        debug_assert_eq!(doc.pages[at].id, id);
                        id
                    } else {
                        let id = doc.alloc_page_id();
                        pages::insert_page(doc, at, Page::new(id, size));
                        inserted.push(id);
                        extra += 1;
                        topology_changed = true;
                        id
                    };
                    let page = &mut doc.pages[at];
                    topology_changed |= page.spread_group != group || page.spread_spine != spine;
                    page.spread_group = group;
                    page.spread_spine = spine;
                    assignments.push(((at, id), source));
                    at += 1;
                }
                if let Some(group) = group {
                    topology_changed |= spreads::replace_slots(doc, group, start..at, self.spine, plan.keep_cover);
                }
            }
        }
        if topology_changed {
            doc.preserve_layout_page_order = true;
            pages::preserve_content(before, doc);
        }
        let (targets, sources) = assignments.into_iter().unzip();
        Ok(Conformed { targets, sources, inserted, topology_changed })
    }
}
