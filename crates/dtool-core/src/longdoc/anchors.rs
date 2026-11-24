use crate::command::Session;
use crate::document::Document;
use crate::table::model::StoryId;
use crate::text_cmd::push_change;
use crate::CommandError;
use dtool_protocol::{Command, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub name: String,
    pub story_pos: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LinkTarget {
    Url(String),
    Anchor(String),
    Page(usize),
    Email(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hyperlink {
    pub range: (usize, usize),
    pub target: LinkTarget,
}

impl Document {
    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    pub fn add_anchor(&mut self, anchor: Anchor) {
        self.anchors.push(anchor);
    }

    pub fn find_anchor(&self, name: &str) -> Option<&Anchor> {
        self.anchors.iter().find(|a| a.name == name)
    }

    pub fn find_anchor_mut(&mut self, name: &str) -> Option<&mut Anchor> {
        self.anchors.iter_mut().find(|a| a.name == name)
    }

    pub fn remove_anchor(&mut self, name: &str) -> bool {
        let before = self.anchors.len();
        self.anchors.retain(|a| a.name != name);
        self.anchors.len() != before
    }

    pub fn hyperlinks(&self) -> &[Hyperlink] {
        &self.hyperlinks
    }

    pub fn add_hyperlink(&mut self, link: Hyperlink) {
        self.hyperlinks.push(link);
    }

    pub fn set_hyperlink(&mut self, index: usize, link: Hyperlink) -> Result<(), CommandError> {
        let slot = self.hyperlinks.get_mut(index).ok_or_else(CommandError::not_found)?;
        *slot = link;
        Ok(())
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn story_pos(&self, story: StoryId, at: usize) -> usize {
        let mut offset = 0usize;
        for item in &self.stories {
            if item.id == story {
                return offset + at.min(item.text.chars().count());
            }
            offset += item.text.chars().count() + 1;
        }
        offset
    }

    pub fn story_at_pos(&self, story_pos: usize) -> Option<(StoryId, usize)> {
        let mut offset = 0usize;
        for item in &self.stories {
            let len = item.text.chars().count();
            if story_pos <= offset + len {
                return Some((item.id, story_pos - offset));
            }
            offset += len + 1;
        }
        None
    }

    pub fn page_index_for_story_pos(&self, story_pos: usize) -> usize {
        crate::longdoc::pages::page_for_story_pos(self, story_pos)
    }
}

pub fn insert_anchor(doc: &mut Document, name: String, story_pos: usize) -> Result<Anchor, CommandError> {
    if name.trim().is_empty() {
        return Err(CommandError::invalid_params());
    }
    let existing_names: Vec<String> = doc.anchors().iter().map(|a| a.name.clone()).collect();
    let unique = crate::longdoc::helpers::unique_name(&existing_names, &name);
    let anchor = Anchor { name: unique, story_pos };
    doc.add_anchor(anchor.clone());
    super::xref::refresh_xrefs(doc);
    Ok(anchor)
}

pub fn rename_anchor(doc: &mut Document, old_name: String, new_name: String) -> Result<(), CommandError> {
    if new_name.trim().is_empty() {
        return Err(CommandError::invalid_params());
    }
    let existing_names: Vec<String> = doc.anchors().iter().filter(|a| a.name != old_name).map(|a| a.name.clone()).collect();
    let unique = crate::longdoc::helpers::unique_name(&existing_names, &new_name);
    let anchor = doc.find_anchor_mut(&old_name).ok_or_else(CommandError::not_found)?;
    anchor.name = unique.clone();
    for link in doc.hyperlinks.iter_mut() {
        if link.target == LinkTarget::Anchor(old_name.clone()) {
            link.target = LinkTarget::Anchor(unique.clone());
        }
    }
    for xref in &mut doc.cross_refs {
        if xref.anchor == old_name {
            xref.anchor.clone_from(&unique);
        }
    }
    super::xref::refresh_xrefs(doc);
    Ok(())
}

pub fn delete_anchor(doc: &mut Document, name: String) -> Result<(), CommandError> {
    if doc.remove_anchor(&name) {
        super::xref::refresh_xrefs(doc);
        Ok(())
    } else {
        Err(CommandError::not_found())
    }
}

pub fn find_anchor<'a>(doc: &'a Document, name: &str) -> Option<&'a Anchor> {
    doc.find_anchor(name)
}

pub fn list_anchors_by_page(doc: &Document) -> Vec<(usize, Vec<Anchor>)> {
    let mut map: std::collections::BTreeMap<usize, Vec<Anchor>> = std::collections::BTreeMap::new();
    for anchor in doc.anchors() {
        let page_idx = doc.page_index_for_story_pos(anchor.story_pos);
        map.entry(page_idx).or_default().push(anchor.clone());
    }
    map.into_iter().collect()
}

pub fn insert_hyperlink(doc: &mut Document, range: (usize, usize), target: LinkTarget) -> Result<(), CommandError> {
    if range.1 < range.0 {
        return Err(CommandError::invalid_params());
    }
    doc.add_hyperlink(Hyperlink { range, target });
    Ok(())
}

pub fn list_hyperlinks(doc: &Document) -> Vec<Hyperlink> {
    doc.hyperlinks().to_vec()
}

pub(crate) fn links_snapshot(doc: &Document) -> Value {
    json!({
        "anchors": doc.anchors,
        "hyperlinks": doc.hyperlinks,
        "cross_refs": doc.cross_refs
    })
}

pub(crate) fn links_restore(doc: &mut Document, snap: &Value) -> Result<(), CommandError> {
    if let Some(v) = snap.get("anchors") {
        doc.anchors = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(v) = snap.get("hyperlinks") {
        doc.hyperlinks = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(v) = snap.get("cross_refs") {
        doc.cross_refs = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    Ok(())
}

pub(crate) fn record(session: &mut Session, command: &Command, before: Value) {
    let undo = Command::with_params(LINKS_RESTORE, json!({ "snapshot": before }));
    push_change(session, &command.name, command.clone(), undo);
}

pub const LINKS_RESTORE: &str = "longdoc.links.restore";

fn param_str(command: &Command, key: &str) -> Result<String, CommandError> {
    command.params.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(CommandError::invalid_params)
}

fn param_usize(command: &Command, key: &str) -> Option<usize> {
    command.params.get(key).and_then(Value::as_u64).map(|v| v as usize)
}

pub fn parse_target(params: &Value) -> Result<LinkTarget, CommandError> {
    let value = params.get("target").ok_or_else(CommandError::invalid_params)?;
    if let Ok(target) = serde_json::from_value::<LinkTarget>(value.clone()) {
        return Ok(target);
    }
    let kind = params.get("type").and_then(Value::as_str).unwrap_or("url");
    let text = value.as_str().map(str::to_string).or_else(|| value.as_u64().map(|v| v.to_string())).ok_or_else(CommandError::invalid_params)?;
    match kind.to_ascii_lowercase().as_str() {
        "url" => Ok(LinkTarget::Url(text)),
        "anchor" => Ok(LinkTarget::Anchor(text)),
        "page" => text.trim().parse().map(LinkTarget::Page).map_err(|_| CommandError::invalid_params()),
        "email" => Ok(LinkTarget::Email(text)),
        _ => Err(CommandError::invalid_params()),
    }
}

fn caret_pos(session: &Session) -> Option<usize> {
    session.text_edit.as_ref().map(|edit| session.document.story_pos(edit.story, edit.caret))
}

fn selection_range(session: &Session) -> Option<(usize, usize)> {
    session.text_edit.as_ref().map(|edit| {
        let a = session.document.story_pos(edit.story, edit.anchor);
        let b = session.document.story_pos(edit.story, edit.caret);
        (a.min(b), a.max(b))
    })
}

fn link_json(index: usize, link: &Hyperlink) -> Value {
    json!({ "index": index, "start": link.range.0, "end": link.range.1, "target": link.target })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        "anchor.insert" => Some(session.has_document && (command.params.get("story_pos").is_some() || session.text_edit.is_some())),
        "hyperlink.insert" => Some(session.has_document && (command.params.get("start").is_some() || session.text_edit.is_some())),
        "anchor.rename" | "anchor.delete" | "anchor.list" | "hyperlink.list" | "hyperlink.update" | "hyperlink.delete" | LINKS_RESTORE => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let result = match command.name.as_str() {
        "anchor.insert" => (|| {
            let name = param_str(command, "name")?;
            let pos = param_usize(command, "story_pos").or_else(|| caret_pos(session)).ok_or_else(CommandError::invalid_params)?;
            let before = links_snapshot(&session.document);
            let anchor = insert_anchor(&mut session.document, name, pos)?;
            record(session, command, before);
            Ok(json!({ "name": anchor.name, "story_pos": anchor.story_pos, "doc_version": session.generation }))
        })(),
        "anchor.rename" => (|| {
            let name = param_str(command, "name")?;
            let new_name = param_str(command, "new_name")?;
            let before = links_snapshot(&session.document);
            rename_anchor(&mut session.document, name, new_name)?;
            record(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "anchor.delete" => (|| {
            let name = param_str(command, "name")?;
            let before = links_snapshot(&session.document);
            delete_anchor(&mut session.document, name)?;
            record(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "anchor.list" => Ok(json!({
            "anchors": list_anchors_by_page(&session.document)
                .into_iter()
                .map(|(page, anchors)| json!({ "page": page, "anchors": anchors }))
                .collect::<Vec<_>>()
        })),
        "hyperlink.insert" => (|| {
            let target = parse_target(&command.params)?;
            let range = match (param_usize(command, "start"), param_usize(command, "end")) {
                (Some(start), Some(end)) => (start, end),
                _ => selection_range(session).ok_or_else(CommandError::invalid_params)?,
            };
            let before = links_snapshot(&session.document);
            insert_hyperlink(&mut session.document, range, target)?;
            record(session, command, before);
            let index = session.document.hyperlinks.len() - 1;
            Ok(json!({ "index": index, "doc_version": session.generation }))
        })(),
        "hyperlink.update" => (|| {
            let index = param_usize(command, "index").ok_or_else(CommandError::invalid_params)?;
            let current = session.document.hyperlinks.get(index).cloned().ok_or_else(CommandError::not_found)?;
            let target = if command.params.get("target").is_some() { parse_target(&command.params)? } else { current.target };
            let start = param_usize(command, "start").unwrap_or(current.range.0);
            let end = param_usize(command, "end").unwrap_or(current.range.1);
            if end < start {
                return Err(CommandError::invalid_params());
            }
            let before = links_snapshot(&session.document);
            session.document.set_hyperlink(index, Hyperlink { range: (start, end), target })?;
            record(session, command, before);
            Ok(json!({ "index": index, "doc_version": session.generation }))
        })(),
        "hyperlink.delete" => (|| {
            let index = param_usize(command, "index").ok_or_else(CommandError::invalid_params)?;
            if index >= session.document.hyperlinks.len() {
                return Err(CommandError::not_found());
            }
            let before = links_snapshot(&session.document);
            session.document.hyperlinks.remove(index);
            record(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "hyperlink.list" => Ok(json!({
            "hyperlinks": session.document.hyperlinks.iter().enumerate().map(|(i, l)| link_json(i, l)).collect::<Vec<_>>()
        })),
        LINKS_RESTORE => (|| {
            let snap = command.params.get("snapshot").ok_or_else(CommandError::invalid_params)?;
            links_restore(&mut session.document, snap)?;
            crate::text_cmd::bump(session);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        _ => return None,
    };
    Some(result)
}
