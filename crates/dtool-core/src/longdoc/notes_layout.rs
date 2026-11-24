use crate::longdoc::notes::{note_numbers, NoteKind};
use crate::object::ObjectId;
use crate::Document;

pub const SIDENOTE_GAP: f32 = 6.0;
pub const SIDENOTE_RATIO: f32 = 0.3;

pub fn note_line_height(doc: &Document) -> f32 {
    crate::text_cmd::DEFAULT_SIZE_PT * 1.2 * crate::units::units_per_point(doc.units, doc.dpi)
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotePlacement {
    pub note_id: u64,
    pub kind: NoteKind,
    pub label: String,
    pub text: String,
    pub lines: Vec<String>,
    pub frame: ObjectId,
    pub page: usize,
    pub rect: [f32; 4],
}

fn story_frames(doc: &Document, story: crate::StoryId) -> Vec<(ObjectId, usize, [f32; 2], [f32; 2])> {
    crate::frame::flow_frames(doc, story)
        .into_iter()
        .filter_map(|id| doc.get_object(id))
        .map(|o| (o.id, crate::data_merge::layout::object_page_index(doc, o.id), o.pos, o.size))
        .collect()
}

fn ref_frame<'a>(doc: &Document, story_pos: usize, frames: &'a [(ObjectId, usize, [f32; 2], [f32; 2])]) -> Option<&'a (ObjectId, usize, [f32; 2], [f32; 2])> {
    let range = crate::longdoc::pages::frame_range_for_story_pos(doc, story_pos);
    range.and_then(|r| frames.iter().find(|f| f.0 == r.frame)).or_else(|| frames.first())
}

pub fn layout_notes(doc: &Document, line_height: f32) -> Vec<NotePlacement> {
    layout_notes_with(doc, line_height, |text, _, _| text.lines().map(str::to_owned).collect())
}

pub fn layout_notes_with(
    doc: &Document,
    line_height: f32,
    wrap: impl Fn(&str, f32, f32) -> Vec<String>,
) -> Vec<NotePlacement> {
    let line_height = if line_height > 0.0 { line_height } else { note_line_height(doc) };
    let mut out = Vec::new();
    for kind in [NoteKind::Footnote, NoteKind::Endnote, NoteKind::Sidenote] {
        let mut stacks = [Vec::new(), Vec::new()];
        for (id, _, label) in note_numbers(doc, kind) {
            let Some(note) = doc.notes.notes.iter().find(|n| n.id == id) else { continue };
            let Some((story, _)) = doc.story_at_pos(note.story_pos) else { continue };
            let frames = story_frames(doc, story);
            let text = format!("{} {}", label, note.body);
            let Some((frame, page, rect, lines)) = place(doc, kind, note.story_pos, &frames, &mut stacks, line_height, |width| {
                width.map_or_else(|| text.lines().map(str::to_owned).collect(),
                    |width| wrap(&text, line_height * 0.8, width))
            }) else { continue };
            out.push(NotePlacement { note_id: id, kind, label, text: note.body.clone(), lines, frame, page, rect });
        }
    }
    out
}

fn place(
    doc: &Document,
    kind: NoteKind,
    story_pos: usize,
    frames: &[(ObjectId, usize, [f32; 2], [f32; 2])],
    stacks: &mut [Vec<(ObjectId, f32)>; 2],
    line_height: f32,
    lines: impl Fn(Option<f32>) -> Vec<String>,
) -> Option<(ObjectId, usize, [f32; 4], Vec<String>)> {
    let height = |lines: &[String]| line_height * lines.len().max(1) as f32;
    match kind {
        NoteKind::Footnote => {
            let frame = ref_frame(doc, story_pos, frames)?;
            let lines = lines(None);
            let height = height(&lines);
            let used = stack_take(&mut stacks[0], frame.0, height);
            let bottom = frame.2[1] + frame.3[1] - used;
            Some((frame.0, frame.1, [frame.2[0], bottom - height, frame.2[0] + frame.3[0], bottom], lines))
        }
        NoteKind::Endnote => {
            let lines = lines(None);
            let height = height(&lines);
            if let Some(chosen) = doc.notes.endnote_frame.and_then(|id| doc.get_object(ObjectId(id))) {
                let used = stack_take(&mut stacks[0], chosen.id, height);
                let top = chosen.pos[1] + used;
                let page = crate::data_merge::layout::object_page_index(doc, chosen.id);
                return Some((chosen.id, page, [chosen.pos[0], top, chosen.pos[0] + chosen.size[0], top + height], lines));
            }
            let frame = frames.last()?;
            let used = stack_take(&mut stacks[0], frame.0, height);
            let top = frame.2[1] + frame.3[1] + used;
            Some((frame.0, frame.1, [frame.2[0], top, frame.2[0] + frame.3[0], top + height], lines))
        }
        NoteKind::Sidenote => {
            let frame = ref_frame(doc, story_pos, frames)?;
            let (story, at) = doc.story_at_pos(story_pos)?;
            let from = crate::longdoc::pages::frame_range_for_story_pos(doc, story_pos).map(|r| r.start.min(at)).unwrap_or(0);
            let line = doc
                .stories
                .iter()
                .find(|s| s.id == story)
                .map(|s| s.text.chars().skip(from).take(at - from).filter(|c| *c == '\n').count())
                .unwrap_or(0);
            let gap = SIDENOTE_GAP * crate::units::units_per_point(doc.units, doc.dpi);
            let [page_left, page_top, page_width, page_height] = crate::data_merge::layout::page_rect(doc, frame.1);
            let left_edge = frame.2[0] - gap;
            let right_edge = frame.2[0] + frame.3[0] + gap;
            let left_space = left_edge - (page_left + gap);
            let right_space = page_left + page_width - gap - right_edge;
            let outer = usize::from(frame.1 % 2 == 0);
            let spaces = [left_space, right_space];
            let max_width = (frame.3[0] * SIDENOTE_RATIO).max(line_height);
            let wanted = (frame.2[1] + line as f32 * line_height).max(page_top + gap);
            let limit = page_top + page_height - gap;
            let mut best: Option<(_, _, _, f32)> = None;
            for side in [outer, 1 - outer] {
                let width = max_width.min(spaces[side].max(line_height));
                let x0 = if side == 1 { right_edge } else { left_edge - width };
                let lines = lines(Some(width));
                let top = wanted.max(stack_floor(&stacks[side], frame.0));
                let bottom = top + height(&lines);
                let readable_width = width.max(line_height * 1.6);
                let overflow = (readable_width - spaces[side]).max(0.0) + (bottom - limit).max(0.0);
                if best.as_ref().is_none_or(|best| overflow < best.3) {
                    best = Some((side, [x0, top, x0 + width, bottom], lines, overflow));
                }
                if overflow == 0.0 {
                    break;
                }
            }
            let (side, rect, lines, _) = best?;
            stack_set(&mut stacks[side], frame.0, rect[3]);
            Some((frame.0, frame.1, rect, lines))
        }
    }
}

fn stack_floor(stacks: &[(ObjectId, f32)], frame: ObjectId) -> f32 {
    stacks.iter().find(|(id, _)| *id == frame).map(|(_, y)| *y).unwrap_or(f32::MIN)
}

fn stack_set(stacks: &mut Vec<(ObjectId, f32)>, frame: ObjectId, y: f32) {
    match stacks.iter_mut().find(|(id, _)| *id == frame) {
        Some((_, value)) => *value = y,
        None => stacks.push((frame, y)),
    }
}

fn stack_take(stacks: &mut Vec<(ObjectId, f32)>, frame: ObjectId, height: f32) -> f32 {
    match stacks.iter_mut().find(|(id, _)| *id == frame) {
        Some((_, used)) => {
            let before = *used;
            *used += height;
            before
        }
        None => {
            stacks.push((frame, height));
            0.0
        }
    }
}
