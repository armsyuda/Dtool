use crate::command::Session;
use crate::document::Document;
use crate::object::ObjectKind;
use crate::CommandError;
use dtool_protocol::{Command, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MISSING: &str = "[Missing]";
const MAX_CHARS: usize = 50;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossRef {
    pub anchor: String,
    pub format: String,
    pub cached: String,
}

impl CrossRef {
    pub fn new(anchor: String, format: String) -> Self {
        Self {
            anchor,
            format,
            cached: String::new(),
        }
    }
}

impl Document {
    pub fn cross_refs(&self) -> &[CrossRef] {
        &self.cross_refs
    }

    pub fn set_cross_refs(&mut self, xrefs: Vec<CrossRef>) {
        self.cross_refs = xrefs;
    }

    pub fn add_cross_ref(&mut self, xref: CrossRef) {
        self.cross_refs.push(xref);
    }

    pub fn page_for_story_pos(&self, story_pos: usize) -> usize {
        self.page_index_for_story_pos(story_pos)
    }

    pub fn text_at_story_pos(&self, story_pos: usize) -> Option<String> {
        let (story, at) = self.story_at_pos(story_pos)?;
        let story = self.stories.iter().find(|s| s.id == story)?;
        let chars: Vec<char> = story.text.chars().collect();
        let start = chars[..at.min(chars.len())].iter().rposition(|c| *c == '\n').map(|i| i + 1).unwrap_or(0);
        let end = chars[start..].iter().position(|c| *c == '\n').map(|i| start + i).unwrap_or(chars.len());
        Some(chars[start..end].iter().collect::<String>().trim().to_string())
    }

    pub fn caption_for_page(&self, page_index: usize) -> Option<String> {
        self.objects
            .iter()
            .filter(|o| o.parent_id.is_some())
            .filter(|o| crate::data_merge::layout::object_page_index(self, o.id) == page_index)
            .find_map(|o| match &o.kind {
                ObjectKind::Text(text) => Some(text.clone()),
                _ => None,
            })
    }
}

fn truncate(text: String) -> String {
    if text.chars().count() > MAX_CHARS {
        format!("{}...", text.chars().take(MAX_CHARS - 3).collect::<String>())
    } else {
        text
    }
}

pub fn resolve_xref(doc: &Document, xref: &CrossRef) -> Result<String, CommandError> {
    let target_pos = doc.find_anchor(&xref.anchor).map(|a| a.story_pos).ok_or_else(CommandError::not_found)?;
    let page_index = doc.page_for_story_pos(target_pos);
    match xref.format.as_str() {
        "page" => Ok(crate::longdoc::sections::section_page_label(doc, page_index)),
        "anchor_text" => Ok(truncate(doc.text_at_story_pos(target_pos).unwrap_or_default())),
        "caption" => Ok(truncate(doc.caption_for_page(page_index).unwrap_or_default())),
        "page_n" => Ok(format!("Page {}", crate::longdoc::sections::section_page_label(doc, page_index))),
        _ => Err(CommandError::invalid_params()),
    }
}

pub fn update_all_xrefs(doc: &mut Document) -> Result<(), CommandError> {
    refresh_xrefs(doc);
    Ok(())
}

pub(crate) fn refresh_xrefs(doc: &mut Document) {
    let mut refs = std::mem::take(&mut doc.cross_refs);
    for xref in &mut refs {
        xref.cached = resolve_xref(doc, xref).unwrap_or_else(|_| MISSING.to_string());
    }
    doc.set_cross_refs(refs);
}

pub fn insert_xref(doc: &mut Document, anchor: String, format: String) -> Result<(), CommandError> {
    let mut xref = CrossRef::new(anchor, format);
    xref.cached = resolve_xref(doc, &xref)?;
    doc.add_cross_ref(xref);
    Ok(())
}

pub fn list_missing_xrefs(doc: &Document) -> Vec<CrossRef> {
    doc.cross_refs().iter().filter(|x| x.cached == MISSING).cloned().collect()
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        "xref.insert" => Some(session.has_document && !session.document.anchors.is_empty()),
        "xref.update_all" => Some(session.has_document && !session.document.cross_refs.is_empty()),
        "xref.list" => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let result = match command.name.as_str() {
        "xref.insert" => (|| {
            let anchor = command.params.get("anchor").and_then(Value::as_str).ok_or_else(CommandError::invalid_params)?.to_string();
            let format = command.params.get("format").and_then(Value::as_str).unwrap_or("page").to_string();
            let before = crate::longdoc::anchors::links_snapshot(&session.document);
            insert_xref(&mut session.document, anchor, format)?;
            crate::longdoc::anchors::record(session, command, before);
            let cached = session.document.cross_refs.last().map(|x| x.cached.clone()).unwrap_or_default();
            Ok(json!({ "index": session.document.cross_refs.len() - 1, "text": cached, "doc_version": session.generation }))
        })(),
        "xref.update_all" => (|| {
            let before = crate::longdoc::anchors::links_snapshot(&session.document);
            update_all_xrefs(&mut session.document)?;
            crate::longdoc::anchors::record(session, command, before);
            Ok(json!({ "missing": list_missing_xrefs(&session.document).len(), "doc_version": session.generation }))
        })(),
        "xref.list" => Ok(json!({
            "xrefs": session.document.cross_refs,
            "missing": list_missing_xrefs(&session.document)
        })),
        _ => return None,
    };
    Some(result)
}
