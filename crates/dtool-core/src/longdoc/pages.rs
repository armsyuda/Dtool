use crate::data_merge::layout::object_page_index;
use crate::document::Document;
use crate::object::ObjectId;
use crate::table::model::StoryId;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRange {
    pub frame: ObjectId,
    pub start: usize,
    pub end: usize,
}

pub type FlowRanges = fn(&Document, StoryId) -> Option<Vec<FrameRange>>;

static FLOW_RANGES: OnceLock<FlowRanges> = OnceLock::new();

pub fn register_flow_ranges(ranges: FlowRanges) -> bool {
    FLOW_RANGES.set(ranges).is_ok() || FLOW_RANGES.get().is_some()
}

pub fn flow_ranges_ready() -> bool {
    FLOW_RANGES.get().is_some()
}

pub fn estimate_flow_ranges(doc: &Document, story: StoryId) -> Vec<FrameRange> {
    let Some(item) = doc.stories.iter().find(|s| s.id == story) else { return Vec::new() };
    let frames = crate::frame::flow_frames(doc, story);
    let total = item.text.chars().count();
    let size = crate::text_cmd::size_pt_of(item, crate::text_cmd::DEFAULT_SIZE_PT) * crate::units::units_per_point(doc.units, doc.dpi);
    let per_char = (size * 0.55 * size * 1.2).max(1.0);
    let mut start = 0usize;
    let mut out = Vec::with_capacity(frames.len());
    for (i, id) in frames.iter().enumerate() {
        let capacity = doc
            .get_object(*id)
            .map(|o| (o.size[0].max(0.0) * o.size[1].max(0.0) / per_char).floor() as usize)
            .unwrap_or(0);
        let end = if i + 1 == frames.len() { total } else { (start + capacity).min(total) };
        out.push(FrameRange { frame: *id, start, end });
        start = end;
    }
    out
}


fn exact_flow_ranges(doc: &Document, story: StoryId) -> Option<Vec<FrameRange>> {
    FLOW_RANGES.get().and_then(|ranges| ranges(doc, story)).filter(|ranges| !ranges.is_empty())
}

pub fn flow_ranges(doc: &Document, story: StoryId) -> Vec<FrameRange> {
    exact_flow_ranges(doc, story).unwrap_or_else(|| estimate_flow_ranges(doc, story))
}

pub(crate) struct StoryExcerpt {
    pub source: StoryId,
    pub copied: StoryId,
    pub start: usize,
    pub end: usize,
}

pub(crate) fn restrict_page_copy(doc: &Document, bundle: &mut crate::clipboard::ClipboardBundle) -> Vec<StoryExcerpt> {
    use crate::clipboard::object_stories;
    use crate::frame::FrameChain;
    use crate::ObjectKind;

    let mut next_story = doc.stories.iter().map(|story| story.id.0).max().unwrap_or_default();
    let mut excerpts = Vec::new();
    let mut chains = Vec::new();
    for chain in std::mem::take(&mut bundle.frame_chains) {
        let frames = crate::frame::flow_frames(doc, chain.story);
        if frames == chain.frames {
            chains.push(chain);
            continue;
        }
        let Some(layout) = exact_flow_ranges(doc, chain.story).filter(|layout| {
            layout.iter().map(|range| range.frame).eq(frames.iter().copied())
        }) else {
            chains.push(chain);
            continue;
        };
        let Some(original) = bundle.stories.iter().find(|story| story.id == chain.story).cloned() else {
            chains.push(chain);
            continue;
        };
        let segments = layout.split(|range| !chain.frames.contains(&range.frame)).filter(|run| !run.is_empty());
        for segment in segments {
            next_story += 1;
            let id = StoryId(next_story);
            let Some(first) = segment.first() else { continue };
            let Some(last) = segment.last() else { continue };
            let end = last.end.max(first.start);
            let text = original.text.chars().skip(first.start).take(end - first.start).collect();
            let (ranges, range_map) = crop_runs(&original.ranges, first.start, end);
            let (paras, _) = crop_runs(&original.paras, first.start, end);
            let specials = original.specials.iter().filter(|(at, _)| *at >= first.start && *at < end)
                .map(|(at, kind)| (*at - first.start, *kind)).collect();
            let frames: Vec<_> = segment.iter().map(|range| range.frame).collect();
            for object in bundle.objects.iter_mut().filter(|object| frames.contains(&object.id)) {
                if let ObjectKind::FrameText { story } = &mut object.kind {
                    *story = id;
                }
            }
            bundle.swatch_bindings.retain_mut(|binding| {
                if binding.story != Some(chain.story) || !frames.contains(&binding.object) {
                    return true;
                }
                binding.story = Some(id);
                if let Some(index) = binding.range {
                    binding.range = range_map.get(index).copied().flatten();
                    return binding.range.is_some();
                }
                true
            });
            bundle.stories.push(crate::document::Story { id, text, ranges, specials, paras });
            excerpts.push(StoryExcerpt { source: chain.story, copied: id, start: first.start, end });
            if frames.len() > 1 {
                chains.push(FrameChain { story: id, frames });
            }
        }
    }
    bundle.frame_chains = chains;
    let used: std::collections::HashSet<_> = bundle.objects.iter().flat_map(object_stories).collect();
    bundle.stories.retain(|story| used.contains(&story.id));
    bundle.text_links.retain(|link| used.contains(&link.story));
    excerpts
}

fn crop_runs(runs: &[serde_json::Value], start: usize, end: usize) -> (Vec<serde_json::Value>, Vec<Option<usize>>) {
    let mut cropped = Vec::new();
    let map = runs.iter().map(|run| {
        let mut run = run.clone();
        let Some(values) = run.as_object_mut() else { return None };
        if !values.contains_key("start") && !values.contains_key("end") {
            let index = cropped.len();
            cropped.push(run);
            return Some(index);
        }
        let left = values.get("start").and_then(serde_json::Value::as_u64).unwrap_or_default() as usize;
        let right = values.get("end").and_then(serde_json::Value::as_u64).unwrap_or(left as u64) as usize;
        let low = start.max(left);
        let high = end.min(right);
        if low >= high && !(left == right && left >= start && left <= end) {
            return None;
        }
        values.insert("start".into(), serde_json::json!(low - start));
        values.insert("end".into(), serde_json::json!(high - start));
        let index = cropped.len();
        cropped.push(run);
        Some(index)
    }).collect();
    (cropped, map)
}

pub fn pick_range(ranges: &[FrameRange], at: usize) -> Option<FrameRange> {
    ranges
        .iter()
        .find(|r| at >= r.start && at < r.end)
        .or_else(|| ranges.iter().rev().find(|r| at >= r.start && r.end > r.start))
        .or_else(|| ranges.iter().rev().find(|r| at >= r.start))
        .or_else(|| ranges.first())
        .copied()
}

pub fn frame_range_for_story_pos(doc: &Document, story_pos: usize) -> Option<FrameRange> {
    let (story, at) = doc.story_at_pos(story_pos)?;
    pick_range(&flow_ranges(doc, story), at)
}

pub fn frame_for_story_pos(doc: &Document, story_pos: usize) -> Option<(ObjectId, usize)> {
    let range = frame_range_for_story_pos(doc, story_pos)?;
    Some((range.frame, object_page_index(doc, range.frame)))
}

pub fn page_for_story_pos(doc: &Document, story_pos: usize) -> usize {
    if let Some((_, page)) = frame_for_story_pos(doc, story_pos) {
        return page;
    }
    let Some((story, _)) = doc.story_at_pos(story_pos) else { return 0; };
    doc.objects.iter().find(|object| crate::clipboard::object_stories(object).contains(&story))
        .map_or(0, |object| object_page_index(doc, object.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Story, Units};
    use crate::frame::FrameChain;
    use crate::longdoc::toc::{build_toc, TocSpec};
    use crate::object::{Object, ObjectKind};
    use serde_json::json;

    const BLOCK: usize = 210;

    fn threaded_doc() -> (Document, Vec<usize>) {
        let page = [400.0, 300.0];
        let mut doc = Document::new(page, Units::Px, 72.0, 5);
        let story = StoryId(1);
        let mut text = String::new();
        let mut paras = Vec::new();
        let mut heads = Vec::new();
        for i in 0..3 {
            let start = text.chars().count();
            let head = format!("Head{}", i + 1);
            text.push_str(&head);
            text.push('\n');
            paras.push(json!({ "start": start, "end": start + head.chars().count(), "style": "H" }));
            heads.push(start);
            let fill = BLOCK - head.chars().count() - 2;
            text.push_str(&"a".repeat(fill));
            text.push('\n');
        }
        doc.stories.push(Story { id: story, text, ranges: Vec::new(), specials: Vec::new(), paras });
        let mut frames = Vec::new();
        for p in [0usize, 2, 4] {
            let id = doc.create_object(Object {
                pos: [p as f32 * page[0] + 20.0, 20.0],
                size: [200.0, 100.0],
                kind: ObjectKind::FrameText { story },
                frame: Some(crate::FrameProps::from_rect([200.0, 100.0])),
                ..Object::default()
            });
            frames.push(id);
        }
        doc.frame_chains.push(FrameChain { story, frames });
        (doc, heads)
    }

    #[test]
    fn threaded_story_maps_to_chain_pages() {
        let (doc, heads) = threaded_doc();
        let pages: Vec<usize> = heads.iter().map(|at| page_for_story_pos(&doc, doc.story_pos(StoryId(1), *at))).collect();
        assert_eq!(pages, vec![0, 2, 4]);
        assert_eq!(doc.page_index_for_story_pos(doc.story_pos(StoryId(1), heads[2])), 4);
    }

    #[test]
    fn toc_pages_follow_threaded_frames() {
        let (doc, _) = threaded_doc();
        let spec = TocSpec { styles: vec![("H".to_string(), 1)], ..TocSpec::default() };
        let labels: Vec<String> = build_toc(&doc, &spec).into_iter().map(|e| e.page_label).collect();
        assert_eq!(labels, vec!["1", "3", "5"]);
    }
}
