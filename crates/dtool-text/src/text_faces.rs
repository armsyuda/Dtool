use crate::story::{Line, Story};
use crate::FontLibrary;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::{register_text_faces, FaceRun, LineFace};
use dtool_core::Document;
use std::borrow::Cow;

fn previewed(doc: &Document, story: dtool_core::StoryId) -> Cow<'_, Document> {
    crate::font_preview::preview_document(doc, story).map_or(Cow::Borrowed(doc), Cow::Owned)
}

fn face_of(story: &Story, index: usize) -> FaceRun {
    let style = story.chars.iter().find(|run| run.range.start <= index && index < run.range.end).map(|run| run.style.clone()).unwrap_or_default();
    FaceRun { start: 0, end: 0, family: style.family, style: style.style, weight: style.weight, italic: style.italic, size: style.size_pt }
}

pub fn line_faces(story: &Story, lines: &[Line]) -> Vec<LineFace> {
    let chars: Vec<char> = story.text.chars().collect();
    lines
        .iter()
        .map(|line| {
            let start = line.range.start.min(chars.len());
            let end = line.range.end.min(chars.len()).max(start);
            let mut runs: Vec<FaceRun> = Vec::new();
            let mut at = 0usize;
            for index in start..end {
                if chars[index] == '\n' {
                    continue;
                }
                let face = face_of(story, index);
                match runs.last_mut() {
                    Some(last) if last.end == at && last.family == face.family && last.style == face.style && last.weight == face.weight && last.italic == face.italic && last.size == face.size => last.end = at + 1,
                    _ => runs.push(FaceRun { start: at, end: at + 1, ..face }),
                }
                at += 1;
            }
            if runs.is_empty() {
                runs.push(FaceRun { start: 0, end: 0, ..face_of(story, start) });
            }
            LineFace::from_runs(runs)
        })
        .collect()
}

pub fn text_faces_with(lib: &FontLibrary, doc: &Document, id: ObjectId) -> Option<Vec<LineFace>> {
    let object = doc.get_object(id)?;
    match &object.kind {
        ObjectKind::FrameText { story } => {
            let view = previewed(doc, *story);
            let doc = view.as_ref();
            let core = doc.stories.iter().find(|s| s.id == *story)?;
            let flow = crate::frame::shared_doc_flow(lib, doc, *story)?;
            let index = flow.frames.iter().position(|f| *f == id)?;
            let lines = flow.layout.per_frame.get(index)?;
            Some(line_faces(&crate::bridge::story_with_substitutions(core, doc), lines))
        }
        ObjectKind::ArtText { story } => {
            let view = previewed(doc, *story);
            let core = view.stories.iter().find(|s| s.id == *story)?;
            let layout = crate::frame::shared_artistic_layout(lib, &view, *story)?;
            let story = crate::bridge::story_with_substitutions(core, &view);
            Some(line_faces(&story, &layout.1))
        }
        _ => None,
    }
}

pub fn text_faces_shared(doc: &Document, id: ObjectId) -> Option<Vec<LineFace>> {
    text_faces_with(&FontLibrary::shared(), doc, id)
}

pub fn install() -> bool {
    register_text_faces(text_faces_shared)
}
