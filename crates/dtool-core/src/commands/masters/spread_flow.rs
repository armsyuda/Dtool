use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::document::StoryId;
use crate::frame::{FrameChain, StoryOwner, flow_frames, story_owners};
use crate::{Command, Document, ObjectId, ObjectKind};

use super::{InheritedMaster, frame_flow, inherit_flow, translate_flow};

struct Cluster<'a> {
    bindings: Vec<&'a InheritedMaster>,
}

impl Cluster<'_> {
    fn links(&self) -> Vec<(ObjectId, ObjectId)> {
        self.bindings.iter().flat_map(|binding| binding.links.iter().copied()).collect()
    }
}

fn clusters<'a>(doc: &'a Document) -> Vec<Cluster<'a>> {
    let masters = &doc.master_pages;
    let mut result = Vec::new();
    let mut add = |bindings: Vec<(&'a InheritedMaster, u32)>| {
        let mut occurrences = HashMap::new();
        let mut lanes = BTreeMap::<_, Vec<_>>::new();
        for (binding, slot) in bindings {
            if masters.spread(binding.master).is_none() {
                continue;
            }
            // If a source page repeats, its instances belong to separate flows.
            let occurrence = occurrences.entry((binding.master, slot)).or_insert(0usize);
            lanes.entry((masters.spread_id(binding.master).0, slot, *occurrence)).or_default().push(binding);
            *occurrence += 1;
        }
        result.extend(lanes.into_values().filter(|bindings| bindings.len() > 1)
            .map(|bindings| Cluster { bindings }));
    };
    if masters.spreads.is_empty() {
        return result;
    }
    let mut pages = HashMap::<_, Vec<_>>::new();
    for item in &masters.assignments {
        pages.entry(item.page).or_default().push((&item.inherited, item.stack_slot));
    }
    for spread in crate::data_merge::layout::spread_ranges(doc) {
        if let Some(spread) = doc.pages.get(spread) {
            add(spread.iter().filter_map(|page| pages.get(&page.id)).flatten().copied().collect());
        }
    }
    let based: HashMap<_, _> = masters.master_assignments.iter().map(|item| (item.target, (&item.inherited, 0))).collect();
    for spread in &masters.spreads {
        add(spread.pages.iter().filter_map(|page| based.get(page).copied()).collect());
    }
    result
}

pub(super) fn links_for(doc: &Document, instance: ObjectId) -> Option<Vec<(ObjectId, ObjectId)>> {
    clusters(doc).into_iter().find(|cluster| cluster.bindings.iter().any(|binding|
        binding.links.iter().any(|&(_, id)| id == instance))).map(|cluster| cluster.links())
}

pub(crate) fn stitch_new(before: &Document, doc: &mut Document) -> bool {
    if before.next_object_id == doc.next_object_id || doc.master_pages.spreads.is_empty() || doc.frame_chains.is_empty() {
        return false;
    }
    let groups: Vec<_> = clusters(doc).into_iter().map(|cluster| cluster.links().into_iter().collect::<HashMap<_, _>>()).collect();
    let originals = doc.frame_chains.clone();
    let previous_stories: HashSet<_> = before.stories.iter().map(|story| story.id).collect();
    let mut abandoned = HashSet::new();
    for links in groups {
        for chain in &originals {
            let frames: Vec<_> = chain.frames.iter().filter_map(|id| links.get(id).copied()).collect();
            if frames.len() < 2 {
                continue;
            }
            let Some(stories) = frames.iter().map(|&frame| frame_story(doc, frame)).collect::<Option<Vec<_>>>() else { continue };
            if stories.iter().all(|&id| id == stories[0]) {
                continue;
            }
            let existing: HashSet<_> = stories.iter().copied().filter(|story| previous_stories.contains(story)).collect();
            if existing.len() > 1 {
                // Independent local stories may both have edits worth keeping.
                continue;
            }
            let distinct: HashSet<_> = stories.iter().copied().collect();
            if !distinct.iter().all(|&story| {
                let expected: Vec<_> = frames.iter().zip(&stories).filter(|&(_, &id)| id == story)
                    .map(|(&frame, _)| frame).collect();
                flow_frames(doc, story) == expected && story_owners(doc, story).iter().all(|owner|
                    matches!(owner, StoryOwner::Object(id) if expected.contains(id)))
            }) {
                continue;
            }
            let story = existing.into_iter().next().unwrap_or(stories[0]);
            for &frame in &frames {
                if let Some(object) = doc.find_object_mut(frame.0) {
                    object.kind = ObjectKind::FrameText { story };
                }
            }
            doc.frame_chains.retain(|chain| !distinct.contains(&chain.story));
            doc.frame_chains.push(FrameChain { story, frames });
            abandoned.extend(distinct.into_iter().filter(|&id| id != story));
        }
    }
    if abandoned.is_empty() {
        return false;
    }
    let retained = doc.stories.iter().map(|story| story.id).filter(|id| !abandoned.contains(id)).collect();
    crate::longdoc::marks::retain_story_content(doc, &retained);
    crate::longdoc::xref::refresh_xrefs(doc);
    true
}

fn frame_story(doc: &Document, id: ObjectId) -> Option<StoryId> {
    match doc.get_object(id)?.kind {
        ObjectKind::FrameText { story } => Some(story),
        _ => None,
    }
}

pub(super) fn propagate_topology(before: &Document, doc: &mut Document, command: &Command) -> (bool, HashSet<ObjectId>) {
    if !matches!(command.name.as_str(), "text.link" | "text.unlink") {
        return (false, HashSet::new());
    }
    let groups: Vec<_> = clusters(doc).into_iter().map(|cluster| {
        let source_groups = cluster.bindings.iter().map(|binding|
            binding.links.iter().map(|&(source, _)| source).collect::<HashSet<_>>()).collect::<Vec<_>>();
        (source_groups, cluster.links())
    }).collect();
    let mut steps = VecDeque::from([command.clone()]);
    let mut handled = HashSet::new();
    let mut changed = false;
    let mut suppressed = HashSet::new();
    while let Some(step) = steps.pop_front() {
        let field = if step.name == "text.link" { "from" } else { "id" };
        let Some(id) = step.params[field].as_u64().map(ObjectId) else { continue };
        if !handled.insert(id) {
            continue;
        }
        let mut sources = frame_flow(before, id);
        sources.extend(step.params["to"].as_u64().map(ObjectId));
        for (source_groups, links) in &groups {
            if source_groups.iter().filter(|group| sources.iter().any(|id| group.contains(id))).take(2).count() < 2 {
                continue;
            }
            let template: Vec<_> = links.iter().map(|&(source, _)| source).collect();
            let Some(translated) = translate_flow(&step, links) else { continue };
            if !sources.iter().all(|id| template.contains(id)) {
                continue;
            }
            suppressed.extend(links.iter().filter(|(source, _)| sources.contains(source)).map(|&(_, instance)| instance));
            let (updated, _) = inherit_flow(before, doc, links, &template, &step);
            if updated {
                changed = true;
                steps.push_back(translated);
            }
        }
    }
    (changed, suppressed)
}
