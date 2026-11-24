use std::collections::{HashMap, HashSet, VecDeque};

use crate::clipboard::{ClipboardBundle, object_stories};
use crate::document::PageId;
use crate::longdoc::anchors::{Anchor, Hyperlink, LinkTarget};
use crate::longdoc::helpers::unique_name;
use crate::longdoc::index::{IndexMark, IndexTopic, TOPIC_SEP, ensure_topic, find_topic, normalize_path};
use crate::longdoc::pages::StoryExcerpt;
use crate::{CommandError, Document, ObjectId, StoryId};

struct Span {
    start: usize,
    end: usize,
    destination: usize,
    includes_end: bool,
}

impl Span {
    fn contains_point(&self, point: usize) -> bool {
        point >= self.start && (point < self.end || (self.includes_end && point == self.end))
    }

    fn point(&self, point: usize) -> usize {
        self.destination + (point - self.start)
    }
}

fn story_spans(
    source: &Document,
    destination: &Document,
    pasted: &HashMap<StoryId, StoryId>,
    excerpts: &[StoryExcerpt],
) -> HashMap<StoryId, Vec<Span>> {
    let offsets: HashMap<_, _> = destination.stories.iter().scan(0, |offset, story| {
        let start = *offset;
        *offset += story.text.chars().count() + 1;
        Some((story.id, start))
    }).collect();
    let lengths: HashMap<_, _> = source.stories.iter().map(|story| (story.id, story.text.chars().count())).collect();
    let mut spans = HashMap::<_, Vec<_>>::new();
    for (&id, &length) in &lengths {
        if let Some(&destination) = pasted.get(&id).and_then(|id| offsets.get(id)) {
            spans.entry(id).or_default().push(Span { start: 0, end: length, destination, includes_end: true });
        }
    }
    for excerpt in excerpts {
        if let Some(&destination) = pasted.get(&excerpt.copied).and_then(|id| offsets.get(id)) {
            spans.entry(excerpt.source).or_default().push(Span {
                start: excerpt.start, end: excerpt.end, destination,
                includes_end: lengths.get(&excerpt.source) == Some(&excerpt.end),
            });
        }
    }
    spans
}

fn copy_index_references(source: &[IndexTopic], destination: &mut Vec<IndexTopic>, mut pending: VecDeque<String>) {
    let mut copied = HashSet::new();
    while let Some(path) = pending.pop_front() {
        if !copied.insert(path.clone()) {
            continue;
        }
        if let Some((parent, _)) = path.rsplit_once(TOPIC_SEP) {
            pending.push_back(parent.to_owned());
        }
        let Some(original) = find_topic(source, &path) else { continue; };
        if let Some(topic) = ensure_topic(destination, &path) {
            topic.merge_references(&original.see, &original.see_also);
        }
        // References may be freely typed. Only follow targets that name a source topic.
        for target in original.see.iter().chain(&original.see_also) {
            let target = normalize_path(target);
            if !copied.contains(&target) && find_topic(source, &target).is_some() {
                pending.push_back(target);
            }
        }
    }
}

pub(crate) enum PageTransfer<'a> {
    Import { indices: &'a [usize], at: usize },
    Duplicate { original: PageId, copy: PageId },
}

impl PageTransfer<'_> {
    fn page_number(&self, source: &Document, destination: &Document, number: usize) -> Option<usize> {
        let index = number.checked_sub(1)?;
        match self {
            Self::Import { indices, at } => indices.binary_search(&index).ok()
                .map(|copied| at + copied + 1),
            Self::Duplicate { original, copy } => {
                let id = source.pages.get(index)?.id;
                let target = if id == *original { *copy } else { id };
                destination.pages.iter().position(|page| page.id == target).map(|index| index + 1)
            }
        }
    }
}

pub(crate) fn copy(
    source: &Document,
    prior: &Document,
    destination: &mut Document,
    bundle: &ClipboardBundle,
    objects: &HashMap<ObjectId, ObjectId>,
    excerpts: &[StoryExcerpt],
    transfer: PageTransfer<'_>,
) -> Result<usize, CommandError> {
    let pasted: HashMap<_, _> = bundle.objects.iter().filter_map(|object| {
        let actual = destination.get_object(*objects.get(&object.id)?)?;
        Some(object_stories(object).into_iter().zip(object_stories(actual)))
    }).flatten().collect();
    let spans = story_spans(source, destination, &pasted, excerpts);
    let generated_stories = |id: u64| {
        let story = StoryId(id);
        std::iter::once(story).chain(excerpts.iter().filter_map(move |excerpt| {
            (excerpt.source == story).then_some(excerpt.copied)
        })).filter_map(|id| pasted.get(&id).map(|copy| copy.0))
    };
    for original in &source.toc.tocs {
        destination.toc.tocs.extend(generated_stories(original.story).map(|story| {
            crate::longdoc::toc::TocRecord { story, spec: original.spec.clone() }
        }));
    }
    for &original in &source.index.indexes {
        destination.index.indexes.extend(generated_stories(original));
    }
    let point = |position| {
        let (story, offset) = source.story_at_pos(position)?;
        spans.get(&story)?.iter().find(|span| span.contains_point(offset)).map(|span| span.point(offset))
    };
    if !source.notes.notes.is_empty() {
        let notes = &mut destination.notes;
        let mut next_id = notes.notes.iter().map(|note| note.id).fold(notes.next_id, u64::max);
        for original in &source.notes.notes {
            let Some(story_pos) = point(original.story_pos) else { continue };
            next_id = next_id.checked_add(1).ok_or_else(CommandError::invalid_params)?;
            let mut note = original.clone();
            note.id = next_id;
            note.story_pos = story_pos;
            notes.notes.push(note);
            notes.next_id = next_id;
        }
    }
    let mut names: Vec<_> = prior.anchors.iter().map(|anchor| anchor.name.clone())
        .chain(prior.hyperlinks.iter().filter_map(|link| match &link.target {
            LinkTarget::Anchor(name) => Some(name.clone()), _ => None,
        }))
        .chain(prior.cross_refs.iter().map(|xref| xref.anchor.clone())).collect();
    let mut anchors = HashMap::new();
    for anchor in &source.anchors {
        if let Some(story_pos) = point(anchor.story_pos) {
            let name = unique_name(&names, &anchor.name);
            anchors.insert(anchor.name.as_str(), name.clone());
            names.push(name.clone());
            destination.anchors.push(Anchor { name, story_pos });
        }
    }

    let mut omitted_links = 0;
    for link in &source.hyperlinks {
        let Some((story, start)) = source.story_at_pos(link.range.0) else { continue; };
        let Some((end_story, end)) = source.story_at_pos(link.range.1) else { continue; };
        if story != end_story { continue; }
        let Some(copied) = spans.get(&story) else { continue; };
        let ranges: Vec<_> = copied.iter().filter_map(|span| {
            let lower = start.max(span.start);
            let upper = end.min(span.end);
            (lower < upper || (start == end && span.contains_point(start)))
                .then(|| (span.point(lower), span.point(upper)))
        }).collect();
        if ranges.is_empty() { continue; }
        let target = match &link.target {
            LinkTarget::Anchor(name) => anchors.get(name.as_str()).cloned()
                .or_else(|| matches!(transfer, PageTransfer::Duplicate { .. }).then(|| name.clone()))
                .map(LinkTarget::Anchor),
            LinkTarget::Page(number) => transfer.page_number(source, destination, *number).map(LinkTarget::Page),
            target => Some(target.clone()),
        };
        if let Some(target) = target {
            destination.hyperlinks.extend(ranges.into_iter().map(|range| Hyperlink { range, target: target.clone() }));
        } else {
            omitted_links += 1;
        }
    }
    destination.cross_refs.extend(source.cross_refs.iter().filter_map(|xref| {
        let mut copy = xref.clone();
        copy.anchor = anchors.get(xref.anchor.as_str())?.clone();
        Some(copy)
    }));
    let mut marked_topics = VecDeque::new();
    for mark in &source.index.marks {
        if let Some(story_pos) = point(mark.story_pos) {
            let topic = normalize_path(&mark.topic);
            if ensure_topic(&mut destination.index.topics, &topic).is_some() {
                marked_topics.push_back(topic.clone());
                destination.index.marks.push(IndexMark { topic, story_pos });
            }
        }
    }
    copy_index_references(&source.index.topics, &mut destination.index.topics, marked_topics);
    crate::longdoc::xref::update_all_xrefs(destination)?;
    Ok(omitted_links)
}
