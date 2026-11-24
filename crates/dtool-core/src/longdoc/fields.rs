use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::marks::{self, StoryMarks};
use crate::command::Session;
use crate::data_merge::layout::object_page_index;
use crate::document::{Document, Story};
use crate::object::ObjectId;
use crate::table::model::StoryId;
use crate::text_cmd::push_change;
use dtool_protocol::{Command, CommandError, CommandResult};

pub use super::sections::{page_number, section_page_label};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    PageNumber,
    NextFramePage,
    PrevFramePage,
    TotalPages,
    LastPageInSection,
    SectionName,
    ArtboardName,
    RunningHeader,
    DateTime,
    Created,
    Saved,
    Printed,
    Filename,
    Path,
    Meta(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldRun {
    pub kind: FieldKind,
    pub format: String,
}

pub fn resolve_field(doc: &Document, page_index: usize, field: &FieldRun) -> String {
    let page_index = page_index.min(doc.pages.len().saturating_sub(1));
    match &field.kind {
        FieldKind::PageNumber => format_page(doc, page_index, &field.format),
        FieldKind::TotalPages => doc.pages.len().to_string(),
        FieldKind::LastPageInSection => {
            format_page(doc, last_page_in_section(doc, page_index), &field.format)
        }
        FieldKind::NextFramePage => neighbor_frame_page(doc, page_index, 1)
            .map(|index| format_page(doc, index, &field.format))
            .unwrap_or_default(),
        FieldKind::PrevFramePage => neighbor_frame_page(doc, page_index, -1)
            .map(|index| format_page(doc, index, &field.format))
            .unwrap_or_default(),
        FieldKind::SectionName => section_at(doc, page_index)
            .map(|section| section.name.clone())
            .unwrap_or_default(),
        FieldKind::ArtboardName => String::new(),
        FieldKind::RunningHeader => running_header(doc, page_index, &field.format),
        FieldKind::DateTime => format_time(now_unix(), doc.field_info.offset(), &field.format),
        FieldKind::Created => stamp_text(doc, doc.field_info.created, &field.format),
        FieldKind::Saved => stamp_text(doc, doc.field_info.saved, &field.format),
        FieldKind::Printed => stamp_text(doc, doc.field_info.printed, &field.format),
        FieldKind::Filename => doc
            .field_info
            .file_path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        FieldKind::Path => doc.field_info.file_path.clone().unwrap_or_default(),
        FieldKind::Meta(name) => meta_value(&doc.metadata, name),
    }
}

fn format_page(doc: &Document, page_index: usize, _format: &str) -> String {
    page_number(doc, page_index).1
}

fn last_page_in_section(doc: &Document, page_index: usize) -> usize {
    let physical = page_index.saturating_add(1);
    let total = doc.pages.len();
    let mut end = total;
    for section in doc.sections() {
        if section.start_page > physical {
            end = section.start_page.saturating_sub(1);
            break;
        }
    }
    if end == 0 {
        0
    } else {
        end.min(total).saturating_sub(1)
    }
}

fn section_at(doc: &Document, page_index: usize) -> Option<&super::sections::Section> {
    let physical = page_index.saturating_add(1);
    let mut active = None;
    for section in doc.sections() {
        if section.start_page <= physical {
            active = Some(section);
        } else {
            break;
        }
    }
    active
}

fn frames_for_story(doc: &Document, story: StoryId) -> Vec<(ObjectId, usize, [f32; 2])> {
    let mut frames: Vec<(ObjectId, usize, [f32; 2])> = doc
        .objects
        .iter()
        .filter(|object| object.story_id() == Some(story))
        .map(|object| (object.id, object_page_index(doc, object.id), object.pos))
        .collect();
    frames.sort_by(|a, b| a.1.cmp(&b.1).then(a.2[1].total_cmp(&b.2[1])).then(a.2[0].total_cmp(&b.2[0])).then(a.0.0.cmp(&b.0.0)));
    frames
}

fn neighbor_frame_page(doc: &Document, page_index: usize, step: i32) -> Option<usize> {
    let on_page: Vec<_> = doc
        .objects
        .iter()
        .filter(|object| object.story_id().is_some() && object_page_index(doc, object.id) == page_index)
        .collect();
    for object in on_page {
        let story = object.story_id()?;
        let frames = frames_for_story(doc, story);
        let current = frames.iter().position(|(id, _, _)| *id == object.id)?;
        let next = current as i32 + step;
        if next >= 0 {
            if let Some((_, index, _)) = frames.get(next as usize) {
                if *index != page_index {
                    return Some(*index);
                }
            }
        }
    }
    None
}

fn object_for_story(doc: &Document, story: StoryId, preferred: Option<ObjectId>) -> Option<ObjectId> {
    if let Some(id) = preferred {
        if doc.get_object(id).and_then(|object| object.story_id()) == Some(story) {
            return Some(id);
        }
    }
    frames_for_story(doc, story).first().map(|(id, _, _)| *id)
}

fn page_for_story(doc: &Document, story: StoryId, preferred: Option<ObjectId>) -> usize {
    object_for_story(doc, story, preferred)
        .map(|id| object_page_index(doc, id))
        .unwrap_or(0)
}

fn parse_kind(params: &Value) -> Result<FieldKind, CommandError> {
    let value = params.get("kind").ok_or_else(CommandError::invalid_params)?;
    if let Ok(kind) = serde_json::from_value::<FieldKind>(value.clone()) {
        return Ok(kind);
    }
    let name = value.as_str().ok_or_else(CommandError::invalid_params)?;
    kind_from_name(name).ok_or_else(CommandError::invalid_params)
}

fn kind_from_name(name: &str) -> Option<FieldKind> {
    if let Some(meta) = meta_name(name) {
        return Some(FieldKind::Meta(meta));
    }
    let key = name.rsplit('.').next().unwrap_or(name);
    match key {
        "PageNumber" | "page_number" => Some(FieldKind::PageNumber),
        "NextFramePage" | "next_frame_page" | "next_frame_page_number" => Some(FieldKind::NextFramePage),
        "PrevFramePage" | "prev_frame_page" | "previous_frame_page_number" => Some(FieldKind::PrevFramePage),
        "TotalPages" | "total_pages" => Some(FieldKind::TotalPages),
        "LastPageInSection" | "last_page_in_section" | "last_page_in_section_run" => {
            Some(FieldKind::LastPageInSection)
        }
        "SectionName" | "section_name" => Some(FieldKind::SectionName),
        "ArtboardName" | "artboard_name" => Some(FieldKind::ArtboardName),
        "RunningHeader" | "running_header" => Some(FieldKind::RunningHeader),
        "DateTime" | "date_and_time" | "date_time" => Some(FieldKind::DateTime),
        "Created" | "created_date" | "created" => Some(FieldKind::Created),
        "Saved" | "saved_date" | "saved" => Some(FieldKind::Saved),
        "Printed" | "printed_or_exported_date" | "printed" => Some(FieldKind::Printed),
        "Filename" | "filename" => Some(FieldKind::Filename),
        "Path" | "path" => Some(FieldKind::Path),
        _ => None,
    }
}

fn param_format(params: &Value) -> String {
    params
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn scalar_len(text: &str) -> usize {
    text.chars().count()
}

fn byte_at(text: &str, at: usize) -> Result<usize, CommandError> {
    if at == scalar_len(text) {
        return Ok(text.len());
    }
    text.char_indices()
        .nth(at)
        .map(|(index, _)| index)
        .ok_or_else(CommandError::invalid_params)
}

fn insert_scalars(text: &mut String, at: usize, chunk: &str) -> Result<(), CommandError> {
    let index = byte_at(text, at)?;
    text.insert_str(index, chunk);
    Ok(())
}

fn replace_scalars(text: &mut String, start: usize, end: usize, chunk: &str) -> Result<(), CommandError> {
    if end < start {
        return Err(CommandError::invalid_params());
    }
    let lo = byte_at(text, start)?;
    let hi = byte_at(text, end)?;
    text.replace_range(lo..hi, chunk);
    Ok(())
}

fn shift_bound(value: &mut Value, key: &str, at: usize, delta: isize) {
    if let Some(map) = value.as_object_mut() {
        if let Some(pos) = map.get(key).and_then(Value::as_u64) {
            let pos = pos as usize;
            if pos >= at {
                let moved = (pos as isize + delta).max(0) as usize;
                map.insert(key.to_string(), json!(moved));
            }
        }
    }
}

fn shift_spans(values: &mut [Value], at: usize, delta: isize) {
    for value in values {
        shift_bound(value, "start", at, delta);
        shift_bound(value, "end", at, delta);
    }
}

fn read_field_run(value: &Value) -> Option<(usize, usize, FieldRun)> {
    let map = value.as_object()?;
    let run: FieldRun = serde_json::from_value(map.get("field_run")?.clone()).ok()?;
    let start = map.get("start")?.as_u64()? as usize;
    let end = map.get("end")?.as_u64()? as usize;
    if end < start {
        return None;
    }
    Some((start, end, run))
}

fn field_at(story: &Story, at: usize) -> Option<(usize, usize, usize, FieldRun)> {
    story
        .ranges
        .iter()
        .enumerate()
        .filter_map(|(index, value)| read_field_run(value).map(|(start, end, run)| (index, start, end, run)))
        .find(|(_, start, end, _)| at >= *start && at <= *end)
}

fn snapshot(story: &Story) -> Value {
    json!({
        "text": story.text,
        "ranges": story.ranges,
        "paras": story.paras,
        "specials": story.specials
    })
}

fn restore_story(story: &mut Story, snap: &Value) -> Result<(), CommandError> {
    story.text = snap
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(CommandError::invalid_params)?
        .to_string();
    if let Some(ranges) = snap.get("ranges") {
        story.ranges = serde_json::from_value(ranges.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(paras) = snap.get("paras") {
        story.paras = serde_json::from_value(paras.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(specials) = snap.get("specials") {
        story.specials = serde_json::from_value(specials.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    Ok(())
}

fn find_story_mut(doc: &mut Document, id: StoryId) -> Result<&mut Story, CommandError> {
    doc.stories
        .iter_mut()
        .find(|story| story.id == id)
        .ok_or_else(CommandError::invalid_params)
}

fn find_story(doc: &Document, id: StoryId) -> Result<&Story, CommandError> {
    doc.stories
        .iter()
        .find(|story| story.id == id)
        .ok_or_else(CommandError::invalid_params)
}

fn edit_target(session: &Session) -> Result<(StoryId, usize, Option<ObjectId>), CommandError> {
    let edit = session.text_edit.as_ref().ok_or_else(CommandError::invalid_params)?;
    Ok((edit.story, edit.caret, edit.object))
}

fn set_caret(session: &mut Session, at: usize) {
    if let Some(edit) = session.text_edit.as_mut() {
        edit.caret = at;
        edit.anchor = at;
    }
}

fn field_restore(session: &Session, name: &str, story: StoryId, caret: usize) -> Result<Command, CommandError> {
    Ok(Command::with_params(name, json!({
        "story": story.0,
        "snapshot": snapshot(find_story(&session.document, story)?),
        "caret": caret,
        "marks": StoryMarks::capture(&session.document)
    })))
}

fn run_insert(session: &mut Session, command: &Command) -> CommandResult {
    let kind = parse_kind(&command.params)?;
    let format = param_format(&command.params);
    let field = FieldRun { kind, format };
    let (story_id, at, object) = edit_target(session)?;
    let page_index = page_for_story(&session.document, story_id, object);
    let text = resolve_field(&session.document, page_index, &field);
    let count = scalar_len(&text);
    let undo = field_restore(session, "text.field.insert.undo", story_id, at)?;
    let position = session.document.story_pos(story_id, at);
    {
        let story = find_story_mut(&mut session.document, story_id)?;
        insert_scalars(&mut story.text, at, &text)?;
        let touching: Vec<usize> = story
            .ranges
            .iter()
            .enumerate()
            .filter_map(|(index, run)| read_field_run(run).filter(|(start, end, _)| *start < at && *end == at).map(|_| index))
            .collect();
        shift_spans(&mut story.paras, at, count as isize);
        shift_spans(&mut story.ranges, at, count as isize);
        for index in touching {
            if let Some(map) = story.ranges[index].as_object_mut() {
                map.insert("end".to_string(), json!(at));
            }
        }
        story.ranges.push(json!({
            "start": at,
            "end": at + count,
            "field_run": field
        }));
    }
    marks::shift_after_text_edit(&mut session.document, position, 0, count);
    super::xref::refresh_xrefs(&mut session.document);
    let redo = field_restore(session, "text.field.insert.undo", story_id, at + count)?;
    push_change(session, "text.field.insert", redo, undo);
    set_caret(session, at + count);
    Ok(json!({
        "doc_version": session.generation,
        "story": story_id.0,
        "at": at,
        "length": count,
        "text": text
    }))
}

fn run_update_one(session: &mut Session, story_id: StoryId, index: usize, object: Option<ObjectId>) -> Result<usize, CommandError> {
    let page_index = page_for_story(&session.document, story_id, object);
    let story = find_story(&session.document, story_id)?;
    let (start, end, field) = read_field_run(&story.ranges[index]).ok_or_else(CommandError::invalid_params)?;
    let text = resolve_field(&session.document, page_index, &field);
    let old_len = end.saturating_sub(start);
    let new_len = scalar_len(&text);
    let delta = new_len as isize - old_len as isize;
    let position = session.document.story_pos(story_id, start);
    {
        let story = find_story_mut(&mut session.document, story_id)?;
        replace_scalars(&mut story.text, start, end, &text)?;
        if delta != 0 {
            shift_spans(&mut story.paras, end, delta);
            for (pos, value) in story.ranges.iter_mut().enumerate() {
                if pos == index {
                    if let Some(map) = value.as_object_mut() {
                        map.insert("end".to_string(), json!(start + new_len));
                    }
                } else {
                    shift_bound(value, "start", end, delta);
                    shift_bound(value, "end", end, delta);
                }
            }
        }
    }
    marks::shift_after_text_edit(&mut session.document, position, old_len, new_len);
    if delta != 0 && let Some(edit) = session.text_edit.as_mut().filter(|edit| edit.story == story_id) {
        let relocate = |at: usize| {
            if at < start {
                at
            } else if at < end {
                start + (at - start).min(new_len)
            } else {
                at - old_len + new_len
            }
        };
        edit.caret = relocate(edit.caret);
        edit.anchor = relocate(edit.anchor);
    }
    Ok(start + new_len)
}

fn run_update(session: &mut Session) -> CommandResult {
    let (story_id, at, object) = edit_target(session)?;
    let story = find_story(&session.document, story_id)?;
    let (index, _, _, _) = field_at(story, at).ok_or_else(CommandError::invalid_params)?;
    let undo = field_restore(session, "text.field.update.undo", story_id, at)?;
    let end = run_update_one(session, story_id, index, object)?;
    super::xref::refresh_xrefs(&mut session.document);
    let redo = field_restore(session, "text.field.update.undo", story_id, end)?;
    push_change(session, "text.field.update", redo, undo);
    set_caret(session, end);
    Ok(json!({"doc_version": session.generation, "story": story_id.0}))
}

fn run_update_all(session: &mut Session, command: &Command) -> CommandResult {
    let mut snaps = Vec::new();
    let stories: Vec<(StoryId, Option<ObjectId>, Vec<usize>)> = session
        .document
        .stories
        .iter()
        .map(|story| {
            let indexes: Vec<usize> = story
                .ranges
                .iter()
                .enumerate()
                .filter_map(|(index, value)| read_field_run(value).map(|_| index))
                .collect();
            (story.id, None, indexes)
        })
        .filter(|(_, _, indexes)| !indexes.is_empty())
        .collect();
    let caret = session.text_edit.as_ref().map(|edit| (edit.story.0, edit.caret));
    let marks = StoryMarks::capture(&session.document);
    for (story_id, _, indexes) in &stories {
        let story = find_story(&session.document, *story_id)?;
        let len = scalar_len(&story.text);
        if indexes.iter().any(|&index| read_field_run(&story.ranges[index]).is_some_and(|(_, end, _)| end > len)) {
            return Err(CommandError::invalid_params());
        }
    }
    for (story_id, object, indexes) in stories {
        let before = snapshot(find_story(&session.document, story_id)?);
        for index in indexes.into_iter().rev() {
            let _ = run_update_one(session, story_id, index, object)?;
        }
        snaps.push(json!({"story": story_id.0, "snapshot": before}));
    }
    let mut undo = Command::new("text.field.update_all.undo");
    undo.params = json!({"stories": snaps, "caret": caret, "marks": marks});
    super::xref::refresh_xrefs(&mut session.document);
    push_change(session, "fields.update_all", command.clone(), undo);
    Ok(json!({"doc_version": session.generation}))
}

fn run_remove(session: &mut Session) -> CommandResult {
    let (story_id, at, _) = edit_target(session)?;
    let story = find_story(&session.document, story_id)?;
    let (index, start, end, _) = field_at(story, at).ok_or_else(CommandError::invalid_params)?;
    let count = end.saturating_sub(start);
    let undo = field_restore(session, "text.field.remove.undo", story_id, at)?;
    let position = session.document.story_pos(story_id, start);
    {
        let story = find_story_mut(&mut session.document, story_id)?;
        replace_scalars(&mut story.text, start, end, "")?;
        story.ranges.remove(index);
        shift_spans(&mut story.paras, end, -(count as isize));
        shift_spans(&mut story.ranges, end, -(count as isize));
    }
    marks::shift_after_text_edit(&mut session.document, position, count, 0);
    super::xref::refresh_xrefs(&mut session.document);
    let redo = field_restore(session, "text.field.remove.undo", story_id, start)?;
    push_change(session, "text.field.remove", redo, undo);
    set_caret(session, start);
    Ok(json!({"doc_version": session.generation, "story": story_id.0, "at": start}))
}

fn run_query(session: &Session, _command: &Command) -> CommandResult {
    if let Some(edit) = session.text_edit.as_ref() {
        let story = find_story(&session.document, edit.story)?;
        if let Some((_, start, end, field)) = field_at(story, edit.caret) {
            let page_index = page_for_story(&session.document, edit.story, edit.object);
            let text = resolve_field(&session.document, page_index, &field);
            return Ok(json!({
                "story": edit.story.0,
                "start": start,
                "end": end,
                "kind": field.kind,
                "format": field.format,
                "text": text
            }));
        }
    }
    let mut fields = Vec::new();
    for story in &session.document.stories {
        for value in &story.ranges {
            if let Some((start, end, field)) = read_field_run(value) {
                fields.push(json!({
                    "story": story.id.0,
                    "start": start,
                    "end": end,
                    "kind": field.kind,
                    "format": field.format
                }));
            }
        }
    }
    Ok(json!({"fields": fields}))
}

fn run_restore(session: &mut Session, command: &Command) -> CommandResult {
    let marks: Option<StoryMarks> = command.params.get("marks").filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone())).transpose().map_err(|_| CommandError::invalid_params())?;
    if let Some(stories) = command.params.get("stories").and_then(Value::as_array) {
        for item in stories {
            let id = item.get("story").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?;
            let snap = item.get("snapshot").ok_or_else(CommandError::invalid_params)?;
            restore_story(find_story_mut(&mut session.document, StoryId(id))?, snap)?;
        }
        if let Some(caret) = command.params.get("caret") {
            if let (Some(story), Some(at)) = (caret.get(0).and_then(Value::as_u64), caret.get(1).and_then(Value::as_u64)) {
                if let Some(edit) = session.text_edit.as_mut() {
                    if edit.story == StoryId(story) {
                        set_caret(session, at as usize);
                    }
                }
            }
        }
        if let Some(marks) = marks {
            marks.restore(&mut session.document);
        }
        return Ok(json!({"doc_version": session.generation}));
    }
    let id = command
        .params
        .get("story")
        .and_then(Value::as_u64)
        .ok_or_else(CommandError::invalid_params)?;
    let snap = command.params.get("snapshot").ok_or_else(CommandError::invalid_params)?;
    restore_story(find_story_mut(&mut session.document, StoryId(id))?, snap)?;
    if let Some(marks) = marks {
        marks.restore(&mut session.document);
    }
    if let Some(at) = command.params.get("caret").and_then(Value::as_u64)
        && session.text_edit.as_ref().is_some_and(|edit| edit.story == StoryId(id))
    {
        set_caret(session, at as usize);
    }
    Ok(json!({"doc_version": session.generation}))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if menu_kind(&command.name).is_some() {
        return Some(session.has_document && session.text_edit.is_some());
    }
    match command.name.as_str() {
        "text.field.insert" | "text.field.update" | "text.field.remove" => {
            Some(session.has_document && session.text_edit.is_some())
        }
        "text.field.query" | "fields.update_all" => Some(session.has_document),
        DOC_METADATA_SET | DOC_METADATA_QUERY => Some(session.has_document),
        "text.field.insert.undo" | "text.field.update.undo" | "text.field.remove.undo" | "text.field.update_all.undo" => {
            Some(session.has_document)
        }
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if let Some(kind) = menu_kind(&command.name) {
        return Some(run_menu_insert(session, kind));
    }
    let result = match command.name.as_str() {
        "text.field.insert" => run_insert(session, command),
        "text.field.update" => run_update(session),
        "text.field.remove" => run_remove(session),
        "text.field.query" => run_query(session, command),
        "fields.update_all" => run_update_all(session, command),
        DOC_METADATA_SET => run_metadata_set(session, command, true),
        DOC_METADATA_QUERY => Ok(json!({ "metadata": session.document.metadata })),
        "text.field.insert.undo" | "text.field.update.undo" | "text.field.remove.undo" | "text.field.update_all.undo" => {
            run_restore(session, command)
        }
        _ => return None,
    };
    Some(result)
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name == DOC_METADATA_SET {
        return Some(run_metadata_set(session, command, false).map(|_| ()));
    }
    match run(session, command) {
        Some(Ok(_)) => Some(Ok(())),
        Some(Err(err)) => Some(Err(err)),
        None => None,
    }
}

pub const MENU_FIELD_PREFIX: &str = "text.insert.fields.";

pub fn menu_kind(name: &str) -> Option<FieldKind> {
    let key = name.strip_prefix(MENU_FIELD_PREFIX)?;
    match key {
        "page_number" | "next_frame_page_number" | "previous_frame_page_number" | "total_pages" | "last_page_in_section_run" | "section_name"
        | "artboard_name" | "running_header" | "date_and_time" | "created_date" | "saved_date" | "printed_or_exported_date" | "filename"
        | "path" => kind_from_name(key),
        _ if key.starts_with("meta.") => kind_from_name(key),
        _ => None,
    }
}

fn run_menu_insert(session: &mut Session, kind: FieldKind) -> CommandResult {
    let command = Command::with_params("text.field.insert", json!({ "kind": kind }));
    run_insert(session, &command)
}

pub const DEFAULT_DATE_FORMAT: &str = "yyyy-MM-dd HH:mm";
pub const DEFAULT_UTC_OFFSET_MIN: i32 = 540;
pub const DOC_METADATA_SET: &str = "doc.metadata.set";
pub const DOC_METADATA_QUERY: &str = "doc.metadata.query";
pub const META_NAMES: [&str; 6] = ["Title", "Author", "Subject", "Keywords", "Copyright", "Description"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FieldDocInfo {
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub saved: Option<i64>,
    #[serde(default)]
    pub printed: Option<i64>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub utc_offset_min: Option<i32>,
    #[serde(skip)]
    pub resolved_sig: Option<String>,
}

impl FieldDocInfo {
    pub fn offset(&self) -> i32 {
        self.utc_offset_min.unwrap_or(DEFAULT_UTC_OFFSET_MIN)
    }
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

pub fn format_time(unix: i64, offset_min: i32, format: &str) -> String {
    let format = if format.trim().is_empty() { DEFAULT_DATE_FORMAT } else { format };
    let local = unix + offset_min as i64 * 60;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let secs = local.rem_euclid(86_400);
    let (hour, minute, second) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let table = [
        ("yyyy", format!("{year:04}")),
        ("yy", format!("{:02}", year.rem_euclid(100))),
        ("MM", format!("{month:02}")),
        ("dd", format!("{day:02}")),
        ("HH", format!("{hour:02}")),
        ("mm", format!("{minute:02}")),
        ("ss", format!("{second:02}")),
        ("M", month.to_string()),
        ("d", day.to_string()),
        ("H", hour.to_string()),
    ];
    let mut out = String::new();
    let mut rest = format;
    'scan: while let Some(ch) = rest.chars().next() {
        for (token, value) in &table {
            if rest.starts_with(token) {
                out.push_str(value);
                rest = &rest[token.len()..];
                continue 'scan;
            }
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

fn stamp_text(doc: &Document, stamp: Option<i64>, format: &str) -> String {
    stamp.map(|t| format_time(t, doc.field_info.offset(), format)).unwrap_or_default()
}

fn meta_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let tail = lower
        .strip_prefix(MENU_FIELD_PREFIX)
        .unwrap_or(&lower)
        .strip_prefix("meta.")
        .or_else(|| lower.strip_prefix("meta:"))
        .or_else(|| lower.strip_prefix("meta(").and_then(|rest| rest.strip_suffix(')')))?;
    META_NAMES
        .iter()
        .find(|known| known.eq_ignore_ascii_case(tail.trim()))
        .map(|known| known.to_string())
}

pub fn meta_value(meta: &crate::metadata::DocMetadata, name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "title" => meta.title.clone(),
        "author" => meta.author.clone(),
        "subject" => meta.subject.clone(),
        "keywords" => meta.keywords.join(", "),
        "copyright" => meta.copyright.clone(),
        "description" => meta.description.clone(),
        _ => String::new(),
    }
}

fn running_header(doc: &Document, page_index: usize, style: &str) -> String {
    let style = if style.trim().is_empty() {
        doc.toc.spec.styles.first().map(|(name, _)| name.clone()).unwrap_or_default()
    } else {
        style.to_string()
    };
    if style.is_empty() {
        return String::new();
    }
    let spec = super::toc::TocSpec { styles: vec![(style, 1)], page_numbers: false, leader: String::new() };
    let entries = super::toc::build_toc(doc, &spec);
    entries
        .iter()
        .find(|entry| entry.page == page_index)
        .or_else(|| entries.iter().rev().find(|entry| entry.page < page_index))
        .map(|entry| entry.text.clone())
        .unwrap_or_default()
}

fn text_param(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_string)
}

fn run_metadata_set(session: &mut Session, command: &Command, record: bool) -> CommandResult {
    let before = session.document.metadata.clone();
    let params = &command.params;
    let restore_marks: Option<StoryMarks> = if record {
        None
    } else {
        params.get("marks").filter(|value| !value.is_null())
            .map(|value| serde_json::from_value(value.clone())).transpose().map_err(|_| CommandError::invalid_params())?
    };
    let mut meta = match params.get("metadata") {
        Some(full) => serde_json::from_value(full.clone()).map_err(|_| CommandError::invalid_params())?,
        None => before.clone(),
    };
    if let Some(v) = text_param(params, "title") {
        meta.title = v;
    }
    if let Some(v) = text_param(params, "author") {
        meta.author = v;
    }
    if let Some(v) = text_param(params, "subject") {
        meta.subject = v;
    }
    if let Some(v) = text_param(params, "copyright") {
        meta.copyright = v;
    }
    if let Some(v) = text_param(params, "description") {
        meta.description = v;
    }
    match params.get("keywords") {
        Some(Value::String(list)) => {
            meta.keywords = list.split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_string).collect();
        }
        Some(Value::Array(items)) => {
            meta.keywords = items.iter().filter_map(Value::as_str).map(str::to_string).collect();
        }
        Some(_) => return Err(CommandError::invalid_params()),
        None => {}
    }
    let before_marks = if record { StoryMarks::capture(&session.document) } else { None };
    session.document.metadata = meta;
    refresh_fields(session);
    if let Some(marks) = restore_marks {
        marks.restore(&mut session.document);
    }
    if record {
        let undo = Command::with_params(
            DOC_METADATA_SET,
            json!({ "metadata": before, "marks": before_marks }),
        );
        let redo = Command::with_params(
            DOC_METADATA_SET,
            json!({ "metadata": session.document.metadata, "marks": StoryMarks::capture(&session.document) }),
        );
        push_change(session, DOC_METADATA_SET, redo, undo);
    }
    Ok(json!({ "doc_version": session.generation, "metadata": session.document.metadata }))
}

pub fn has_field_runs(doc: &Document) -> bool {
    doc.stories.iter().any(|story| story.ranges.iter().any(|run| read_field_run(run).is_some()))
}

pub fn field_signature(doc: &Document) -> String {
    let frames: Vec<String> = doc
        .objects
        .iter()
        .filter(|object| object.story_id().is_some())
        .map(|object| format!("{:?}{:?}", object.id, object.pos))
        .collect();
    json!({
        "pages": doc.pages.len(),
        "sections": doc.sections,
        "frames": frames,
        "created": doc.field_info.created,
        "saved": doc.field_info.saved,
        "printed": doc.field_info.printed,
        "path": doc.field_info.file_path,
        "offset": doc.field_info.utc_offset_min,
        "meta": doc.metadata,
        "toc": doc.toc.spec
    })
    .to_string()
}

pub fn refresh_fields(session: &mut Session) -> bool {
    let targets: Vec<(StoryId, Vec<usize>)> = session
        .document
        .stories
        .iter()
        .map(|story| {
            let indexes: Vec<usize> = story
                .ranges
                .iter()
                .enumerate()
                .filter_map(|(index, run)| read_field_run(run).map(|_| index))
                .collect();
            (story.id, indexes)
        })
        .filter(|(_, indexes)| !indexes.is_empty())
        .collect();
    let mut changed = false;
    for (story_id, indexes) in targets {
        let before = find_story(&session.document, story_id).map(|story| story.text.clone()).unwrap_or_default();
        for index in indexes.into_iter().rev() {
            if run_update_one(session, story_id, index, None).is_err() {
                break;
            }
        }
        if find_story(&session.document, story_id).map(|story| story.text != before).unwrap_or(false) {
            changed = true;
        }
    }
    session.document.field_info.resolved_sig = Some(field_signature(&session.document));
    if changed {
        super::xref::refresh_xrefs(&mut session.document);
        session.generation += 1;
    }
    changed
}

pub fn refresh_if_stale(session: &mut Session) -> bool {
    if !has_field_runs(&session.document) {
        return false;
    }
    let current = field_signature(&session.document);
    if session.document.field_info.resolved_sig.as_deref() == Some(current.as_str()) {
        return false;
    }
    refresh_fields(session)
}

pub fn is_print_command(name: &str) -> bool {
    ["export", "doc.export", "file.export", "print", "doc.print", "file.print"]
        .iter()
        .any(|prefix| name == *prefix || name.starts_with(&format!("{prefix}.")) || name.starts_with(&format!("{prefix}_")))
}

pub fn stamp_printed(session: &mut Session) {
    if !has_field_runs(&session.document) && session.document.field_info.printed.is_none() {
        return;
    }
    session.document.field_info.printed = Some(now_unix());
    refresh_fields(session);
}

pub fn stamp_saved(session: &mut Session, path: &std::path::Path) -> FieldDocInfo {
    let before = session.document.field_info.clone();
    let tracked = before.created.is_some() || before.saved.is_some() || before.file_path.is_some();
    if !tracked && !has_field_runs(&session.document) {
        return before;
    }
    let now = now_unix();
    let info = &mut session.document.field_info;
    info.created.get_or_insert(now);
    info.saved = Some(now);
    info.file_path = Some(path.to_string_lossy().to_string());
    refresh_fields(session);
    before
}
