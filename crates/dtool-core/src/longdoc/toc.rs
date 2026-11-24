use crate::command::Session;
use crate::document::{Document, Story};
use crate::object::{Object, ObjectKind};
use crate::table::model::StoryId;
use crate::CommandError;
use dtool_protocol::{Command, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_LEVEL: u8 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TocSpec {
    pub styles: Vec<(String, u8)>,
    pub page_numbers: bool,
    pub leader: String,
}

impl Default for TocSpec {
    fn default() -> Self {
        Self {
            styles: Vec::new(),
            page_numbers: true,
            leader: "\t".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TocEntry {
    pub level: u8,
    pub style: String,
    pub text: String,
    pub story_pos: usize,
    pub page: usize,
    pub page_label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TocRecord {
    pub story: u64,
    pub spec: TocSpec,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TocData {
    #[serde(default)]
    pub spec: TocSpec,
    #[serde(default)]
    pub tocs: Vec<TocRecord>,
}

pub fn is_generated(doc: &Document, story: StoryId) -> bool {
    doc.toc.tocs.iter().any(|t| t.story == story.0) || crate::longdoc::index::is_index_story(doc, story)
}

fn para_bounds(run: &Value) -> (usize, usize) {
    let start = run.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
    let end = run.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
    (start, end)
}

fn paragraph_style(run: &Value) -> Option<&str> {
    if run.get("frame").is_some() {
        return None;
    }
    run.get(crate::text_styles::PARA_STYLE_KEY).and_then(Value::as_str)
        .or_else(|| run.get("style").and_then(Value::as_str))
}

pub fn para_style_names(doc: &Document) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for story in doc.stories.iter().filter(|s| !is_generated(doc, s.id)) {
        for run in &story.paras {
            if let Some(name) = paragraph_style(run) {
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    names
}

pub fn build_toc(doc: &Document, spec: &TocSpec) -> Vec<TocEntry> {
    let mut entries = Vec::new();
    for story in doc.stories.iter().filter(|s| !is_generated(doc, s.id)) {
        let styles: Vec<_> = story.paras.iter().filter_map(|run| {
            Some((para_bounds(run), paragraph_style(run)?))
        }).collect();
        if styles.is_empty() { continue; }
        let mut offset = 0;
        for paragraph in story.text.split(['\n', '\u{2029}']) {
            let text = paragraph.trim();
            let start = offset + paragraph.chars().take_while(|ch| ch.is_whitespace()).count();
            offset += paragraph.chars().count() + 1;
            if text.is_empty() { continue; }
            let Some(&(_, name)) = styles.iter().rev().find(|&&((low, high), _)| low <= start && start < high) else { continue };
            let Some((_, level)) = spec.styles.iter().find(|(s, _)| s == name) else { continue };
            let story_pos = doc.story_pos(story.id, start);
            let page = doc.page_index_for_story_pos(story_pos);
            entries.push(TocEntry {
                level: (*level).clamp(1, MAX_LEVEL),
                style: name.to_string(),
                text: text.to_owned(),
                story_pos,
                page,
                page_label: crate::longdoc::sections::section_page_label(doc, page),
            });
        }
    }
    entries.sort_by(|a, b| a.page.cmp(&b.page).then(a.story_pos.cmp(&b.story_pos)));
    entries
}

pub fn toc_text(entries: &[TocEntry], spec: &TocSpec) -> (String, Vec<Value>) {
    let mut text = String::new();
    let mut paras = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        let start = text.chars().count();
        text.push_str(&entry.text);
        if spec.page_numbers {
            text.push_str(&spec.leader);
            text.push_str(&entry.page_label);
        }
        let end = text.chars().count();
        paras.push(json!({ "start": start, "end": end, "toc_level": entry.level }));
    }
    (text, paras)
}

pub(crate) fn write_story(doc: &mut Document, story: StoryId, text: String, paras: Vec<Value>) -> Result<(), CommandError> {
    let start = doc.story_pos(story, 0);
    let target = doc.stories.iter_mut().find(|s| s.id == story).ok_or_else(CommandError::not_found)?;
    let old_len = target.text.chars().count();
    let new_len = text.chars().count();
    target.text = text;
    target.paras = paras;
    target.ranges = Vec::new();
    target.specials = Vec::new();
    crate::text_cmd::set_size_range(target, crate::text_cmd::DEFAULT_SIZE_PT);
    super::marks::shift_after_story_rebuild(doc, start, old_len, new_len);
    super::xref::refresh_xrefs(doc);
    Ok(())
}

pub fn generated_frame_rect(session: &Session, params: &Value) -> ([f32; 2], [f32; 2]) {
    let read = |key: &str| -> Option<[f32; 2]> {
        let arr = params.get(key)?.as_array()?;
        Some([arr.first()?.as_f64()? as f32, arr.get(1)?.as_f64()? as f32])
    };
    let caret_frame = session
        .text_edit
        .as_ref()
        .and_then(|edit| edit.object.and_then(|id| session.document.get_object(id)).or_else(|| session.document.objects.iter().find(|o| o.story_id() == Some(edit.story))));
    let page = crate::data_merge::layout::page_size(&session.document);
    let pos = read("pos").or_else(|| caret_frame.map(|o| o.pos)).unwrap_or([36.0, 36.0]);
    let size = read("size").or_else(|| caret_frame.map(|o| o.size)).unwrap_or([(page[0] - 72.0).max(72.0), (page[1] - 72.0).max(72.0)]);
    (pos, size)
}

pub fn create_generated_story(doc: &mut Document, pos: [f32; 2], size: [f32; 2], name: &str) -> StoryId {
    let story_id = doc.alloc_story_id();
    doc.stories.push(Story { id: story_id, text: String::new(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
    let object = Object {
        pos,
        size,
        kind: ObjectKind::FrameText { story: story_id },
        frame: Some(crate::FrameProps::from_rect(size)),
        name: name.to_string(),
        ..Object::default()
    };
    let _ = doc.create_object(object);
    story_id
}

fn rebuild(doc: &mut Document, index: usize) -> Result<usize, CommandError> {
    let record = doc.toc.tocs.get(index).cloned().ok_or_else(CommandError::not_found)?;
    let entries = build_toc(doc, &record.spec);
    let (text, paras) = toc_text(&entries, &record.spec);
    write_story(doc, StoryId(record.story), text, paras)?;
    Ok(entries.len())
}

fn target_index(session: &Session, command: &Command) -> Option<usize> {
    let story = command.params.get("story").and_then(Value::as_u64).or_else(|| session.text_edit.as_ref().map(|e| e.story.0));
    match story {
        Some(id) => session.document.toc.tocs.iter().position(|t| t.story == id).or(if session.document.toc.tocs.len() == 1 { Some(0) } else { None }),
        None => (!session.document.toc.tocs.is_empty()).then_some(0),
    }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let has = session.has_document;
    match command.name.as_str() {
        "toc.insert" => Some(has && !session.document.toc.spec.styles.is_empty()),
        "toc.update" => Some(has && target_index(session, command).is_some()),
        "toc.update_all" => Some(has && !session.document.toc.tocs.is_empty()),
        "toc.spec.set" | "toc.list" => Some(has),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let result = match command.name.as_str() {
        "toc.spec.set" => (|| {
            let spec: TocSpec = serde_json::from_value(command.params.get("spec").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())?;
            if spec.styles.iter().any(|(_, level)| *level == 0 || *level > MAX_LEVEL) {
                return Err(CommandError::invalid_params());
            }
            let before = crate::longdoc::doc_snapshot(&session.document);
            session.document.toc.spec = spec;
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "toc.insert" => (|| {
            let spec = session.document.toc.spec.clone();
            if spec.styles.is_empty() {
                return Err(CommandError::invalid_params());
            }
            let (pos, size) = generated_frame_rect(session, &command.params);
            let before = crate::longdoc::doc_snapshot(&session.document);
            let story = create_generated_story(&mut session.document, pos, size, "toc");
            session.document.toc.tocs.push(TocRecord { story: story.0, spec });
            let last = session.document.toc.tocs.len() - 1;
            let count = rebuild(&mut session.document, last)?;
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "story": story.0, "entries": count, "doc_version": session.generation }))
        })(),
        "toc.update" => (|| {
            let index = target_index(session, command).ok_or_else(CommandError::not_found)?;
            let before = crate::longdoc::doc_snapshot(&session.document);
            let count = rebuild(&mut session.document, index)?;
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "entries": count, "doc_version": session.generation }))
        })(),
        "toc.update_all" => (|| {
            let before = crate::longdoc::doc_snapshot(&session.document);
            let mut total = 0;
            for index in 0..session.document.toc.tocs.len() {
                total += rebuild(&mut session.document, index)?;
            }
            crate::longdoc::record_doc(session, command, before);
            let _ = crate::longdoc::fields::run(session, &Command::new("fields.update_all"));
            Ok(json!({ "entries": total, "doc_version": session.generation }))
        })(),
        "toc.list" => {
            let entries = build_toc(&session.document, &session.document.toc.spec);
            Ok(json!({ "entries": entries, "tocs": session.document.toc.tocs, "styles": para_style_names(&session.document) }))
        }
        _ => return None,
    };
    Some(result)
}
