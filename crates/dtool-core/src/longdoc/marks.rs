use crate::document::Document;
use crate::StoryId;
use crate::longdoc::notes::{note_numbers, NoteKind};
use crate::object::ObjectId;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::OnceLock};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretPoint {
    pub frame: ObjectId,
    pub point: [f32; 2],
    pub line_height: f32,
}

pub type CaretLocator = fn(&Document, usize) -> Option<CaretPoint>;

static CARET_LOCATOR: OnceLock<CaretLocator> = OnceLock::new();

pub(crate) fn shift_after_story_rebuild(doc: &mut Document, start: usize, old_len: usize, new_len: usize) {
    if old_len == new_len {
        return;
    }
    relocate_marks(doc, |position, _| {
        Some(if position < start {
            position
        } else if position <= start + old_len {
            start + (position - start).min(new_len)
        } else {
            (position - old_len).saturating_add(new_len)
        })
    });
}

pub(crate) fn shift_after_text_edit(doc: &mut Document, start: usize, removed: usize, inserted: usize) {
    if removed == 0 && inserted == 0 {
        return;
    }
    let end = start + removed;
    relocate_marks(doc, |position, stay_at_start| {
        Some(if position < start || (stay_at_start && position == start) {
            position
        } else if position < end {
            start + (position - start).min(inserted)
        } else {
            (position - removed).saturating_add(inserted)
        })
    });
}

pub(crate) fn retain_story_content(doc: &mut Document, retained: &HashSet<StoryId>) {
    if StoryMarks::present(doc) && doc.stories.iter().any(|story| !retained.contains(&story.id)) {
        let mut old_offset = 0;
        let mut new_offset = 0;
        let mut spans = Vec::with_capacity(doc.stories.len());
        for story in &doc.stories {
            let length = story.text.chars().count() + 1;
            let destination = retained.contains(&story.id).then_some(new_offset);
            spans.push((old_offset, destination));
            old_offset += length;
            if destination.is_some() {
                new_offset += length;
            }
        }
        relocate_marks(doc, |position, _| {
            if position >= old_offset {
                return None;
            }
            let at = spans.partition_point(|&(start, _)| start <= position);
            let (start, destination) = spans[at - 1];
            destination.map(|offset| offset + (position - start))
        });
    }
    doc.stories.retain(|story| retained.contains(&story.id));
}

pub(crate) fn merge_story_marks(doc: &mut Document, from: (usize, usize), into: (usize, usize)) {
    let (source, source_len) = from;
    let (target, target_len) = into;
    let source_end = source + source_len;
    let target_end = target + target_len;
    let target_after = if source < target { target - source_len - 1 } else { target };
    relocate_marks(doc, |position, _| {
        if (source..=source_end).contains(&position) {
            return Some(target_after + target_len + (position - source));
        }
        let removed = if position > source_end { source_len + 1 } else { 0 };
        let added = if position > target_end { source_len } else { 0 };
        Some((position - removed).saturating_add(added))
    });
}

fn relocate_marks(doc: &mut Document, relocate: impl Fn(usize, bool) -> Option<usize>) {
    macro_rules! relocate_points {
        ($items:expr) => {
            $items.retain_mut(|item| {
                let Some(position) = relocate(item.story_pos, false) else { return false };
                item.story_pos = position;
                true
            });
        };
    }
    relocate_points!(doc.anchors);
    relocate_points!(doc.notes.notes);
    relocate_points!(doc.index.marks);
    doc.hyperlinks.retain_mut(|link| {
        let (start, end) = link.range;
        let (Some(start), Some(end)) = (relocate(start, false), relocate(end, end > start)) else { return false };
        if end < start {
            return false;
        }
        link.range = (start, end);
        true
    });
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StoryMarks {
    anchors: Vec<super::anchors::Anchor>,
    hyperlinks: Vec<super::anchors::Hyperlink>,
    notes: Vec<super::notes::Note>,
    index_marks: Vec<super::index::IndexMark>,
    cross_refs: Vec<super::xref::CrossRef>,
}

impl StoryMarks {
    pub(crate) fn present(doc: &Document) -> bool {
        !doc.anchors.is_empty() || !doc.hyperlinks.is_empty() || !doc.notes.notes.is_empty()
            || !doc.index.marks.is_empty() || !doc.cross_refs.is_empty()
    }

    pub(crate) fn capture(doc: &Document) -> Option<Self> {
        if !Self::present(doc) {
            return None;
        }
        Some(Self::snapshot(doc))
    }

    pub(crate) fn snapshot(doc: &Document) -> Self {
        Self {
            anchors: doc.anchors.clone(),
            hyperlinks: doc.hyperlinks.clone(),
            notes: doc.notes.notes.clone(),
            index_marks: doc.index.marks.clone(),
            cross_refs: doc.cross_refs.clone(),
        }
    }

    pub(crate) fn restore(self, doc: &mut Document) {
        doc.anchors = self.anchors;
        doc.hyperlinks = self.hyperlinks;
        doc.notes.notes = self.notes;
        doc.index.marks = self.index_marks;
        doc.cross_refs = self.cross_refs;
    }
}

pub fn register_caret_locator(locator: CaretLocator) -> bool {
    CARET_LOCATOR.set(locator).is_ok() || CARET_LOCATOR.get().is_some()
}

pub fn estimate_caret_point(doc: &Document, story_pos: usize) -> Option<CaretPoint> {
    let (story, at) = doc.story_at_pos(story_pos)?;
    let item = doc.stories.iter().find(|s| s.id == story)?;
    let range = crate::longdoc::pages::frame_range_for_story_pos(doc, story_pos)?;
    let object = doc.get_object(range.frame)?;
    let size = crate::text_cmd::size_pt_of(item, crate::text_cmd::DEFAULT_SIZE_PT) * crate::units::units_per_point(doc.units, doc.dpi);
    let line_height = size * 1.2;
    let from = range.start.min(at);
    let before: Vec<char> = item.text.chars().skip(from).take(at - from).collect();
    let line = before.iter().filter(|c| **c == '\n').count();
    let column = before.iter().rev().take_while(|c| **c != '\n').count();
    let x = (column as f32 * size * 0.5).min(object.size[0].max(0.0));
    Some(CaretPoint { frame: range.frame, point: [object.pos[0] + x, object.pos[1] + line as f32 * line_height], line_height })
}

pub fn caret_point(doc: &Document, story_pos: usize) -> Option<CaretPoint> {
    CARET_LOCATOR
        .get()
        .and_then(|locate| locate(doc, story_pos))
        .or_else(|| estimate_caret_point(doc, story_pos))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    NoteRef,
    NoteMark,
    IndexMark,
}

impl MarkKind {
    pub fn role(&self) -> &'static str {
        match self {
            MarkKind::NoteRef => "NoteRef",
            MarkKind::NoteMark => "NoteMark",
            MarkKind::IndexMark => "IndexMark",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarkPlacement {
    pub kind: MarkKind,
    pub frame: ObjectId,
    pub rect: [f32; 4],
    pub label: String,
}

fn bar(kind: MarkKind, caret: CaretPoint, label: String) -> MarkPlacement {
    let [x, y] = caret.point;
    MarkPlacement { kind, frame: caret.frame, rect: [x - 0.75, y, x + 0.75, y + caret.line_height], label }
}

pub fn layout_marks(doc: &Document) -> Vec<MarkPlacement> {
    let mut out = Vec::new();
    for kind in [NoteKind::Footnote, NoteKind::Endnote, NoteKind::Sidenote] {
        for (id, _, label) in note_numbers(doc, kind) {
            let Some(note) = doc.notes.notes.iter().find(|n| n.id == id) else { continue };
            let Some(caret) = caret_point(doc, note.story_pos) else { continue };
            let [x, y] = caret.point;
            let height = caret.line_height * 0.6;
            let width = label.chars().count().max(1) as f32 * caret.line_height * 0.35;
            if doc.notes.show_marks {
                out.push(bar(MarkKind::NoteMark, caret, String::new()));
            }
            out.push(MarkPlacement { kind: MarkKind::NoteRef, frame: caret.frame, rect: [x, y, x + width, y + height], label });
        }
    }
    if doc.index.show_marks {
        for mark in &doc.index.marks {
            if let Some(caret) = caret_point(doc, mark.story_pos) {
                out.push(bar(MarkKind::IndexMark, caret, mark.topic.clone()));
            }
        }
    }
    out
}
