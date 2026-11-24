pub use dtool_core::longdoc::notes_layout::{NotePlacement, SIDENOTE_GAP, SIDENOTE_RATIO, note_line_height};

use crate::{CharStyle, FontLibrary, Run, Story, TextRange, layout_story};
use dtool_core::Document;

pub fn layout_notes(doc: &Document, line_height: f32) -> Vec<NotePlacement> {
    dtool_core::longdoc::notes_layout::layout_notes_with(doc, line_height, wrap_sidenote)
}

pub struct NoteTextLine {
    pub frame: dtool_core::ObjectId,
    pub baseline: [f32; 2],
    pub size: f32,
    pub text: String,
}

pub fn output_note_lines(doc: &Document) -> Vec<NoteTextLine> {
    use dtool_core::longdoc::marks::{MarkKind, layout_marks};

    if doc.notes.notes.is_empty() {
        return Vec::new();
    }
    let height = note_line_height(doc);
    let size = height * 0.8;
    let mut lines = Vec::new();
    for note in layout_notes(doc, height) {
        lines.extend(note.lines.into_iter().enumerate().map(|(index, text)| NoteTextLine {
            frame: note.frame,
            baseline: [note.rect[0], note.rect[1] + size + index as f32 * height],
            size,
            text,
        }));
    }
    lines.extend(layout_marks(doc).into_iter().filter(|mark| mark.kind == MarkKind::NoteRef)
        .map(|mark| NoteTextLine {
            frame: mark.frame,
            baseline: [mark.rect[0], mark.rect[1]],
            size: mark.rect[3] - mark.rect[1],
            text: mark.label,
        }));
    lines
}

impl NoteTextLine {
    pub fn contours(&self, library: &FontLibrary) -> Vec<Vec<[f32; 2]>> {
        let story = note_story(&self.text, self.size);
        let mut lines = layout_story(library, &story, None);
        for line in &mut lines {
            line.baseline = 0.0;
        }
        crate::canvas_text::line_outlines(library, &story, &lines, self.baseline, |_, _, _| true)
    }
}

fn note_story(text: &str, size: f32) -> Story {
    Story {
        text: text.to_owned(),
        chars: vec![Run {
            range: TextRange::new(0, text.chars().count()),
            style: CharStyle { family: "Malgun Gothic".to_owned(), size_pt: size, ..Default::default() },
        }],
        ..Default::default()
    }
}

fn wrap_sidenote(text: &str, size: f32, width: f32) -> Vec<String> {
    let library = FontLibrary::shared();
    let mut lines = Vec::new();
    // The canvas UI and PDF use different font stacks for plain note text.
    // Allow a little room for differences in glyph advances.
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let scalars: Vec<_> = paragraph.chars().collect();
        let story = note_story(paragraph, size);
        lines.extend(layout_story(&library, &story, Some(width * 0.94)).into_iter().filter_map(|line| {
            let text: String = scalars[line.range.start..line.range.end].iter().collect();
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }));
    }
    lines
}
