use crate::frame::layout_doc_flow;
use crate::FontLibrary;
use dtool_core::text_cmd::{fill_story, filler_text, register_filler_fit};
use dtool_core::{Document, StoryId};

pub const MAX_FILLER_WORDS: usize = 20_000;

pub struct FillerProbe {
    doc: Document,
    story: StoryId,
}

impl FillerProbe {
    pub fn new(doc: &Document, story: StoryId) -> Option<Self> {
        doc.stories.iter().any(|s| s.id == story).then(|| Self { doc: doc.clone(), story })
    }

    pub fn overflows(&mut self, lib: &FontLibrary, text: String) -> bool {
        let story = self.story;
        match self.doc.stories.iter_mut().find(|s| s.id == story) {
            Some(target) => fill_story(target, text),
            None => return true,
        }
        layout_doc_flow(lib, &self.doc, story).map_or(true, |flow| flow.layout.overset.is_some())
    }
}

pub fn fit_with(lib: &FontLibrary, doc: &Document, story: StoryId, source: &str) -> Option<String> {
    if source.split_whitespace().next().is_none() {
        return None;
    }
    let mut probe = FillerProbe::new(doc, story)?;
    layout_doc_flow(lib, doc, story)?;
    if probe.overflows(lib, filler_text(source, 1)) {
        return Some(String::new());
    }
    let mut low = 1;
    let mut high = 2;
    while high <= MAX_FILLER_WORDS && !probe.overflows(lib, filler_text(source, high)) {
        low = high;
        high *= 2;
    }
    if high > MAX_FILLER_WORDS {
        return Some(filler_text(source, low));
    }
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if probe.overflows(lib, filler_text(source, mid)) {
            high = mid;
        } else {
            low = mid;
        }
    }
    Some(filler_text(source, low))
}

fn fit_shared(doc: &Document, story: StoryId, source: &str) -> Option<String> {
    fit_with(&FontLibrary::shared(), doc, story, source)
}

pub fn install() -> bool {
    register_filler_fit(fit_shared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtool_core::object::{Object, ObjectId, ObjectKind};
    use dtool_core::Story;

    #[test]
    fn filler_fills_frame_to_bottom_without_overflow() {
        let lib = FontLibrary::shared();
        let mut doc = Document::new([400.0, 300.0], dtool_core::document::Units::Px, 72.0, 1);
        let story = StoryId(1);
        doc.stories.push(Story { id: story, text: String::new(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
        let _ = doc.create_object(Object {
            id: ObjectId(1),
            pos: [0.0, 0.0],
            size: [200.0, 120.0],
            kind: ObjectKind::FrameText { story },
            frame: Some(dtool_core::FrameProps::from_rect([200.0, 120.0])),
            ..Object::default()
        });
        let source = "lorem ipsum dolor sit amet";
        let fill = fit_with(&lib, &doc, story, source).unwrap();
        let words = fill.split_whitespace().count();
        assert!(words > 1);
        let mut probe = FillerProbe::new(&doc, story).unwrap();
        assert!(!probe.overflows(&lib, fill));
        assert!(probe.overflows(&lib, filler_text(source, words + 1)));
    }
}
