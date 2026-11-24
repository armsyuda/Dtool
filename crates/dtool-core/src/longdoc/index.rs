use crate::command::Session;
use crate::document::Document;
use crate::table::model::StoryId;
use crate::CommandError;
use dtool_protocol::{Command, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const TOPIC_SEP: char = '/';

const CHOSEONG: [char; 19] = [
    '\u{3131}', '\u{3132}', '\u{3134}', '\u{3137}', '\u{3138}', '\u{3139}', '\u{3141}', '\u{3142}', '\u{3143}', '\u{3145}',
    '\u{3146}', '\u{3147}', '\u{3148}', '\u{3149}', '\u{314a}', '\u{314b}', '\u{314c}', '\u{314d}', '\u{314e}',
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexTopic {
    pub name: String,
    #[serde(default)]
    pub sub: Vec<IndexTopic>,
    #[serde(default)]
    pub see: Vec<String>,
    #[serde(default)]
    pub see_also: Vec<String>,
}

impl IndexTopic {
    pub(crate) fn merge_references(&mut self, see: &[String], see_also: &[String]) {
        extend_unique(&mut self.see, see);
        extend_unique(&mut self.see_also, see_also);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexMark {
    pub topic: String,
    pub story_pos: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexData {
    #[serde(default)]
    pub topics: Vec<IndexTopic>,
    #[serde(default)]
    pub marks: Vec<IndexMark>,
    #[serde(default)]
    pub indexes: Vec<u64>,
    #[serde(default)]
    pub show_marks: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IndexLine {
    pub level: u8,
    pub header: bool,
    pub text: String,
}

pub fn is_index_story(doc: &Document, story: StoryId) -> bool {
    doc.index.indexes.contains(&story.0)
}

pub fn split_path(path: &str) -> Vec<String> {
    path.split(TOPIC_SEP).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

pub fn normalize_path(path: &str) -> String {
    split_path(path).join(&TOPIC_SEP.to_string())
}

pub fn find_topic<'a>(topics: &'a [IndexTopic], path: &str) -> Option<&'a IndexTopic> {
    let mut list = topics;
    let mut found = None;
    for part in split_path(path) {
        let topic = list.iter().find(|t| t.name == part)?;
        found = Some(topic);
        list = &topic.sub;
    }
    found
}

pub fn ensure_topic<'a>(topics: &'a mut Vec<IndexTopic>, path: &str) -> Option<&'a mut IndexTopic> {
    let parts = split_path(path);
    let (first, rest) = parts.split_first()?;
    let index = match topics.iter().position(|t| &t.name == first) {
        Some(i) => i,
        None => {
            topics.push(IndexTopic { name: first.clone(), ..IndexTopic::default() });
            topics.len() - 1
        }
    };
    let topic = &mut topics[index];
    if rest.is_empty() {
        Some(topic)
    } else {
        ensure_topic(&mut topic.sub, &rest.join(&TOPIC_SEP.to_string()))
    }
}

pub fn remove_topic(topics: &mut Vec<IndexTopic>, path: &str) -> bool {
    let parts = split_path(path);
    let Some((last, parents)) = parts.split_last() else { return false };
    let mut list = topics;
    for part in parents {
        let Some(topic) = list.iter_mut().find(|t| &t.name == part) else { return false };
        list = &mut topic.sub;
    }
    let before = list.len();
    list.retain(|t| &t.name != last);
    list.len() != before
}

pub fn topic_paths(topics: &[IndexTopic]) -> Vec<String> {
    fn walk(topics: &[IndexTopic], prefix: &str, out: &mut Vec<String>) {
        for topic in topics {
            let path = if prefix.is_empty() { topic.name.clone() } else { format!("{}{}{}", prefix, TOPIC_SEP, topic.name) };
            out.push(path.clone());
            walk(&topic.sub, &path, out);
        }
    }
    let mut out = Vec::new();
    walk(topics, "", &mut out);
    out
}

fn base_jamo(c: char) -> char {
    match c {
        '\u{3132}' => '\u{3131}',
        '\u{3138}' => '\u{3137}',
        '\u{3143}' => '\u{3142}',
        '\u{3146}' => '\u{3145}',
        '\u{3149}' => '\u{3148}',
        _ => c,
    }
}

pub fn group_of(name: &str) -> (u8, String) {
    let Some(c) = name.trim().chars().next() else { return (3, String::new()) };
    let code = c as u32;
    if (0xAC00..=0xD7A3).contains(&code) {
        return (0, base_jamo(CHOSEONG[((code - 0xAC00) / 588) as usize]).to_string());
    }
    if (0x3131..=0x314E).contains(&code) {
        return (0, base_jamo(c).to_string());
    }
    if c.is_ascii_alphabetic() {
        return (1, c.to_ascii_uppercase().to_string());
    }
    (2, c.to_string())
}

fn sort_key(name: &str) -> (u8, String, String) {
    let (order, group) = group_of(name);
    (order, group, name.trim().to_lowercase())
}

fn sorted(topics: &[IndexTopic]) -> Vec<&IndexTopic> {
    let mut list: Vec<&IndexTopic> = topics.iter().collect();
    list.sort_by(|a, b| sort_key(&a.name).cmp(&sort_key(&b.name)));
    list
}

pub fn page_ranges(doc: &Document, pages: &[usize]) -> String {
    let mut pages = pages.to_vec();
    pages.sort_unstable();
    pages.dedup();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < pages.len() {
        let start = pages[i];
        let mut end = start;
        while i + 1 < pages.len() && pages[i + 1] == end + 1 {
            i += 1;
            end = pages[i];
        }
        let first = crate::longdoc::sections::section_page_label(doc, start);
        if end > start {
            out.push(format!("{}\u{2013}{}", first, crate::longdoc::sections::section_page_label(doc, end)));
        } else {
            out.push(first);
        }
        i += 1;
    }
    out.join(", ")
}

fn merged_topics(doc: &Document) -> Vec<IndexTopic> {
    let mut topics = doc.index.topics.clone();
    for mark in &doc.index.marks {
        let _ = ensure_topic(&mut topics, &mark.topic);
    }
    topics
}

fn mark_pages(doc: &Document) -> BTreeMap<String, Vec<usize>> {
    let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for mark in &doc.index.marks {
        map.entry(normalize_path(&mark.topic)).or_default().push(doc.page_index_for_story_pos(mark.story_pos));
    }
    map
}

fn topic_line(doc: &Document, topic: &IndexTopic, pages: Option<&Vec<usize>>) -> String {
    let mut parts = Vec::new();
    let ranges = pages.map(|p| page_ranges(doc, p)).unwrap_or_default();
    parts.push(if ranges.is_empty() { topic.name.clone() } else { format!("{} {}", topic.name, ranges) });
    if !topic.see.is_empty() {
        parts.push(format!("{} {}", dtool_strings::text("index.see"), topic.see.join(", ")));
    }
    if !topic.see_also.is_empty() {
        parts.push(format!("{} {}", dtool_strings::text("index.see_also"), topic.see_also.join(", ")));
    }
    parts.join(". ")
}

fn push_topic(doc: &Document, topic: &IndexTopic, path: &str, level: u8, pages: &BTreeMap<String, Vec<usize>>, out: &mut Vec<IndexLine>) {
    out.push(IndexLine { level, header: false, text: topic_line(doc, topic, pages.get(path)) });
    for sub in sorted(&topic.sub) {
        let sub_path = format!("{}{}{}", path, TOPIC_SEP, sub.name);
        push_topic(doc, sub, &sub_path, level.saturating_add(1), pages, out);
    }
}

pub fn build_index(doc: &Document) -> Vec<IndexLine> {
    let topics = merged_topics(doc);
    let pages = mark_pages(doc);
    let mut out = Vec::new();
    let mut group: Option<(u8, String)> = None;
    for topic in sorted(&topics) {
        let g = group_of(&topic.name);
        if group.as_ref() != Some(&g) {
            out.push(IndexLine { level: 0, header: true, text: g.1.clone() });
            group = Some(g);
        }
        push_topic(doc, topic, &topic.name, 1, &pages, &mut out);
    }
    out
}

pub fn index_text(lines: &[IndexLine]) -> (String, Vec<Value>) {
    let mut text = String::new();
    let mut paras = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        let start = text.chars().count();
        text.push_str(&"  ".repeat(line.level.saturating_sub(1) as usize));
        text.push_str(&line.text);
        let end = text.chars().count();
        paras.push(json!({ "start": start, "end": end, "index_level": line.level, "index_header": line.header }));
    }
    (text, paras)
}

fn rebuild_all(doc: &mut Document) -> Result<usize, CommandError> {
    let lines = build_index(doc);
    let (text, paras) = index_text(&lines);
    for story in doc.index.indexes.clone() {
        crate::longdoc::toc::write_story(doc, StoryId(story), text.clone(), paras.clone())?;
    }
    Ok(lines.len())
}

fn caret_pos(session: &Session) -> Option<usize> {
    session.text_edit.as_ref().map(|edit| session.document.story_pos(edit.story, edit.caret))
}

fn str_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        _ => Vec::new(),
    }
}

fn extend_unique(list: &mut Vec<String>, items: &[String]) {
    for item in items {
        if !list.contains(item) {
            list.push(item.clone());
        }
    }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let has = session.has_document;
    let index = &session.document.index;
    match command.name.as_str() {
        "index.mark.insert" => Some(has && (command.params.get("story_pos").is_some() || session.text_edit.is_some())),
        "index.mark.delete" => Some(has && !index.marks.is_empty()),
        "index.topic.add" | "index.marks.toggle" | "index.list" => Some(has),
        "index.topic.remove" => Some(has && !index.topics.is_empty()),
        "index.insert" => Some(has && (!index.topics.is_empty() || !index.marks.is_empty())),
        "index.update" => Some(has && !index.indexes.is_empty()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let result = match command.name.as_str() {
        "index.mark.insert" => (|| {
            let topic = command.params.get("topic").and_then(Value::as_str).map(normalize_path).filter(|t| !t.is_empty()).ok_or_else(CommandError::invalid_params)?;
            let story_pos = command.params.get("story_pos").and_then(Value::as_u64).map(|v| v as usize).or_else(|| caret_pos(session)).ok_or_else(CommandError::invalid_params)?;
            let before = crate::longdoc::doc_snapshot(&session.document);
            let _ = ensure_topic(&mut session.document.index.topics, &topic);
            session.document.index.marks.push(IndexMark { topic: topic.clone(), story_pos });
            let index = session.document.index.marks.len() - 1;
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "index": index, "topic": topic, "story_pos": story_pos, "doc_version": session.generation }))
        })(),
        "index.mark.delete" => (|| {
            let index = command.params.get("index").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)? as usize;
            if index >= session.document.index.marks.len() {
                return Err(CommandError::out_of_range());
            }
            let before = crate::longdoc::doc_snapshot(&session.document);
            session.document.index.marks.remove(index);
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "index.topic.add" => (|| {
            let name = command.params.get("name").and_then(Value::as_str).map(normalize_path).filter(|t| !t.is_empty()).ok_or_else(CommandError::invalid_params)?;
            let parent = command.params.get("parent").and_then(Value::as_str).map(normalize_path).unwrap_or_default();
            let path = if parent.is_empty() { name } else { format!("{}{}{}", parent, TOPIC_SEP, name) };
            let see = str_list(command.params.get("see"));
            let see_also = str_list(command.params.get("see_also"));
            let before = crate::longdoc::doc_snapshot(&session.document);
            let topic = ensure_topic(&mut session.document.index.topics, &path).ok_or_else(CommandError::invalid_params)?;
            topic.merge_references(&see, &see_also);
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "path": path, "doc_version": session.generation }))
        })(),
        "index.topic.remove" => (|| {
            let path = command.params.get("path").and_then(Value::as_str).ok_or_else(CommandError::invalid_params)?.to_string();
            let before = crate::longdoc::doc_snapshot(&session.document);
            if !remove_topic(&mut session.document.index.topics, &path) {
                return Err(CommandError::not_found());
            }
            let prefix = format!("{}{}", normalize_path(&path), TOPIC_SEP);
            let norm = normalize_path(&path);
            session.document.index.marks.retain(|m| {
                let t = normalize_path(&m.topic);
                t != norm && !t.starts_with(&prefix)
            });
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "index.insert" => (|| {
            let (pos, size) = crate::longdoc::toc::generated_frame_rect(session, &command.params);
            let before = crate::longdoc::doc_snapshot(&session.document);
            let story = crate::longdoc::toc::create_generated_story(&mut session.document, pos, size, "index");
            session.document.index.indexes.push(story.0);
            let count = rebuild_all(&mut session.document)?;
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "story": story.0, "lines": count, "doc_version": session.generation }))
        })(),
        "index.update" => (|| {
            let before = crate::longdoc::doc_snapshot(&session.document);
            let count = rebuild_all(&mut session.document)?;
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "lines": count, "doc_version": session.generation }))
        })(),
        "index.marks.toggle" => (|| {
            let before = crate::longdoc::doc_snapshot(&session.document);
            session.document.index.show_marks = !session.document.index.show_marks;
            crate::longdoc::record_doc(session, command, before);
            Ok(json!({ "show_marks": session.document.index.show_marks }))
        })(),
        "index.list" => {
            let doc = &session.document;
            let marks: Vec<Value> = doc
                .index
                .marks
                .iter()
                .map(|m| json!({ "topic": m.topic, "story_pos": m.story_pos, "page": doc.page_index_for_story_pos(m.story_pos) }))
                .collect();
            Ok(json!({ "lines": build_index(doc), "topics": doc.index.topics, "marks": marks, "indexes": doc.index.indexes, "show_marks": doc.index.show_marks }))
        }
        _ => return None,
    };
    Some(result)
}
