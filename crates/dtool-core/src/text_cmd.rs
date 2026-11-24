use crate::command::Session;
use crate::document::{Document, Story, Special, BreakKind};
use crate::longdoc::marks::{self, StoryMarks};
use crate::object::{Object, ObjectId, ObjectKind, PathTextData};
use crate::table::model::StoryId;
use crate::undo::Change;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const DEFAULT_SIZE_PT: f32 = 12.0;
const FIXTURE_WIDTH_RATIO: f32 = 0.5;

pub fn can_execute(cmd: &Command) -> bool {
    matches!(
        cmd.name.as_str(),
        "text.art.add"
            | "text.art.vertical.set"
            | "text.frame.add"
            | "text.link"
            | "text.unlink"
            | "text.link.undo"
            | "text.shape_text.add"
            | "text.frame.set"
            | "text.frame.undo_add"
            | "text.shape_text.undo_add"
            | "text.insert"
            | "text.delete"
            | "text.range.replace"
            | "text.select"
            | "text.edit.end"
            | "text.edit.type"
            | "text.toggle_unicode"
            | "text.edit.backspace"
            | "text.edit.delete_forward"
            | "text.edit.caret"
            | "text.edit.compose"
            | "text.edit.compose_commit"
            | "text.edit.compose_cancel"
            | "text.to_path"
            | "text.path.attach"
            | "text.path.set"
            | "text.path_text.add"
            | "text.art.undo_add"
            | "text.to_path.undo"
            | "text.insert.undo"
            | "text.story.restore"
            | "text.special.undo"
            | "doc.object.query"
            | "font.usage.query"
            | "font.substitution.set"
            | "font.replace"
            | "font.replace.undo"
            | "text.glyph.insert"
            | "text.style"
            | "text.style.query"
            | "text.special.insert"
            | "text.link.set"
            | "text.para.set"
            | "text.para.query"
            | "text.para.set.undo"
            | "text.style.undo"
    )
}

pub fn run(cmd: &Command, session: &mut Session) -> CommandResult {
    if let Some(result)=crate::commands::frame_text::run(session,cmd) {return result;}
    match cmd.name.as_str() {
        "text.art.add" => run_art_add(cmd, session),
        "text.art.vertical.set" => run_art_vertical(cmd, session),
        "text.insert" => run_insert(cmd, session),
        "text.delete" => run_delete(cmd, session),
        "text.range.replace" => run_range_replace(cmd, session),
        "text.select" => run_select(cmd, session),
        "text.edit.end" => run_edit_end(cmd, session),
        "text.edit.type" => run_edit_type(cmd, session),
        "text.toggle_unicode" => run_toggle_unicode(session),
        "text.edit.backspace" => run_edit_erase(session, false),
        "text.edit.delete_forward" => run_edit_erase(session, true),
        "text.edit.caret" => run_edit_caret(cmd, session),
        "text.edit.compose" => run_edit_compose(cmd, session),
        "text.edit.compose_commit" => run_edit_compose_commit(cmd, session),
        "text.edit.compose_cancel" => run_edit_compose_cancel(session),
        "text.to_path" => run_to_path(cmd, session),
        "text.path.attach" => run_path_attach(cmd, session),
        "text.path.set" => run_path_set(cmd, session),
        "text.path_text.add" => run_path_text_add(cmd, session),
        "doc.object.query" => run_doc_query(cmd, session),
        "font.usage.query" => run_font_usage_query(cmd, session),
        "font.substitution.set" => run_font_substitution(cmd, session),
        "font.replace" => run_font_replace(cmd, session),
        "text.glyph.insert" => run_glyph_insert(cmd, session),
        "text.style" => {
            let result = run_text_style(cmd, session);
            if result.is_ok() {
                if let Some(family) = cmd.params.get("patch").and_then(|patch| patch.get("family")).and_then(|family| family.as_str()) {
                    crate::commands::text_host::push_recent_font(family);
                }
            }
            result
        }
        "text.style.query" => run_text_style_query(cmd, session),
        "text.special.insert" => run_special_insert(cmd, session),
        "text.link.set" => run_link_set(cmd, session),
        "text.para.set" => run_para_set(cmd, session),
        "text.para.query" => run_para_query(cmd, session),
        "text.art.undo_add" | "text.to_path.undo" | "text.insert.undo" | "text.story.restore" | "text.para.set.undo" | "text.special.undo" | "text.style.undo" | "font.replace.undo" => {
            apply(cmd, &mut session.document)?;
            bump(session);
            Ok(json!({ "doc_version": session.generation }))
        }
        _ => Err(CommandError::not_implemented()),
    }
}

pub fn apply(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    if let Some(result)=crate::commands::frame_text::apply_doc(doc,cmd) {return result;}
    match cmd.name.as_str() {
        "text.art.add" => apply_art_add(cmd, doc),
        "text.insert" => apply_insert(cmd, doc),
        "text.delete" => apply_delete(cmd, doc),
        "text.range.replace" => apply_range_replace(cmd, doc),
        "text.select" | "text.edit.end" => Ok(()),
        "text.to_path" => apply_to_path(cmd, doc),
        "text.path.attach" => apply_path_attach(cmd, doc),
        "text.path.set" => apply_path_set(cmd, doc),
        "text.path_text.add" => apply_path_text_add(cmd, doc),
        "text.art.undo_add" => apply_art_undo_add(cmd, doc),
        "text.to_path.undo" => apply_to_path_undo(cmd, doc),
        "text.insert.undo" => apply_insert_undo(cmd, doc),
        "text.story.restore" => apply_story_restore(cmd, doc),
        "text.special.undo" => apply_special_undo(cmd, doc),
        "text.special.insert" => apply_special_redo(cmd, doc),
        "font.replace" => apply_font_replace(cmd, doc),
        "font.replace.undo" => apply_font_replace_undo(cmd, doc),
        "doc.object.query" | "font.usage.query" | "text.para.query" => Ok(()),
        "text.para.set" => apply_para_set(cmd, doc),
        "text.para.set.undo" => apply_para_set_undo(cmd, doc),
        "text.style.undo" => apply_text_style_undo(cmd, doc),
        "text.style" => apply_text_style(cmd, doc),
        _ => Ok(()),
    }
}

pub(crate) fn apply_session(cmd: &Command, session: &mut Session) -> Result<(), CommandError> {
    if cmd.name == "text.range.replace" {
        let (story, start, end, text) = range_replace_params(cmd)?;
        let before = find_story(&session.document, story)?.clone();
        apply_range_replace(cmd, &mut session.document)?;
        sync_edited_story(session, &before, start, end - start, scalar_len(text));
        return Ok(());
    }
    let before = if !session.document.symbols.instances.is_empty()
        && matches!(cmd.name.as_str(), "text.insert" | "text.delete" | "text.style" | "text.para.set")
    {
        let story = StoryId(param_u64(&cmd.params, "story")?);
        Some(find_story(&session.document, story)?.clone())
    } else {
        None
    };
    apply(cmd, &mut session.document)?;
    if let Some(before) = before {
        sync_linked_change(session, &before);
    }
    if cmd.name == "text.story.restore" {
        if let Some(edit) = session.text_edit.as_mut().filter(|edit| cmd.params["story"]["id"].as_u64() == Some(edit.story.0)) {
            if let Ok([caret, anchor]) = serde_json::from_value::<[usize; 2]>(cmd.params["edit_selection"].clone()) {
                let len = cmd.params["story"]["text"].as_str().map_or(0, scalar_len);
                edit.caret = caret.min(len);
                edit.anchor = anchor.min(len);
            }
        }
    }
    Ok(())
}

pub fn bump(session: &mut Session) {
    session.generation += 1;
    session.document.version += 1;
}

pub fn push_change(session: &mut Session, name: &str, redo: Command, undo: Command) {
    session.undo.push(Change {
        name: name.to_string(),
        redo,
        undo,
    });
    bump(session);
}

pub fn param_u64(params: &Value, key: &str) -> Result<u64, CommandError> {
    let value = params.get(key).ok_or_else(CommandError::invalid_params)?;
    if let Some(n) = value.as_u64() {
        return Ok(n);
    }
    if let Some(n) = value.as_i64() {
        if n >= 0 {
            return Ok(n as u64);
        }
    }
    if let Some(s) = value.as_str() {
        return s.parse().map_err(|_| CommandError::invalid_params());
    }
    Err(CommandError::invalid_params())
}

pub fn optional_u64(params: &Value, key: &str) -> Result<Option<u64>, CommandError> {
    if params.get(key).is_none() || params.get(key) == Some(&Value::Null) {
        return Ok(None);
    }
    param_u64(params, key).map(Some)
}

fn param_pos(params: &Value) -> Result<[f32; 2], CommandError> {
    let arr = params
        .get("pos")
        .and_then(|v| v.as_array())
        .ok_or_else(CommandError::invalid_params)?;
    if arr.len() != 2 {
        return Err(CommandError::invalid_params());
    }
    let x = arr[0].as_f64().ok_or_else(CommandError::invalid_params)? as f32;
    let y = arr[1].as_f64().ok_or_else(CommandError::invalid_params)? as f32;
    if !x.is_finite() || !y.is_finite() {
        return Err(CommandError::invalid_params());
    }
    Ok([x, y])
}

fn param_size_pt(params: &Value) -> Result<f32, CommandError> {
    match params.get("size_pt") {
        None | Some(Value::Null) => Ok(DEFAULT_SIZE_PT),
        Some(value) => {
            let size = value
                .as_f64()
                .ok_or_else(CommandError::invalid_params)? as f32;
            if !size.is_finite() || size <= 0.0 {
                return Err(CommandError::invalid_params());
            }
            Ok(size)
        }
    }
}

pub fn param_text(params: &Value) -> String {
    params
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn param_at(params: &Value) -> Result<usize, CommandError> {
    let value = params.get("at").ok_or_else(CommandError::invalid_params)?;
    if let Some(n) = value.as_u64() {
        return Ok(n as usize);
    }
    if let Some(s) = value.as_str() {
        return s.parse().map_err(|_| CommandError::invalid_params());
    }
    Err(CommandError::invalid_params())
}

fn param_range(params: &Value) -> Result<(usize, usize), CommandError> {
    let range = params.get("range").ok_or_else(CommandError::invalid_params)?;
    let start = range
        .get("start")
        .and_then(|v| v.as_u64())
        .ok_or_else(CommandError::invalid_params)? as usize;
    let end = range
        .get("end")
        .and_then(|v| v.as_u64())
        .ok_or_else(CommandError::invalid_params)? as usize;
    Ok((start, end))
}

fn story_error() -> CommandError {
    CommandError::new("STORY_NOT_FOUND")
}

fn range_error() -> CommandError {
    CommandError::new("RANGE_OUT_OF_BOUNDS")
}

pub fn conflict_error() -> CommandError {
    CommandError::new("CONFLICT")
}

fn scalar_len(text: &str) -> usize {
    text.chars().count()
}

fn scalar_byte(text: &str, at: usize) -> Option<usize> {
    if at == 0 {
        return Some(0);
    }
    let mut n = 0;
    for (i, _) in text.char_indices() {
        if n == at {
            return Some(i);
        }
        n += 1;
    }
    if n == at {
        Some(text.len())
    } else {
        None
    }
}

fn insert_scalars(text: &mut String, at: usize, s: &str) -> Result<(), CommandError> {
    let byte = scalar_byte(text, at).ok_or_else(range_error)?;
    text.insert_str(byte, s);
    Ok(())
}

fn delete_scalars(text: &mut String, start: usize, end: usize) -> Result<(), CommandError> {
    if start > end {
        return Err(range_error());
    }
    let start_byte = scalar_byte(text, start).ok_or_else(range_error)?;
    let end_byte = scalar_byte(text, end).ok_or_else(range_error)?;
    if start_byte > end_byte {
        return Err(range_error());
    }
    let mut next = String::new();
    next.push_str(&text[..start_byte]);
    next.push_str(&text[end_byte..]);
    *text = next;
    Ok(())
}

pub fn measure_art(text: &str, size_pt: f32) -> [f32; 2] {
    if !size_pt.is_finite() || size_pt <= 0.0 {
        return [0.0, 0.0];
    }
    if text.is_empty() {
        return [0.0, size_pt];
    }
    let mut max_width = 0.0f32;
    let mut lines = 0u32;
    for line in text.split('\n') {
        let width = line.chars().count() as f32 * size_pt * FIXTURE_WIDTH_RATIO;
        if width > max_width {
            max_width = width;
        }
        lines += 1;
    }
    [max_width, lines as f32 * size_pt]
}

pub fn size_pt_of(story: &Story, fallback: f32) -> f32 {
    for range in &story.ranges {
        if let Some(size) = range.get("size_pt").and_then(|v| v.as_f64()) {
            let size = size as f32;
            if size.is_finite() && size > 0.0 {
                return size;
            }
        }
    }
    fallback
}

pub type FillerFit = fn(&Document, StoryId, &str) -> Option<String>;

static FILLER_FIT: std::sync::OnceLock<FillerFit> = std::sync::OnceLock::new();

pub fn register_filler_fit(fit: FillerFit) -> bool {
    FILLER_FIT.set(fit).is_ok() || FILLER_FIT.get().is_some()
}

pub fn filler_ready() -> bool {
    FILLER_FIT.get().is_some()
}

pub fn filler_text(source: &str, count: usize) -> String {
    let words: Vec<&str> = source.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }
    (0..count).map(|i| words[i % words.len()]).collect::<Vec<&str>>().join(" ")
}

pub fn fill_story(story: &mut Story, text: String) {
    let end = scalar_len(&text);
    let mut range = story.ranges.first().cloned().unwrap_or_else(|| json!({ "size_pt": DEFAULT_SIZE_PT }));
    range["start"] = json!(0);
    range["end"] = json!(end);
    let para = story.paras.first().cloned().map(|mut para| {
        if para.get("start").is_some() {
            para["start"] = json!(0);
        }
        if para.get("end").is_some() {
            para["end"] = json!(end);
        }
        para
    });
    story.text = text;
    story.ranges = vec![range];
    story.specials.clear();
    story.paras = para.into_iter().collect();
}

pub fn estimate_filler(doc: &Document, story: StoryId, source: &str) -> String {
    let size_pt = doc.stories.iter().find(|s| s.id == story).map(|s| size_pt_of(s, DEFAULT_SIZE_PT)).unwrap_or(DEFAULT_SIZE_PT);
    let size = size_pt * crate::units::units_per_point(doc.units, doc.dpi);
    let area: f32 = crate::frame::flow_frames(doc, story)
        .iter()
        .filter_map(|id| doc.get_object(*id))
        .map(|object| object.size[0].max(0.0) * object.size[1].max(0.0))
        .sum();
    let budget = (area / (size * 0.55 * size * 1.2) * 0.85).floor() as usize;
    let mut count = 0;
    while count < 100_000 && scalar_len(&filler_text(source, count + 1)) <= budget {
        count += 1;
    }
    filler_text(source, count)
}

pub fn fit_filler(doc: &Document, story: StoryId, source: &str) -> String {
    FILLER_FIT
        .get()
        .and_then(|fit| fit(doc, story, source))
        .unwrap_or_else(|| estimate_filler(doc, story, source))
}

pub fn set_size_range(story: &mut Story, size_pt: f32) {
    let end = scalar_len(&story.text);
    story.ranges = vec![json!({
        "start": 0,
        "end": end,
        "size_pt": size_pt
    })];
}

pub fn next_story_id(doc: &Document) -> u64 {
    doc.peek_story_id().0
}

fn find_story_mut(doc: &mut Document, id: StoryId) -> Result<&mut Story, CommandError> {
    doc.stories
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(story_error)
}

fn find_story(doc: &Document, id: StoryId) -> Result<&Story, CommandError> {
    doc.stories.iter().find(|s| s.id == id).ok_or_else(story_error)
}

fn objects_for_story(doc: &mut Document, id: StoryId) -> Vec<u64> {
    let mut ids: Vec<u64> = Vec::new();
    for owner in crate::frame::story_owners(doc, id) {
        let object = match owner {
            crate::frame::StoryOwner::Object(object) | crate::frame::StoryOwner::TableCell(object, _) => object.0,
        };
        if !ids.contains(&object) {
            ids.push(object);
        }
    }
    ids
}

fn estimated_art_size(doc: &Document, story: &Story) -> [f32; 2] {
    let size = size_pt_of(story, DEFAULT_SIZE_PT) * crate::units::units_per_point(doc.units, doc.dpi);
    measure_art(&story.text, size)
}

fn shaped_art_width(doc: &Document, id: ObjectId) -> Option<f32> {
    if !crate::text_lines::text_lines_ready() {
        return None;
    }
    crate::text_lines::text_lines(doc, id).iter().map(|line| line.width)
        .filter(|width| width.is_finite() && *width >= 0.0)
        .reduce(f32::max)
}

fn set_art_size(object: &mut Object, size: [f32; 2]) {
    let size = [size[0].copysign(object.size[0]), size[1].copysign(object.size[1])];
    if size == object.size {
        return;
    }
    // A different box size changes the pivot used by rotated and flipped objects.
    // Keep the rendered origin in the same place when refreshing that box.
    let origin = object.pos;
    let before = crate::geometry::apply_matrix(&crate::geometry::object_matrix(object), origin);
    object.size = size;
    let after = crate::geometry::apply_matrix(&crate::geometry::object_matrix(object), origin);
    object.pos = [origin[0] + (before[0] - after[0]), origin[1] + (before[1] - after[1])];
}

pub(crate) fn refresh_art_bounds(doc: &mut Document, id: StoryId) -> Result<(), CommandError> {
    let fallback = estimated_art_size(doc, find_story(doc, id)?);
    for object_id in objects_for_story(doc, id) {
        let object_id = ObjectId(object_id);
        if !doc.get_object(object_id).is_some_and(|obj| matches!(obj.kind, ObjectKind::ArtText { .. })) {
            continue;
        }
        let width = shaped_art_width(doc, object_id).unwrap_or(fallback[0]);
        if let Some(obj) = doc.find_object_mut(object_id.0) {
            let [sx, sy] = obj.text_scale;
            let size = if obj.is_vertical_art_text() {
                [fallback[1] * sx, width * sy]
            } else {
                [width * sx, fallback[1] * sy]
            };
            set_art_size(obj, size);
        }
    }
    Ok(())
}

pub(crate) fn refresh_art_width(doc: &mut Document, id: ObjectId) {
    if let Some(width) = shaped_art_width(doc, id) {
        if let Some(object) = doc.find_object_mut(id.0) {
            let size = if object.is_vertical_art_text() {
                [object.size[0].abs(), width * object.text_scale[1]]
            } else {
                [width * object.text_scale[0], object.size[1].abs()]
            };
            set_art_size(object, size);
        }
    }
}

pub(crate) fn refresh_legacy_art_widths(doc: &mut Document) {
    if !crate::text_lines::text_lines_ready() {
        return;
    }
    let updates: Vec<_> = doc.objects.iter().filter_map(|object| {
        let ObjectKind::ArtText { story } = object.kind else { return None; };
        let story = find_story(doc, story).ok()?;
        // Only replace the old character-count estimate. A sized object keeps its box.
        let estimated = estimated_art_size(doc, story)[0];
        if object.is_vertical_art_text() || object.text_scale != crate::object::IDENTITY_TEXT_SCALE
            || (object.size[0].abs() - estimated).abs() > 0.001
        {
            return None;
        }
        Some((object.id, shaped_art_width(doc, object.id)?, object.size[1].abs()))
    }).collect();
    for (id, width, height) in updates {
        if let Some(object) = doc.find_object_mut(id.0) {
            set_art_size(object, [width, height]);
        }
    }
}

fn run_art_add(cmd: &Command, session: &mut Session) -> CommandResult {
    let mut redo = cmd.clone();
    if optional_u64(&redo.params, "id")?.is_none() {
        redo.params["id"] = session.document.next_object_id.into();
    }
    if optional_u64(&redo.params, "story")?.is_none() {
        redo.params["story"] = next_story_id(&session.document).into();
    }
    apply_art_add(&redo, &mut session.document)?;
    let id = param_u64(&redo.params, "id")?;
    let story = param_u64(&redo.params, "story")?;
    let mut undo = Command::new("text.art.undo_add");
    undo.params["id"] = id.into();
    undo.params["story"] = story.into();
    push_change(session, "text.art.add", redo, undo);
    Ok(json!({
        "doc_version": session.generation,
        "id": id,
        "story": story
    }))
}

fn run_art_vertical(cmd: &Command, session: &mut Session) -> CommandResult {
    let id = ObjectId(param_u64(&cmd.params, "id")?);
    let vertical = cmd.params.get("vertical").and_then(Value::as_bool).ok_or_else(CommandError::invalid_params)?;
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    let ObjectKind::ArtText { story } = object.kind else { return Err(CommandError::invalid_params()); };
    if object.locked { return Err(CommandError::new("CANNOT_EXECUTE")); }
    if object.vertical_art_text == vertical {
        return Ok(json!({ "id": id.0, "vertical": vertical, "doc_version": session.generation }));
    }
    let mut doc = session.document.clone();
    doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?.vertical_art_text = vertical;
    refresh_art_bounds(&mut doc, story)?;
    crate::commands::document_change::commit(session, &cmd.name, doc, session.selection.ids.clone(),
        json!({ "id": id.0, "vertical": vertical }))
}

fn apply_art_add(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let pos = param_pos(&cmd.params)?;
    let size_pt = param_size_pt(&cmd.params)?;
    let text = param_text(&cmd.params);
    let object_id = ObjectId(param_u64(&cmd.params, "id")?);
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    if doc.stories.iter().any(|s| s.id == story_id) || doc.get_object(object_id).is_some() {
        return Err(conflict_error());
    }
    let mut story = Story {
        id: story_id,
        text,
        ranges: Vec::new(), specials: Vec::new(), paras: Vec::new(),
    };
    set_size_range(&mut story, size_pt);
    let vertical_art_text = cmd.params.get("vertical").map(|value| value.as_bool().ok_or_else(CommandError::invalid_params)).transpose()?.unwrap_or(false);
    let mut size = estimated_art_size(doc, &story);
    if vertical_art_text { size.swap(0, 1); }
    let mut object = Object {
        id: object_id,
        pos,
        size,
        kind: ObjectKind::ArtText { story: story_id },
        vertical_art_text,
        name: story.text.clone(),
        ..Object::default()
    };
    crate::artboard::edit::assign_new_object(doc, &mut object, pos);
    doc.stories.push(story);
    let id = doc.create_object(object);
    refresh_art_width(doc, id);
    Ok(())
}

pub fn apply_art_undo_add(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let id = ObjectId(param_u64(&cmd.params, "id")?);
    let story = StoryId(param_u64(&cmd.params, "story")?);
    doc.objects.retain(|obj| obj.id != id);
    crate::frame::drop_story_if_unowned(doc, story);
    Ok(())
}

fn run_insert(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let at = param_at(&cmd.params)?;
    let text = param_text(&cmd.params);
    let snapshot = find_story(&session.document, story_id)?.clone();
    let marks = capture_edit_marks(session, story_id);
    apply_insert(cmd, &mut session.document)?;
    let linked = sync_edited_story(session, &snapshot, at, 0, scalar_len(&text));
    let recorded = record_in_edit_session(session, story_id, snapshot.clone(), linked.as_ref().map(|(before, _)| before.clone()), marks.as_ref());
    if let Some(edit) = session.text_edit.as_mut().filter(|edit| edit.story == story_id) {
        let count = scalar_len(&text);
        if edit.caret >= at {
            edit.caret += count;
        }
        if edit.anchor >= at {
            edit.anchor += count;
        }
    }
    if !recorded {
        finish_direct_text_change(session, cmd, snapshot, linked, marks.as_ref())?;
    }
    Ok(json!({
        "doc_version": session.generation,
        "story": story_id.0,
        "at": at,
        "length": text.chars().count()
    }))
}

fn apply_insert(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let at = param_at(&cmd.params)?;
    let text = param_text(&cmd.params);
    let size_pt = {
        let story = find_story_mut(doc, story_id)?;
        let mut chars = story.text.chars();
        let starts_existing_paragraph = at == 0 || (
            chars.nth(at - 1).is_some_and(crate::commands::active_story::is_para_break)
                && chars.next().is_some()
        );
        insert_scalars(&mut story.text, at, &text)?;
        let count = scalar_len(&text);
        shift_paras_insert(&mut story.paras, at, count, starts_existing_paragraph);
        continue_list_paragraphs(story, at, &text, count);
        let size_pt = size_pt_of(story, DEFAULT_SIZE_PT);
        shift_ranges_insert(&mut story.ranges, at, count);
        shift_specials_insert(&mut story.specials, at, count);
        if story.ranges.is_empty() {
            set_size_range(story, size_pt);
        }
        size_pt
    };
    let _ = size_pt;
    refresh_art_bounds(doc, story_id)
}

fn apply_insert_undo(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let text = param_text(&cmd.params);
    {
        let story = find_story_mut(doc, story_id)?;
        story.text = text;
        if let Some(paras) = cmd.params.get("paras") {
            if let Ok(restored) = serde_json::from_value::<Vec<Value>>(paras.clone()) {
                story.paras = restored;
            }
        }
        if let Some(specials) = cmd.params.get("specials") {
            if let Ok(restored) = serde_json::from_value::<Vec<(usize, crate::document::Special)>>(specials.clone()) {
                story.specials = restored;
            }
        }
        match cmd.params.get("ranges").and_then(|ranges| serde_json::from_value::<Vec<Value>>(ranges.clone()).ok()) {
            Some(restored) => story.ranges = restored,
            None => {
                let size_pt = size_pt_of(story, DEFAULT_SIZE_PT);
                set_size_range(story, size_pt);
            }
        }
    }
    refresh_art_bounds(doc, story_id)
}

fn run_delete(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let (start, end) = param_range(&cmd.params)?;
    let snapshot = find_story(&session.document, story_id)?.clone();
    let marks = capture_edit_marks(session, story_id);
    apply_delete(cmd, &mut session.document)?;
    let linked = sync_edited_story(session, &snapshot, start, end - start, 0);
    let recorded = record_in_edit_session(session, story_id, snapshot.clone(), linked.as_ref().map(|(before, _)| before.clone()), marks.as_ref());
    if let Some(edit) = session.text_edit.as_mut().filter(|edit| edit.story == story_id) {
        let shift = |pos: usize| if pos >= end { pos - (end - start) } else { pos.min(start) };
        edit.caret = shift(edit.caret);
        edit.anchor = shift(edit.anchor);
    }
    if !recorded {
        finish_direct_text_change(session, cmd, snapshot, linked, marks.as_ref())?;
    }
    Ok(json!({ "doc_version": session.generation, "story": story_id.0 }))
}

fn apply_delete(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let (start, end) = param_range(&cmd.params)?;
    {
        let story = find_story_mut(doc, story_id)?;
        let size_pt = size_pt_of(story, DEFAULT_SIZE_PT);
        delete_scalars(&mut story.text, start, end)?;
        let empty_paragraph = empty_final_paragraph(&story.text);
        shift_runs_delete(&mut story.paras, start, end, empty_paragraph);
        shift_runs_delete(&mut story.ranges, start, end, None);
        shift_specials_delete(&mut story.specials, start, end);
        if story.ranges.is_empty() {
            set_size_range(story, size_pt);
        }
    }
    refresh_art_bounds(doc, story_id)
}

pub fn range_replace_command(story: StoryId, start: usize, end: usize, text: &str) -> Command {
    Command::with_params("text.range.replace", json!({
        "story": story.0, "range": { "start": start, "end": end }, "text": text
    }))
}

fn range_replace_params(cmd: &Command) -> Result<(StoryId, usize, usize, &str), CommandError> {
    let story = StoryId(param_u64(&cmd.params, "story")?);
    let (start, end) = param_range(&cmd.params)?;
    let text = cmd.params["text"].as_str().ok_or_else(CommandError::invalid_params)?;
    Ok((story, start, end, text))
}

fn apply_range_replace(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let (story, start, end, text) = range_replace_params(cmd)?;
    if start > end || end > scalar_len(&find_story(doc, story)?.text) {
        return Err(CommandError::out_of_range());
    }
    if start < end {
        let delete = Command::with_params("text.delete", json!({ "story": story.0, "range": cmd.params["range"] }));
        apply_delete(&delete, doc)?;
    }
    if !text.is_empty() {
        let insert = Command::with_params("text.insert", json!({ "story": story.0, "at": start, "text": text }));
        apply_insert(&insert, doc)?;
    }
    Ok(())
}

fn run_range_replace(cmd: &Command, session: &mut Session) -> CommandResult {
    let (story, start, end, text) = range_replace_params(cmd)?;
    let before = find_story(&session.document, story)?.clone();
    let marks = capture_edit_marks(session, story);
    apply_range_replace(cmd, &mut session.document)?;
    let inserted = scalar_len(text);
    let linked = sync_edited_story(session, &before, start, end - start, inserted);
    let recorded = record_in_edit_session(session, story, before.clone(), linked.as_ref().map(|(old, _)| old.clone()), marks.as_ref());
    if let Some(edit) = session.text_edit.as_mut().filter(|edit| edit.story == story) {
        let relocate = |at| {
            if at < start { at } else if at < end { start + inserted } else { at - (end - start) + inserted }
        };
        edit.caret = relocate(edit.caret);
        edit.anchor = relocate(edit.anchor);
    }
    if !recorded {
        finish_direct_text_change(session, cmd, before, linked, marks.as_ref())?;
    }
    Ok(json!({ "doc_version": session.generation, "story": story.0 }))
}

fn run_select(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let (start, end) = param_range(&cmd.params)?;
    let story=find_story(&session.document,story_id)?;
    if start>end || end>scalar_len(&story.text) {return Err(CommandError::invalid_params());}
    let object = optional_u64(&cmd.params, "id")?.map(ObjectId);
    if let Some(id) = object {
        let owns_story = session.document.get_object(id).is_some_and(|object| {
            object.story_id() == Some(story_id) || matches!(&object.kind, ObjectKind::Table(table)
                if table.cells.iter().any(|cell| cell.story == story_id && cell.merged_into.is_none()))
        });
        if !owns_story {
            return Err(CommandError::invalid_params());
        }
    }
    let before = match session.text_edit.take() {
        Some(edit) if edit.story == story_id && (edit.caret.min(edit.anchor), edit.caret.max(edit.anchor)) == (start, end) => edit.before,
        previous => { session.text_edit = previous; commit_text_edit(session); None }
    };
    session.text_edit = Some(crate::command::TextEditState {
        story: story_id,
        object,
        caret: end,
        anchor: start,
        before,
    });
    Ok(json!({
        "doc_version": session.generation,
        "story": story_id.0,
        "range": { "start": start, "end": end }
    }))
}

fn run_edit_end(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = optional_u64(&cmd.params, "story")?;
    if let Some(id) = story_id {
        find_story(&session.document, StoryId(id))?;
    }
    let cancel = match cmd.params.get("cancel") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(CommandError::invalid_params()),
    };
    let pending = session.text_composing.take();
    if !cancel && session.text_edit.is_some() {
        if let Some(text) = pending.filter(|text| !text.is_empty()) {
            let (_, anchor, caret, _) = edit_target(session)?;
            edit_replace(session, anchor.min(caret), anchor.max(caret), &text)?;
        }
    }
    if cancel {
        if let Some(before) = session.text_edit.take().and_then(|edit| edit.before) {
            let id = before.story.id;
            if let Some(story) = session.document.stories.iter_mut().find(|story| story.id == id) {
                *story = before.story;
            }
            if let Some(linked) = before.linked_text {
                crate::commands::symbols::restore_text(&mut session.document, id, &linked);
            }
            if let Some(marks) = before.marks {
                marks.restore(&mut session.document);
            }
            refresh_art_bounds(&mut session.document, id)?;
            bump(session);
        }
    } else {
        commit_text_edit(session);
    }
    session.text_edit = None;
    Ok(json!({ "doc_version": session.generation, "cancelled": cancel }))
}

pub fn is_text_edit_command(name: &str) -> bool {
    matches!(
        name,
        "text.insert"
            | "text.delete"
            | "text.range.replace"
            | "text.select"
            | "text.edit.end"
            | "text.edit.type"
            | "text.toggle_unicode"
            | "text.edit.backspace"
            | "text.edit.delete_forward"
            | "text.edit.caret"
            | "text.edit.compose"
            | "text.edit.compose_commit"
            | "text.edit.compose_cancel"
    )
}

fn edit_target(session: &Session) -> Result<(StoryId, usize, usize, usize), CommandError> {
    let edit = session.text_edit.as_ref().ok_or_else(CommandError::invalid_params)?;
    let story = find_story(&session.document, edit.story)?;
    let len = scalar_len(&story.text);
    Ok((edit.story, edit.anchor.min(len), edit.caret.min(len), len))
}

fn set_caret(session: &mut Session, caret: usize, anchor: usize) {
    if let Some(edit) = session.text_edit.as_mut() {
        edit.caret = caret;
        edit.anchor = anchor;
    }
}

fn edit_state(session: &Session, story: StoryId) -> Value {
    let (caret, anchor) = session.text_edit.as_ref().map(|edit| (edit.caret, edit.anchor)).unwrap_or((0, 0));
    json!({
        "doc_version": session.generation,
        "story": story.0,
        "caret": caret,
        "anchor": anchor,
        "composing": session.text_composing
    })
}

fn edit_replace(session: &mut Session, start: usize, end: usize, text: &str) -> CommandResult {
    let (story, _, _, _) = edit_target(session)?;
    if end > start && !text.is_empty() {
        run_range_replace(&range_replace_command(story, start, end, text), session)?;
    } else if end > start {
        run_delete(&Command::with_params("text.delete", json!({ "story": story.0, "range": { "start": start, "end": end } })), session)?;
    } else if !text.is_empty() {
        run_insert(&Command::with_params("text.insert", json!({ "story": story.0, "at": start, "text": text })), session)?;
    }
    let caret = start + scalar_len(text);
    set_caret(session, caret, caret);
    Ok(edit_state(session, story))
}

fn param_edit_text(cmd: &Command) -> Result<String, CommandError> {
    cmd.params.get("text").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(CommandError::invalid_params)
}

fn run_edit_type(cmd: &Command, session: &mut Session) -> CommandResult {
    let text = param_edit_text(cmd)?;
    let (story, anchor, caret, _) = edit_target(session)?;
    if text == "\n" && anchor == caret
        && let Some(command) = end_empty_list_item(session, story, caret)
    {
        commit_text_edit(session);
        run_para_set(&command, session)?;
        return Ok(edit_state(session, story));
    }
    edit_replace(session, anchor.min(caret), anchor.max(caret), &text)
}

fn end_empty_list_item(session: &Session, story: StoryId, caret: usize) -> Option<Command> {
    let (_, [start, end]) = session.active_para_range()?;
    if start != caret { return None; }
    let document_story = find_story(&session.document, story).ok()?;
    let byte = scalar_byte(&document_story.text, caret)?;
    let empty = end == caret || document_story.text[byte..].starts_with(crate::commands::active_story::is_para_break);
    if !empty { return None; }
    let style = query_para_style(&document_story.paras, start, end);
    let mut list = style["list"].as_object()?.clone();
    if !list.get("kind").is_some_and(is_list_kind) { return None; }
    list.insert("kind".into(), "None".into());
    list.remove("restart");
    list.remove("start_at");
    Some(Command::with_params("text.para.set", json!({
        "story": story.0, "range": [start, end], "patch": { "list": list }
    })))
}

fn parse_unicode_notation(text: &str) -> Option<char> {
    let prefixed = text.starts_with("U+") || text.starts_with("u+");
    let digits = if prefixed { &text[2..] } else { text };
    let minimum = if prefixed { 1 } else { 4 };
    if !(minimum..=6).contains(&digits.len()) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    char::from_u32(u32::from_str_radix(digits, 16).ok()?)
}

fn unicode_replacement(session: &Session) -> Option<(usize, usize, String)> {
    if session.text_composing.is_some() { return None; }
    let (story, anchor, caret, _) = edit_target(session).ok()?;
    let text = &find_story(&session.document, story).ok()?.text;
    let start = anchor.min(caret);
    let end = anchor.max(caret);
    let before = &text[..scalar_byte(text, end)?];

    if start != end {
        let selected = &before[scalar_byte(text, start)?..];
        if let Some(ch) = parse_unicode_notation(selected) {
            return Some((start, end, ch.to_string()));
        }
        let ch = selected.chars().next()?;
        return (end - start == 1).then(|| (start, end, format!("U+{:04X}", u32::from(ch))));
    }

    let digit_count = before.bytes().rev().take_while(u8::is_ascii_hexdigit).count();
    let preceding = &before[..before.len() - digit_count];
    let prefixed = preceding.ends_with("U+") || preceding.ends_with("u+");
    if prefixed || digit_count >= 4 {
        let code_len = digit_count + if prefixed { 2 } else { 0 };
        let (prefix, notation) = before.split_at(before.len() - code_len);
        if prefixed || !prefix.ends_with(|ch: char| ch.is_alphanumeric() || ch == '_') {
            let ch = parse_unicode_notation(notation)?;
            return Some((end - code_len, end, ch.to_string()));
        }
    }

    let ch = before.chars().next_back()?;
    Some((end - 1, end, format!("U+{:04X}", u32::from(ch))))
}

pub fn can_toggle_unicode(session: &Session) -> bool {
    session.has_document && unicode_replacement(session).is_some()
}

fn run_toggle_unicode(session: &mut Session) -> CommandResult {
    let (start, end, text) = unicode_replacement(session).ok_or_else(CommandError::invalid_params)?;
    commit_text_edit(session);
    let result = edit_replace(session, start, end, &text)?;
    commit_text_edit(session);
    Ok(result)
}

fn run_edit_erase(session: &mut Session, forward: bool) -> CommandResult {
    let (_, anchor, caret, len) = edit_target(session)?;
    let (start, end) = if anchor != caret {
        (anchor.min(caret), anchor.max(caret))
    } else if forward {
        (caret, (caret + 1).min(len))
    } else {
        (caret.saturating_sub(1), caret)
    };
    edit_replace(session, start, end, "")
}

fn run_edit_caret(cmd: &Command, session: &mut Session) -> CommandResult {
    let (story, anchor, caret, len) = edit_target(session)?;
    let extend = match cmd.params.get("extend") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(CommandError::invalid_params()),
    };
    let target = match (cmd.params.get("to"), cmd.params.get("by")) {
        (Some(to), None) => to.as_u64().ok_or_else(CommandError::invalid_params)? as i64,
        (None, Some(by)) => caret as i64 + by.as_i64().ok_or_else(CommandError::invalid_params)?,
        _ => return Err(CommandError::invalid_params()),
    };
    let target = target.clamp(0, len as i64) as usize;
    commit_text_edit(session);
    set_caret(session, target, if extend { anchor } else { target });
    Ok(edit_state(session, story))
}

fn run_edit_compose(cmd: &Command, session: &mut Session) -> CommandResult {
    let text = param_edit_text(cmd)?;
    let (story, _, _, _) = edit_target(session)?;
    session.text_composing = if text.is_empty() { None } else { Some(text) };
    Ok(edit_state(session, story))
}

fn run_edit_compose_commit(cmd: &Command, session: &mut Session) -> CommandResult {
    let (_, anchor, caret, _) = edit_target(session)?;
    let pending = session.text_composing.take();
    let text = match cmd.params.get("text") {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => pending.unwrap_or_default(),
        Some(_) => return Err(CommandError::invalid_params()),
    };
    edit_replace(session, anchor.min(caret), anchor.max(caret), &text)
}

fn run_edit_compose_cancel(session: &mut Session) -> CommandResult {
    let (story, _, _, _) = edit_target(session)?;
    session.text_composing = None;
    Ok(edit_state(session, story))
}

type LinkedTextState = crate::commands::symbols::LinkedTextState;

fn capture_edit_marks(session: &Session, story: StoryId) -> Option<StoryMarks> {
    let continuing = session.text_edit.as_ref()
        .is_some_and(|edit| edit.story == story && edit.before.is_some());
    if continuing { None } else { StoryMarks::capture(&session.document) }
}

pub(crate) fn sync_linked_change(session: &mut Session, before: &crate::document::Story) -> Option<(LinkedTextState, LinkedTextState)> {
    (find_story(&session.document, before.id).ok()? != before).then_some(())?;
    let (mut old, new) = crate::commands::symbols::sync_text(&mut session.document, before.id, session.symbol_sync)?;
    old.set_source_snapshot(before);
    Some((old, new))
}

fn sync_edited_story(
    session: &mut Session, before: &crate::document::Story,
    at: usize, removed: usize, inserted: usize,
) -> Option<(LinkedTextState, LinkedTextState)> {
    let has_marks = StoryMarks::present(&session.document);
    if has_marks {
        let position = session.document.story_pos(before.id, at);
        marks::shift_after_text_edit(&mut session.document, position, removed, inserted);
    }
    let linked = sync_linked_change(session, before);
    if has_marks {
        let doc = &mut session.document;
        if let Some((old, _)) = &linked {
            let old_stories: std::collections::HashMap<_, _> = old.stories()
                .filter(|story| story.id != before.id).map(|story| (story.id, story)).collect();
            let mut position = 0;
            let mut shifts = Vec::new();
            // Apply the other instances in document order, using each updated prefix length.
            for story in &doc.stories {
                let len = scalar_len(&story.text);
                if let Some(previous) = old_stories.get(&story.id).filter(|previous| previous.text != story.text) {
                    let exact = previous.text == before.text;
                    shifts.push((position, scalar_len(&previous.text), len, exact));
                }
                position += len + 1;
            }
            for (start, old_len, new_len, exact) in shifts {
                if exact {
                    marks::shift_after_text_edit(doc, start + at, removed, inserted);
                } else {
                    marks::shift_after_story_rebuild(doc, start, old_len, new_len);
                }
            }
        }
        crate::longdoc::xref::refresh_xrefs(doc);
    }
    linked
}

fn restore_text_command(
    story: &crate::document::Story, selection: Option<[usize; 2]>, linked: Option<&LinkedTextState>, marks: Option<&StoryMarks>,
) -> Command {
    Command::with_params("text.story.restore", json!({ "story": story, "edit_selection": selection, "linked_text": linked, "marks": marks }))
}

fn finish_direct_text_change(
    session: &mut Session, cmd: &Command, before: crate::document::Story,
    linked: Option<(LinkedTextState, LinkedTextState)>,
    marks: Option<&StoryMarks>,
) -> Result<(), CommandError> {
    if let Some(marks) = marks {
        let after = find_story(&session.document, before.id)?;
        let (old, new) = linked.as_ref().map_or((None, None), |(old, new)| (Some(old), Some(new)));
        let redo = restore_text_command(after, None, new, StoryMarks::capture(&session.document).as_ref());
        let undo = restore_text_command(&before, None, old, Some(marks));
        push_change(session, &cmd.name, redo, undo);
        return Ok(());
    }
    if !push_linked_text_change(session, &cmd.name, &before, linked)? {
        let undo = Command::with_params("text.insert.undo", json!({
            "story": before.id.0, "text": before.text, "paras": before.paras, "ranges": before.ranges, "specials": before.specials
        }));
        push_change(session, &cmd.name, cmd.clone(), undo);
    }
    Ok(())
}

pub(crate) fn push_linked_text_change(
    session: &mut Session, name: &str, before: &crate::document::Story,
    linked: Option<(LinkedTextState, LinkedTextState)>,
) -> Result<bool, CommandError> {
    if let Some((old, new)) = linked {
        let after = find_story(&session.document, before.id)?;
        let redo = restore_text_command(after, None, Some(&new), None);
        let undo = restore_text_command(before, None, Some(&old), None);
        push_change(session, name, redo, undo);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn record_in_edit_session(
    session: &mut Session, story: StoryId, snapshot: crate::document::Story,
    linked_text: Option<LinkedTextState>, marks: Option<&StoryMarks>,
) -> bool {
    let Some(edit) = session.text_edit.as_mut().filter(|edit| edit.story == story) else { return false };
    if let Some(before) = edit.before.as_mut() {
        if before.linked_text.is_none() {
            before.linked_text = linked_text;
        }
    } else {
        edit.before = Some(crate::command::TextEditSnapshot { story: snapshot, linked_text, marks: marks.cloned(), caret: edit.caret, anchor: edit.anchor });
    }
    bump(session);
    true
}

pub fn commit_text_edit(session: &mut Session) {
    let Some(edit) = session.text_edit.as_mut() else { return };
    let Some(before) = edit.before.take() else { return };
    let Some(after) = session.document.stories.iter().find(|story| story.id == before.story.id) else { return };
    let linked = before.linked_text.as_ref().and_then(|_| crate::commands::symbols::capture_text(&session.document, before.story.id));
    let marks = before.marks.as_ref().and_then(|_| StoryMarks::capture(&session.document));
    if after == &before.story && linked == before.linked_text && marks == before.marks { return; }
    session.undo.push(Change {
        name: "text.edit".to_string(),
        redo: restore_text_command(after, Some([edit.caret, edit.anchor]), linked.as_ref(), marks.as_ref()),
        undo: restore_text_command(&before.story, Some([before.caret, before.anchor]), before.linked_text.as_ref(), before.marks.as_ref()),
    });
}

fn apply_story_restore(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story: crate::document::Story = serde_json::from_value(cmd.params.get("story").cloned().ok_or_else(CommandError::invalid_params)?)
        .map_err(|_| CommandError::invalid_params())?;
    let linked = cmd.params.get("linked_text").filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone())).transpose().map_err(|_| CommandError::invalid_params())?;
    let marks: Option<StoryMarks> = cmd.params.get("marks").filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone())).transpose().map_err(|_| CommandError::invalid_params())?;
    let id = story.id;
    let slot = doc.stories.iter_mut().find(|existing| existing.id == id).ok_or_else(CommandError::not_found)?;
    *slot = story;
    if let Some(linked) = linked {
        crate::commands::symbols::restore_text(doc, id, &linked);
    }
    if let Some(marks) = marks {
        marks.restore(doc);
    }
    refresh_art_bounds(doc, id)
}

fn run_to_path(cmd: &Command, session: &mut Session) -> CommandResult {
    let id = ObjectId(param_u64(&cmd.params, "id")?);
    let object = session
        .document
        .get_object(id)
        .cloned()
        .ok_or_else(CommandError::not_found)?;
    let story_id = match &object.kind {
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story, .. } => Some(*story),
        ObjectKind::PathText(data) => Some(data.story),
        _ => return Err(CommandError::validation("INVALID_PARAMS")),
    };
    let story_index = story_id.and_then(|sid| session.document.stories.iter().position(|s| s.id == sid));
    let story = story_index.map(|index| session.document.stories[index].clone());
    apply_to_path(cmd, &mut session.document)?;
    let mut undo = Command::new("text.to_path.undo");
    undo.params["object"] = serde_json::to_value(&object).unwrap_or(Value::Null);
    if let (Some(story), Some(index)) = (story, story_index) {
        if !session.document.stories.iter().any(|s| s.id == story.id) {
            undo.params["story"] = serde_json::to_value(&story).unwrap_or(Value::Null);
            undo.params["story_index"] = json!(index);
        }
    }
    push_change(session, "text.to_path", cmd.clone(), undo);
    Ok(json!({
        "doc_version": session.generation,
        "id": id.0
    }))
}

fn apply_to_path(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let id = ObjectId(param_u64(&cmd.params, "id")?);
    let outline = crate::text_lines::text_outline_curve(doc, id)?;
    apply_text_outline(doc, id, outline)
}

pub(crate) fn apply_text_outline(
    doc: &mut Document,
    id: ObjectId,
    (pos, size, curve): crate::text_lines::TextOutline,
) -> Result<(), CommandError> {
    let (story_id, object_name, object_text) = {
        let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
        let story_id = match &object.kind {
            ObjectKind::ArtText { story } | ObjectKind::FrameText { story, .. } => *story,
            ObjectKind::PathText(data) => data.story,
            _ => return Err(CommandError::validation("INVALID_PARAMS")),
        };
        (story_id, object.name.clone(), doc.stories.iter().find(|s| s.id == story_id).map(|s| s.text.clone()))
    };
    if let Some(object) = doc.find_object_mut(id.0) {
        if object.name.is_empty() {
            if let Some(text) = object_text {
                object.name = text;
            } else if !object_name.is_empty() {
                object.name = object_name;
            }
        }
        if !matches!(object.kind, ObjectKind::PathText(_)) {
            object.rotation_deg = 0.0;
        }
        object.kind = ObjectKind::Curve(curve);
        object.shape = None;
        object.pos = pos;
        object.size = size;
    } else {
        return Err(CommandError::not_found());
    }
    crate::frame::drop_story_if_unowned(doc, story_id);
    Ok(())
}

fn apply_to_path_undo(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    if let Some(story_value) = cmd.params.get("story").cloned() {
        if !story_value.is_null() {
            let story: Story = serde_json::from_value(story_value).map_err(|_| CommandError::invalid_params())?;
            doc.stories.retain(|s| s.id != story.id);
            let index = cmd.params.get("story_index").and_then(Value::as_u64).map(|i| i as usize).unwrap_or(doc.stories.len());
            doc.stories.insert(index.min(doc.stories.len()), story);
        }
    }
    let object: Object = serde_json::from_value(cmd.params.get("object").cloned().unwrap_or(Value::Null))
        .map_err(|_| CommandError::invalid_params())?;
    let id = object.id;
    if let Some(existing) = doc.find_object_mut(id.0) {
        *existing = object;
        return Ok(());
    }
    let _ = doc.create_object(object);
    if let Some(existing) = doc.find_object_mut(id.0) {
        if existing.id != id {
            existing.id = id;
        }
    }
    Ok(())
}

fn restore_command(object: Object) -> Command {
    Command::with_params("object.restore", json!({ "objects": [object] }))
}

fn run_path_attach(cmd: &Command, session: &mut Session) -> CommandResult {
    let text_id = ObjectId(param_u64(&cmd.params, "text")?);
    let before = session.document.get_object(text_id).cloned().ok_or_else(CommandError::not_found)?;
    apply_path_attach(cmd, &mut session.document)?;
    push_change(session, "text.path.attach", cmd.clone(), restore_command(before));
    Ok(json!({ "doc_version": session.generation, "result_id": text_id.0 }))
}

fn apply_path_attach(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let text_id = ObjectId(param_u64(&cmd.params, "text")?);
    let path_id = ObjectId(param_u64(&cmd.params, "path")?);
    let story_id = {
        let object = doc.get_object(text_id).ok_or_else(CommandError::not_found)?;
        match &object.kind {
            ObjectKind::ArtText { story } => *story,
            _ => return Err(CommandError::validation("INVALID_PARAMS")),
        }
    };
    if path_id == text_id || doc.get_object(path_id).is_none() {
        return Err(CommandError::not_found());
    }
    let object = doc.find_object_mut(text_id.0).ok_or_else(CommandError::not_found)?;
    object.kind = ObjectKind::PathText(PathTextData {
        path: path_id,
        story: story_id,
        flows: Vec::new(),
        source_paint: None,
    });
    Ok(())
}

fn run_path_set(cmd: &Command, session: &mut Session) -> CommandResult {
    let id = ObjectId(param_u64(&cmd.params, "id")?);
    let before = session.document.get_object(id).cloned().ok_or_else(CommandError::not_found)?;
    apply_path_set(cmd, &mut session.document)?;
    push_change(session, "text.path.set", cmd.clone(), restore_command(before));
    Ok(json!({ "doc_version": session.generation }))
}

fn apply_path_set(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let id = ObjectId(param_u64(&cmd.params, "id")?);
    let flows = cmd
        .params
        .get("flows")
        .cloned()
        .ok_or_else(CommandError::invalid_params)?;
    let object = doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    match &mut object.kind {
        ObjectKind::PathText(data) => {
            data.flows = serde_json::from_value(flows).map_err(|_| CommandError::invalid_params())?;
            Ok(())
        }
        _ => Err(CommandError::validation("INVALID_PARAMS")),
    }
}

fn run_path_text_add(cmd: &Command, session: &mut Session) -> CommandResult {
    let mut redo = cmd.clone();
    if optional_u64(&redo.params, "story")?.is_none() {
        redo.params["story"] = next_story_id(&session.document).into();
    }
    let path = param_u64(&redo.params, "path")?;
    let story = param_u64(&redo.params, "story")?;
    let mut document = session.document.clone();
    apply_path_text_add(&redo, &mut document)?;
    commit_text_edit(session);
    crate::commands::document_change::commit(session, "text.path_text.add", document, vec![ObjectId(path)],
        json!({ "result_id": path, "story": story }))
}

fn apply_path_text_add(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let path_id = ObjectId(param_u64(&cmd.params, "path")?);
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let object = doc.get_object(path_id).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let curve = crate::outline::text_path_curve(object).ok_or_else(CommandError::invalid_params)?;
    let contours = crate::outline::text_path_contours(object);
    let click = if cmd.params.get("pos").is_some() { Some(param_pos(&cmd.params)?) } else { None };
    let start_pct = path_start_fraction(&contours, click).ok_or_else(CommandError::invalid_params)?;
    let end_pct = if matches!(contours.as_slice(), [(_, true)]) { start_pct + 1.0 } else { 1.0 };
    if doc.stories.iter().any(|s| s.id == story_id) {
        return Err(conflict_error());
    }
    doc.stories.push(Story {
        id: story_id,
        text: String::new(),
        ranges: vec![json!({ "start": 0, "end": 0, "size_pt": DEFAULT_SIZE_PT })],
        specials: Vec::new(),
        paras: Vec::new(),
    });
    if let Some(object) = doc.find_object_mut(path_id.0) {
        object.path_text_curve = Some(curve);
        object.shape = None;
        object.kind = ObjectKind::PathText(PathTextData {
            path: path_id,
            story: story_id,
            flows: vec![crate::object::PathFlow { start_pct, end_pct, baseline_pct: 0.0, ..Default::default() }],
            source_paint: None,
        });
    }
    Ok(())
}

fn path_start_fraction(contours: &[(Vec<[f32; 2]>, bool)], click: Option<[f32; 2]>) -> Option<f32> {
    let mut length = 0.0;
    let mut closest = (f32::INFINITY, 0.0);
    for (points, closed) in contours {
        let following = points.iter().skip(1).chain(points.first().filter(|_| *closed));
        for (a, b) in points.iter().zip(following) {
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let squared = dx * dx + dy * dy;
            if !squared.is_finite() || squared <= 0.0 { continue; }
            let segment = squared.sqrt();
            if let Some(p) = click {
                let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / squared).clamp(0.0, 1.0);
                let distance = (p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dy * t);
                if distance < closest.0 { closest = (distance, length + segment * t); }
            }
            length += segment;
        }
    }
    (length.is_finite() && length > 0.0).then(|| closest.1 / length)
}

fn run_doc_query(cmd: &Command, session: &mut Session) -> CommandResult {
    let id = ObjectId(param_u64(&cmd.params, "id")?);
    let doc = &session.document;
    let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    let payload = match &object.kind {
        ObjectKind::PathText(data) => {
            let story = find_story(doc, data.story).ok();
            json!({
                "type": "path_text",
                "path_id": data.path.0,
                "story_id": data.story.0,
                "flows": data.flows,
                "source_paint": data.source_paint,
                "text_length": story.map(|s| scalar_len(&s.text)).unwrap_or(0),
                "bounds": [object.pos[0], object.pos[1], object.pos[0] + object.size[0], object.pos[1] + object.size[1]],
                "overset": crate::query::overset_json(doc, id).0,
                "overset_known": crate::query::overset_json(doc, id).1
            })
        }
        ObjectKind::ArtText { story } => {
            let found = find_story(doc, *story)?;
            json!({
                "type": "art_text",
                "vertical": object.is_vertical_art_text(),
                "story_id": story.0,
                "text_length": scalar_len(&found.text),
                "pos": object.pos,
                "size": object.size,
                "overset": crate::query::overset_json(doc, id).0,
                "overset_known": crate::query::overset_json(doc, id).1
            })
        }
        ObjectKind::FrameText { story, .. } => {
            let found = find_story(doc, *story)?;
            json!({
                "type": "frame_text",
                "frame_props": serde_json::to_value(object.frame_props()).unwrap_or(Value::Null),
                "story_id": story.0,
                "text_length": scalar_len(&found.text),
                "pos": object.pos,
                "size": object.size,
                "overset": crate::query::overset_json(doc, id).0,
                "overset_known": crate::query::overset_json(doc, id).1
            })
        }
        ObjectKind::Curve(curve) => json!({
            "type": "curve",
            "pos": object.pos,
            "size": object.size,
            "points": curve.contour.points.len(),
            "holes": curve.holes.len(),
            "closed": curve.contour.closed,
            "source_object": curve.source_object
        }),
        ObjectKind::Shape => json!({ "type": "shape", "pos": object.pos, "size": object.size }),
        ObjectKind::Group => json!({ "type": "group" }),
        ObjectKind::Artboard => json!({ "type": "artboard", "pos": object.pos, "size": object.size }),
        ObjectKind::Image => json!({ "type": "image", "pos": object.pos, "size": object.size }),
        ObjectKind::PictureFrame => json!({ "type": "picture_frame", "pos": object.pos, "size": object.size }),
        ObjectKind::Compound(spec) => json!({ "type": "compound", "op": spec.op, "pos": object.pos, "size": object.size }),
        ObjectKind::Table(_) => json!({ "type": "table" }),
        ObjectKind::Text(content) => json!({ "type": "text", "text_length": scalar_len(content) }),
        ObjectKind::Dimension(d) => json!({ "type": "dimension", "pos": object.pos, "size": object.size, "length": d.length, "units": d.units }),
    };
    Ok(payload)
}

fn collect_font_usages(doc: &Document, include_characters: bool) -> std::collections::BTreeMap<(&str, &str), std::collections::BTreeSet<char>> {
    let mut usages = std::collections::BTreeMap::new();
    for story in doc.stories.iter().chain(doc.symbols.stories()) {
        let characters: Vec<char> = if include_characters { story.text.chars().collect() } else { Vec::new() };
        for range in &story.ranges {
            let family = range["family"].as_str().unwrap_or("");
            if family.is_empty() || range.get("frame").is_some() { continue; }
            let style = range["style"].as_str().unwrap_or("");
            let used: &mut std::collections::BTreeSet<char> = usages.entry((family, style)).or_default();
            let start = range["start"].as_u64().unwrap_or(0) as usize;
            let end = range["end"].as_u64().unwrap_or(0) as usize;
            if let Some(span) = characters.get(start..end.min(characters.len())) {
                used.extend(span.iter().copied());
            }
        }
    }
    usages
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FontFaceName {
    pub family: String,
    pub style: String,
}

impl FontFaceName {
    pub fn matches(&self, family: &str, style: &str) -> bool {
        self.family.eq_ignore_ascii_case(family) && self.style.eq_ignore_ascii_case(style)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FontSubstitution {
    pub source: FontFaceName,
    pub substitute: FontFaceName,
}

type FontAvailability = fn(&str, &str) -> bool;
static FONT_AVAILABILITY: std::sync::OnceLock<FontAvailability> = std::sync::OnceLock::new();
type FontCoverage = fn(&str, &str, &std::collections::BTreeSet<char>) -> Vec<char>;
static FONT_COVERAGE: std::sync::OnceLock<FontCoverage> = std::sync::OnceLock::new();

pub fn register_font_availability(check: FontAvailability) {
    let _ = FONT_AVAILABILITY.set(check);
}

pub fn register_font_coverage(check: FontCoverage) {
    let _ = FONT_COVERAGE.set(check);
}

fn font_available(family: &str, style: &str) -> Option<bool> {
    FONT_AVAILABILITY.get().map(|check| check(family, style))
}

fn run_font_usage_query(_cmd: &Command, session: &mut Session) -> CommandResult {
    let doc = &session.document;
    let coverage = FONT_COVERAGE.get();
    let items: Vec<Value> = collect_font_usages(doc, coverage.is_some()).into_iter().map(|((family, style), chars)| {
        let substitute = doc.font_substitutions.iter().find(|entry| entry.source.matches(family, style));
        let missing = font_available(family, style) == Some(false);
        let unsupported = coverage.filter(|_| !missing).map_or_else(Vec::new, |check| check(family, style, &chars));
        let substitute_unsupported = coverage.filter(|_| missing).zip(substitute).map_or_else(Vec::new, |(check, entry)| {
            check(&entry.substitute.family, &entry.substitute.style, &chars)
        });
        json!({
            "family": family,
            "style": style,
            "missing": missing,
            "substitute": substitute.map(|entry| &entry.substitute),
            "unsupported_chars": unsupported,
            "substitute_unsupported_chars": substitute_unsupported,
        })
    }).collect();
    Ok(json!({ "usages": items }))
}

fn run_font_substitution(cmd: &Command, session: &mut Session) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let face = |value: Value| -> Result<FontFaceName, CommandError> {
        let face: FontFaceName = serde_json::from_value(value).map_err(|_| CommandError::invalid_params())?;
        let family = face.family.trim();
        if family.is_empty() {
            return Err(CommandError::invalid_params());
        }
        Ok(FontFaceName { family: family.to_owned(), style: face.style.trim().to_owned() })
    };
    let source = face(cmd.params.get("source").cloned().ok_or_else(CommandError::invalid_params)?)?;
    let substitute = match cmd.params.get("substitute") {
        Some(Value::Null) => None,
        Some(value) => Some(face(value.clone())?),
        None => return Err(CommandError::invalid_params()),
    };
    if let Some(target) = &substitute {
        if font_available(&source.family, &source.style) == Some(true)
            || font_available(&target.family, &target.style) != Some(true)
            || source.matches(&target.family, &target.style)
        {
            return Err(CommandError::invalid_params());
        }
        if !collect_font_usages(&session.document, false).keys().any(|(family, style)| source.matches(family, style)) {
            return Err(CommandError::not_found());
        }
    }

    let previous = session.document.font_substitutions.iter()
        .position(|entry| entry.source.matches(&source.family, &source.style));
    if previous.map(|index| &session.document.font_substitutions[index].substitute) == substitute.as_ref() {
        return Ok(json!({"doc_version": session.generation}));
    }

    let mut document = session.document.clone();
    match (previous, substitute) {
        (Some(index), Some(substitute)) => document.font_substitutions[index].substitute = substitute,
        (None, Some(substitute)) => document.font_substitutions.push(FontSubstitution { source, substitute }),
        (Some(index), None) => { document.font_substitutions.remove(index); }
        (None, None) => {}
    }
    let selection = session.selection.ids.clone();
    crate::commands::document_change::commit(session, &cmd.name, document, selection, json!({}))
}

fn run_font_replace(cmd: &Command, session: &mut Session) -> CommandResult {
    use crate::text_styles::{AppliedStyle, StyleKind};
    let value = |key| cmd.params[key].as_str().unwrap_or("").trim().to_owned();
    let source = FontFaceName { family: value("from_family"), style: value("from_style") };
    let target = FontFaceName { family: value("to_family"), style: value("to_style") };
    if source.family.is_empty() || target.family.is_empty() {
        return Err(CommandError::invalid_params());
    }

    let mut document = session.document.clone();
    let before = &session.document.text_styles;
    let resolved_face = |kind, name: &str| {
        let props = before.resolve(kind, name).ok()?;
        let property = |key| props.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let face = FontFaceName { family: property("family"), style: property("style") };
        Some(if source.matches(&face.family, &face.style) { target.clone() } else { face })
    };
    for style in &mut document.text_styles.styles {
        let Some(next) = resolved_face(style.kind, &style.name) else { continue };
        let parent = style.based_on.as_deref().and_then(|name| resolved_face(style.kind, name));
        for (key, next, inherited) in [
            ("family", next.family, parent.as_ref().map(|face| face.family.as_str())),
            ("style", next.style, parent.as_ref().map(|face| face.style.as_str())),
        ] {
            let original = style.props.get(key).and_then(Value::as_str);
            if original.or(inherited).unwrap_or("") != next {
                style.props.insert(key.to_string(), Value::String(next));
            }
        }
    }

    let after = &document.text_styles;
    let replace_story = |story: &mut Story| {
        for (kind, ranges) in [(StyleKind::Character, &mut story.ranges), (StyleKind::Paragraph, &mut story.paras)] {
            for range in ranges.iter_mut().filter(|range| range.is_object() && range.get("frame").is_none()) {
                let mut applied = AppliedStyle::read_range(kind, range);
                let old_props = applied.style.as_deref().and_then(|name| before.resolve(kind, name).ok());
                let new_props = applied.style.as_deref().and_then(|name| after.resolve(kind, name).ok());
                let replace = source.matches(range["family"].as_str().unwrap_or(""), range["style"].as_str().unwrap_or(""));
                let mut update_overrides = false;
                for (key, replacement) in [("family", &target.family), ("style", &target.style)] {
                    let old_style = old_props.as_ref().and_then(|props| props.get(key));
                    let new_style = new_props.as_ref().and_then(|props| props.get(key));
                    if replace {
                        range[key] = replacement.clone().into();
                    }
                    if new_props.is_some() && (replace || old_style != new_style) {
                        update_overrides = true;
                        let current = range.get(key).cloned().unwrap_or(Value::Null);
                        if new_style.unwrap_or(&Value::Null) == &current {
                            applied.overrides.remove(key);
                        } else {
                            applied.overrides.insert(key.to_string(), current);
                        }
                    }
                }
                if update_overrides {
                    applied.write_range(kind, range);
                }
            }
        }
    };
    for story in &mut document.stories {
        replace_story(story);
    }
    let changed_stories = document.stories.iter().zip(&session.document.stories)
        .filter_map(|(after, before)| (after != before).then_some(after.id)).collect::<Vec<_>>();
    let mut symbols_changed = false;
    for (_, story) in document.symbols.stories_mut() {
        let before = story.clone();
        replace_story(story);
        symbols_changed |= *story != before;
    }
    if !symbols_changed && changed_stories.is_empty() && after == before {
        return Ok(json!({ "doc_version": session.generation, "changes": [] }));
    }
    for id in changed_stories {
        refresh_art_bounds(&mut document, id)?;
    }
    let selection = session.selection.ids.clone();
    crate::commands::document_change::commit(session, &cmd.name, document, selection, json!({}))
}

fn apply_font_replace_undo(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let snapshot = cmd.params.get("snapshot").filter(|v| v.is_array()).ok_or_else(CommandError::invalid_params)?;
    doc.stories = serde_json::from_value::<Vec<Story>>(snapshot.clone()).map_err(|_| CommandError::invalid_params())?;
    Ok(())
}

fn apply_font_replace(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let from_family = cmd.params.get("from_family").and_then(|v| v.as_str()).unwrap_or("");
    let from_style = cmd.params.get("from_style").and_then(|v| v.as_str()).unwrap_or("");
    let to_family = cmd.params.get("to_family").and_then(|v| v.as_str()).unwrap_or("");
    let to_style = cmd.params.get("to_style").and_then(|v| v.as_str()).unwrap_or("");

    for story in &mut doc.stories {
        for range in &mut story.ranges {
            if !range.is_object() {
                continue;
            }
            let obj = range.as_object_mut().unwrap();
            let current_family = obj.get("family").and_then(|v| v.as_str()).unwrap_or("");
            let current_style = obj.get("style").and_then(|v| v.as_str()).unwrap_or("");
            if current_family == from_family && current_style == from_style {
                obj.insert("family".to_string(), Value::String(to_family.to_string()));
                obj.insert("style".to_string(), Value::String(to_style.to_string()));
            }
        }
    }

    Ok(())
}

fn run_glyph_insert(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let at = param_at(&cmd.params)?;
    let font_id_val = param_u64(&cmd.params, "font")?;
    let gid = param_u64(&cmd.params, "gid")? as u16;

    let previous_text = find_story(&session.document, story_id)?.text.clone();

    let font_id = font_id_val;

    let char_opt = param_glyph_char(&cmd.params).or_else(|| resolve_glyph_char(font_id, gid));

    if let Some(ch) = char_opt {
        let mut redo_cmd = cmd.clone();
        redo_cmd.params["text"] = ch.to_string().into();
        
        apply_insert(&redo_cmd, &mut session.document)?;
        
        let mut undo = Command::new("text.insert.undo");
        undo.params["story"] = story_id.0.into();
        undo.params["text"] = previous_text.into();
        
        push_change(session, "text.glyph.insert", redo_cmd, undo);

        push_recent_glyph(font_id, gid);

        Ok(json!({
            "doc_version": session.generation,
            "story": story_id.0,
            "at": at,
            "encoded": true
        }))
    } else {
        let redo_cmd = cmd.clone();
        apply_glyph_span_insert(&redo_cmd, &mut session.document)?;

        let mut undo = Command::new("text.insert.undo");
        undo.params["story"] = story_id.0.into();
        undo.params["text"] = previous_text.into();
        
        push_change(session, "text.glyph.insert", redo_cmd, undo);

        push_recent_glyph(font_id, gid);

        Ok(json!({
            "doc_version": session.generation,
            "story": story_id.0,
            "at": at,
            "encoded": false
        }))
    }
}

const RECENT_GLYPHS_CAP: usize = 20;

static RECENT_GLYPHS: std::sync::Mutex<Vec<(u64, u16)>> = std::sync::Mutex::new(Vec::new());

fn glyph_char_store() -> &'static std::sync::Mutex<std::collections::HashMap<(u64, u16), char>> {
    static STORE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<(u64, u16), char>>> =
        std::sync::OnceLock::new();
    STORE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn register_glyph_char(font_id: u64, gid: u16, ch: char) {
    if let Ok(mut map) = glyph_char_store().lock() {
        map.insert((font_id, gid), ch);
    }
}

pub fn clear_glyph_chars() {
    if let Ok(mut map) = glyph_char_store().lock() {
        map.clear();
    }
}

pub fn resolve_glyph_char(font_id: u64, gid: u16) -> Option<char> {
    glyph_char_store()
        .lock()
        .ok()
        .and_then(|map| map.get(&(font_id, gid)).copied())
}

pub fn push_recent_glyph(font_id: u64, gid: u16) {
    if let Ok(mut items) = RECENT_GLYPHS.lock() {
        items.retain(|(f, g)| *f != font_id || *g != gid);
        items.insert(0, (font_id, gid));
        if items.len() > RECENT_GLYPHS_CAP {
            items.truncate(RECENT_GLYPHS_CAP);
        }
    }
}

pub fn recent_glyphs() -> Vec<(u64, u16)> {
    RECENT_GLYPHS.lock().map(|items| items.clone()).unwrap_or_default()
}

fn param_glyph_char(params: &Value) -> Option<char> {
    params
        .get("char")
        .and_then(|v| v.as_str())
        .and_then(|s| {
            let mut chars = s.chars();
            let first = chars.next()?;
            if chars.next().is_none() {
                Some(first)
            } else {
                None
            }
        })
}
fn apply_glyph_span_insert(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let at = param_at(&cmd.params)?;
    let font_id_val = param_u64(&cmd.params, "font")?;
    let gid = param_u64(&cmd.params, "gid")? as u16;

    let story = find_story_mut(doc, story_id)?;
    
    let byte_idx = scalar_byte(&story.text, at).ok_or_else(range_error)?;
    
    let replacement_char = '\u{FFFC}'; 
    story.text.insert(byte_idx, replacement_char);

    let new_range = json!({
        "start": at,
        "end": at + 1,
        "font_id": font_id_val,
        "gid": gid,
        "encoded": false
    });

    story.ranges.push(new_range);

    refresh_art_bounds(doc, story_id)
}

fn run_text_style(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let before = find_story(&session.document, story_id)?.clone();
    apply_text_style(cmd, &mut session.document)?;
    let linked = sync_linked_change(session, &before);
    if !push_linked_text_change(session, &cmd.name, &before, linked)? {
        let undo = Command::with_params("text.style.undo", json!({"story": story_id.0, "ranges": before.ranges}));
        push_change(session, "text.style", cmd.clone(), undo);
    }
    Ok(json!({ "doc_version": session.generation }))
}

fn style_override_skip(key: &str) -> bool {
    matches!(key, "start" | "end" | "frame" | "char_style" | "para_style" | "style_overrides")
}

fn apply_text_style(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let (start, end) = param_range(&cmd.params)?;
    let patch = cmd.params.get("patch").cloned().unwrap_or(Value::Null);
    let patch = patch.as_object().cloned().unwrap_or_default();
    let store = doc.text_styles.clone();
    let story = find_story_mut(doc, story_id)?;
    let apply_patch = |run: &mut Value| {
        let style_name = run.get("char_style").and_then(Value::as_str).map(str::to_string);
        let resolved = style_name.as_deref().and_then(|name| store.resolve(crate::text_styles::StyleKind::Character, name).ok()).unwrap_or_default();
        let mut overrides = run.get("style_overrides").and_then(Value::as_object).cloned().unwrap_or_default();
        for (key, value) in &patch {
            if style_override_skip(key) {
                continue;
            }
            if value.is_null() {
                if let Some(map) = run.as_object_mut() {
                    map.remove(key);
                }
                if style_name.is_some() {
                    if resolved.get(key).is_some() {
                        overrides.insert(key.clone(), Value::Null);
                    } else {
                        overrides.remove(key);
                    }
                }
            } else {
                run[key] = value.clone();
                if style_name.is_some() {
                    if resolved.get(key) == Some(value) {
                        overrides.remove(key);
                    } else {
                        overrides.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        if let Some(explicit) = patch.get("style_overrides").and_then(Value::as_object) {
            for (key, value) in explicit {
                overrides.insert(key.clone(), value.clone());
            }
        }
        if style_name.is_some() {
            if overrides.is_empty() {
                if let Some(map) = run.as_object_mut() {
                    map.remove("style_overrides");
                }
            } else {
                run["style_overrides"] = Value::Object(overrides);
            }
        }
        for (key, value) in &patch {
            if key == "start" || key == "end" || key == "style_overrides" {
                continue;
            }
            if key == "char_style" || key == "para_style" {
                if value.is_null() {
                    if let Some(map) = run.as_object_mut() {
                        map.remove(key);
                    }
                } else {
                    run[key] = value.clone();
                }
            }
        }
    };
    if start == 0 && end == 0 && story.text.is_empty() {
        let mut initial = story.ranges.iter().rev().find(|run| is_char_style_run(run)).cloned()
            .unwrap_or_else(|| json!({ "start": 0, "end": 0, "size_pt": DEFAULT_SIZE_PT }));
        apply_patch(&mut initial);
        story.ranges.retain(|run| !is_char_style_run(run));
        story.ranges.push(initial);
        return refresh_art_bounds(doc, story_id);
    }
    let mut new_ranges: Vec<Value> = Vec::new();
    let mut covered: Vec<(usize, usize)> = Vec::new();
    for range in &story.ranges {
        if !range.is_object() {
            continue;
        }
        let r_start = range.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let r_end = range.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        if r_end <= start || r_start >= end || start >= end {
            new_ranges.push(range.clone());
            continue;
        }
        if r_start < start {
            let mut left = range.clone();
            left["end"] = (start as u64).into();
            new_ranges.push(left);
        }
        if r_end > end {
            let mut right = range.clone();
            right["start"] = (end as u64).into();
            new_ranges.push(right);
        }
        let mut mid = range.clone();
        apply_patch(&mut mid);
        let (m_start, m_end) = (r_start.max(start), r_end.min(end));
        mid["start"] = (m_start as u64).into();
        mid["end"] = (m_end as u64).into();
        covered.push((m_start, m_end));
        new_ranges.push(mid);
    }
    if start < end && patch.iter().any(|(key, value)| key != "start" && key != "end" && !value.is_null()) {
        covered.sort();
        let mut cursor = start;
        let mut gaps: Vec<(usize, usize)> = Vec::new();
        for (c_start, c_end) in covered {
            if c_start > cursor {
                gaps.push((cursor, c_start));
            }
            cursor = cursor.max(c_end);
        }
        if cursor < end {
            gaps.push((cursor, end));
        }
        for (g_start, g_end) in gaps {
            let mut run = json!({ "start": g_start, "end": g_end });
            apply_patch(&mut run);
            new_ranges.push(run);
        }
    }
    new_ranges.sort_by_key(|run| run.get("start").and_then(|v| v.as_u64()).unwrap_or(0));
    story.ranges = new_ranges;
    refresh_art_bounds(doc, story_id)
}

enum StyleMix<T> {
    Empty,
    Value(T),
    Mixed,
}

impl<T: PartialEq> StyleMix<T> {
    fn ingest(&mut self, value: T) {
        match self {
            StyleMix::Empty => *self = StyleMix::Value(value),
            StyleMix::Value(seen) if *seen == value => {}
            StyleMix::Value(_) | StyleMix::Mixed => *self = StyleMix::Mixed,
        }
    }

    fn into_json(self, to_value: impl FnOnce(T) -> Value) -> Value {
        match self {
            StyleMix::Value(value) => to_value(value),
            StyleMix::Empty | StyleMix::Mixed => json!("Mixed"),
        }
    }
}

fn is_char_style_run(range: &Value) -> bool {
    range.is_object() && range.get("frame").is_none()
}

fn run_text_style_query(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let (start, end) = param_range(&cmd.params)?;
    let story = find_story(&session.document, story_id)?;

    let mut family = StyleMix::Empty;
    let mut style = StyleMix::Empty;
    let mut size_pt = StyleMix::Empty;
    let mut weight = StyleMix::Empty;
    let mut italic = StyleMix::Empty;
    let mut tracking = StyleMix::Empty;
    let mut fill = StyleMix::Empty;
    let mut align = StyleMix::Empty;
    let mut leading = StyleMix::Empty;

    for range in &story.ranges {
        if !is_char_style_run(range) {
            continue;
        }
        let r_start = range.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let r_end = range.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let initial = story.text.is_empty() && start == 0 && end == 0 && r_start == 0 && r_end == 0;
        if !initial && (r_end <= start || r_start >= end) {
            continue;
        }
        family.ingest(range.get("family").and_then(Value::as_str).unwrap_or("").to_string());
        style.ingest(range.get("style").and_then(Value::as_str).unwrap_or("").to_string());
        size_pt.ingest(range.get("size_pt").and_then(Value::as_f64));
        weight.ingest(range.get("weight").and_then(Value::as_u64));
        italic.ingest(range.get("italic").and_then(Value::as_bool).unwrap_or(false));
        tracking.ingest(range.get("tracking").and_then(Value::as_f64).unwrap_or(0.0).to_bits());
        fill.ingest(crate::paint::range_fill(range));
    }

    for run in &story.paras {
        if !run.is_object() {
            continue;
        }
        let (r_start, r_end) = para_bounds(run);
        if r_end <= start || r_start >= end {
            continue;
        }
        align.ingest(run.get("align").and_then(Value::as_str).unwrap_or("Left").to_string());
        leading.ingest(run.get("leading").and_then(Value::as_f64));
    }

    Ok(json!({
        "family": family.into_json(Value::from),
        "style": style.into_json(Value::from),
        "size_pt": size_pt.into_json(|value| value.map(Value::from).unwrap_or(json!("Mixed"))),
        "weight": weight.into_json(|value| value.map(Value::from).unwrap_or(json!("Mixed"))),
        "italic": italic.into_json(Value::from),
        "tracking": tracking.into_json(|bits| json!(f64::from_bits(bits))),
        "fill": fill.into_json(|paint| paint.map(|value| crate::paint::paint_to_json_value(&value)).unwrap_or(Value::Null)),
        "align": align.into_json(Value::from),
        "leading": leading.into_json(|value| value.map(Value::from).unwrap_or(json!("Mixed"))),
    }))
}

fn run_special_insert(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let at = param_at(&cmd.params)?;
    let what = cmd.params.get("what").and_then(|v| v.as_str()).unwrap_or("");

    if what == "column_break" || what == "frame_break" || what == "page_break" || what == "odd_page_break" || what == "even_page_break" {
        let previous_specials = find_story(&session.document, story_id)?.specials.clone();
        
        apply_special_insert(cmd, &mut session.document)?;
        
        let mut undo = Command::new("text.special.undo");
        undo.params["story"] = story_id.0.into();
        undo.params["specials"] = serde_json::to_value(&previous_specials).unwrap_or(Value::Null);
        
        push_change(session, "text.special.insert", cmd.clone(), undo);
        
        return Ok(json!({
            "doc_version": session.generation,
            "story": story_id.0,
            "at": at,
            "special": what
        }));
    }

    if what == "indent_to_here" {
        let previous_specials = find_story(&session.document, story_id)?.specials.clone();
        
        apply_special_insert(cmd, &mut session.document)?;
        
        let mut undo = Command::new("text.special.undo");
        undo.params["story"] = story_id.0.into();
        undo.params["specials"] = serde_json::to_value(&previous_specials).unwrap_or(Value::Null);
        
        push_change(session, "text.special.insert", cmd.clone(), undo);
        
        return Ok(json!({
            "doc_version": session.generation,
            "story": story_id.0,
            "at": at,
            "special": what
        }));
    }

    if what == "right_indent_tab" {
        let previous_specials = find_story(&session.document, story_id)?.specials.clone();
        
        apply_special_insert(cmd, &mut session.document)?;
        
        let mut undo = Command::new("text.special.undo");
        undo.params["story"] = story_id.0.into();
        undo.params["specials"] = serde_json::to_value(&previous_specials).unwrap_or(Value::Null);
        
        push_change(session, "text.special.insert", cmd.clone(), undo);
        
        return Ok(json!({
            "doc_version": session.generation,
            "story": story_id.0,
            "at": at,
            "special": what
        }));
    }

    let ch = resolve_special_char(what)
        .ok_or_else(|| CommandError::validation("UNKNOWN_SPECIAL"))?;
    
    let previous = find_story(&session.document, story_id)?.text.clone();
    
    let mut redo_cmd = cmd.clone();
    redo_cmd.params["text"] = ch.to_string().into();
    
    apply_insert(&redo_cmd, &mut session.document)?;
    
    let mut undo = Command::new("text.insert.undo");
    undo.params["story"] = story_id.0.into();
    undo.params["text"] = previous.into();
    
    push_change(session, "text.special.insert", redo_cmd, undo);
    
    Ok(json!({
        "doc_version": session.generation,
        "story": story_id.0,
        "at": at,
        "char": ch.to_string()
    }))
}

fn apply_special_undo(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let specials: Vec<(usize, Special)> = match cmd.params.get("specials") {
        Some(Value::Null) | None => Vec::new(),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?,
    };
    find_story_mut(doc, story_id)?.specials = specials;
    refresh_art_bounds(doc, story_id)
}

fn apply_special_redo(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    if cmd.params.get("text").and_then(|v| v.as_str()).is_some() {
        apply_insert(cmd, doc)
    } else {
        apply_special_insert(cmd, doc)
    }
}

fn apply_special_insert(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let at = param_at(&cmd.params)?;
    let what = cmd.params.get("what").and_then(|v| v.as_str()).unwrap_or("");
    
    let story = find_story_mut(doc, story_id)?;
    
    if !story.specials.iter().any(|(pos, _)| *pos == at) {
        match what {
            "column_break" => {
                story.specials.push((at, Special::Break(BreakKind::Column)));
            }
            "frame_break" => {
                story.specials.push((at, Special::Break(BreakKind::Frame)));
            }
            "page_break" => {
                story.specials.push((at, Special::Break(BreakKind::Page)));
            }
            "odd_page_break" => {
                story.specials.push((at, Special::Break(BreakKind::OddPage)));
            }
            "even_page_break" => {
                story.specials.push((at, Special::Break(BreakKind::EvenPage)));
            }
            "indent_to_here" => {
                story.specials.push((at, Special::IndentToHere));
            }
            "right_indent_tab" => {
                story.specials.push((at, Special::RightIndentTab));
            }
            _ => return Err(CommandError::validation("UNKNOWN_SPECIAL")),
        }
    }
    
    refresh_art_bounds(doc, story_id)
}

fn run_link_set(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let (start, end) = param_range(&cmd.params)?;
    let url = cmd.params.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if url.is_empty() {
        return Err(CommandError::invalid_params());
    }

    let previous_ranges = find_story(&session.document, story_id)?.ranges.clone();

    apply_link_set(cmd, &mut session.document)?;

    let mut undo = Command::new("text.style.undo");
    undo.params["story"] = story_id.0.into();
    undo.params["ranges"] = serde_json::to_value(&previous_ranges).unwrap_or(Value::Null);

    push_change(session, "text.link.set", cmd.clone(), undo);

    Ok(json!({ "doc_version": session.generation }))
}

fn apply_link_set(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let (start, end) = param_range(&cmd.params)?;
    let url = cmd.params.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if url.is_empty() {
        return Err(CommandError::invalid_params());
    }

    let story = find_story_mut(doc, story_id)?;

    let mut new_ranges: Vec<Value> = Vec::new();

    for range in &story.ranges {
        if !range.is_object() {
            continue;
        }

        let r_start = range.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let r_end = range.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        if r_end <= start || r_start >= end {
            new_ranges.push(range.clone());
            continue;
        }

        if r_start < start {
            let mut left = range.clone();
            left["end"] = (start as u64).into();
            new_ranges.push(left);
        }

        if r_end > end {
            let mut right = range.clone();
            right["start"] = (end as u64).into();
            new_ranges.push(right);
        }

        let mut patched = range.clone();
        patched["link"] = Value::String(url.clone());

        let mut mid = patched;
        mid["start"] = (r_start.max(start) as u64).into();
        mid["end"] = (r_end.min(end) as u64).into();
        new_ranges.push(mid);
    }

    story.ranges = new_ranges;

    refresh_art_bounds(doc, story_id)
}

fn resolve_special_char(what: &str) -> Option<char> {
    match what {
        "em_dash" => Some('\u{2014}'),
        "en_dash" => Some('\u{2013}'),
        "figure_dash" => Some('\u{2012}'),
        "soft_hyphen" => Some('\u{00AD}'),
        "nb_hyphen" => Some('\u{2011}'),
        "zw_space" => Some('\u{200B}'),
        "zwnj" => Some('\u{200C}'),
        "em_space" => Some('\u{2003}'),
        "en_space" => Some('\u{2002}'),
        "thin_space" => Some('\u{2009}'),
        "hair_space" => Some('\u{200A}'),
        "figure_space" => Some('\u{2007}'),
        "punct_space" => Some('\u{2008}'),
        "line_break" => Some('\u{2028}'),
        "para_break" => Some('\u{2029}'),
        "nbsp" => Some('\u{00A0}'),
        _ => None,
    }
}

fn param_para_range(params: &Value, text_len: usize) -> Result<(usize, usize), CommandError> {
    let range = params.get("range").ok_or_else(CommandError::invalid_params)?;
    if range.as_str() == Some("all") {
        return Ok((0, text_len));
    }
    if let Some(arr) = range.as_array() {
        if arr.len() == 2 {
            let start = as_index(&arr[0])?;
            let end = as_index(&arr[1])?;
            if start > end {
                return Err(CommandError::invalid_params());
            }
            return Ok((start, end));
        }
        return Err(CommandError::invalid_params());
    }
    if range.is_object() {
        let start = range
            .get("start")
            .ok_or_else(CommandError::invalid_params)
            .and_then(as_index)?;
        let end = range
            .get("end")
            .ok_or_else(CommandError::invalid_params)
            .and_then(as_index)?;
        if start > end {
            return Err(CommandError::invalid_params());
        }
        return Ok((start, end));
    }
    Err(CommandError::invalid_params())
}

fn as_index(value: &Value) -> Result<usize, CommandError> {
    if let Some(v) = value.as_u64() {
        return Ok(v as usize);
    }
    if let Some(v) = value.as_i64() {
        if v >= 0 {
            return Ok(v as usize);
        }
    }
    if let Some(v) = value.as_f64() {
        if v.is_finite() && v >= 0.0 && v.fract() == 0.0 {
            return Ok(v as usize);
        }
    }
    Err(CommandError::invalid_params())
}

fn as_number(value: &Value) -> Option<f64> {
    value.as_f64()
}

fn validate_para_patch(patch: &Value) -> Result<(), CommandError> {
    let obj = patch.as_object().ok_or_else(CommandError::invalid_params)?;
    if let Some(decorations) = obj.get("paragraph_decorations") {
        let decorations = decorations.as_array().ok_or_else(CommandError::invalid_params)?;
        for item in decorations {
            let item = item.as_object().ok_or_else(CommandError::invalid_params)?;
            for (key, value) in item {
                let valid = match key.as_str() {
                    "sides" => value.as_array().is_some_and(|sides| sides.len() == 4 && sides.iter().all(Value::is_boolean)),
                    "fill" if value.is_null() => true,
                    "fill" | "stroke" => serde_json::from_value::<crate::paint::Colour>(value.clone()).is_ok(),
                    "width" | "padding" => as_number(value).is_some_and(|n| (0.0..=f32::MAX as f64).contains(&n)),
                    _ => false,
                };
                if !valid { return Err(CommandError::invalid_params()); }
            }
        }
    }
    if let Some(initial) = obj.get("initial_words") {
        let initial = initial.as_object().ok_or_else(CommandError::invalid_params)?;
        for (key, value) in initial {
            let valid = match key.as_str() {
                "on" => value.is_boolean(),
                "max_words" => value.as_u64().is_some_and(|count| (1..=999).contains(&count)),
                "end_chars" => value.as_str().is_some_and(|s| !s.chars().any(char::is_control)),
                "style" => value.is_null() || value.as_str().is_some_and(|s| !s.trim().is_empty()),
                _ => false,
            };
            if !valid {
                return Err(CommandError::invalid_params());
            }
        }
    }
    if let Some(spacing) = obj.get("justification") {
        let spacing = spacing.as_object().ok_or_else(CommandError::invalid_params)?;
        for (key, value) in spacing {
            let valid = match key.as_str() {
                "enabled" => value.is_boolean(),
                "word_min" | "word_ideal" | "word_max" => as_number(value).is_some_and(|n| (1.0..=1000.0).contains(&n)),
                "letter_min" | "letter_ideal" | "letter_max" => as_number(value).is_some_and(|n| (-100.0..=100.0).contains(&n)),
                _ => false,
            };
            if !valid {
                return Err(CommandError::invalid_params());
            }
        }
        for [min, ideal, max] in [
            ["word_min", "word_ideal", "word_max"],
            ["letter_min", "letter_ideal", "letter_max"],
        ] {
            let values = [min, ideal, max].into_iter().filter_map(|key| spacing.get(key).and_then(as_number)).collect::<Vec<_>>();
            if values.windows(2).any(|range| range[0] > range[1]) {
                return Err(CommandError::invalid_params());
            }
        }
    }
    if obj.get("baseline_grid").is_some_and(|value| !value.is_boolean()) {
        return Err(CommandError::invalid_params());
    }
    if let Some(v) = obj.get("space_before") {
        let n = as_number(v).ok_or_else(CommandError::invalid_params)?;
        if n < 0.0 {
            return Err(CommandError::invalid_params());
        }
    }
    if let Some(v) = obj.get("space_after") {
        let n = as_number(v).ok_or_else(CommandError::invalid_params)?;
        if n < 0.0 {
            return Err(CommandError::invalid_params());
        }
    }
    if let Some(v) = obj.get("leading") {
        let n = as_number(v).ok_or_else(CommandError::invalid_params)?;
        if n <= 0.0 {
            return Err(CommandError::invalid_params());
        }
    }
    for key in ["keep_together", "keep_with_previous", "prevent_orphans", "prevent_widows"] {
        if obj.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(CommandError::invalid_params());
        }
    }
    if obj.get("flow_start").is_some_and(|value| !matches!(
        value.as_str(), Some("Anywhere" | "NextColumn" | "NextFrame" | "NextPage" | "NextOddPage" | "NextEvenPage")
    )) {
        return Err(CommandError::invalid_params());
    }
    if let Some(v) = obj.get("keep_with_next") {
        let n = as_number(v).ok_or_else(CommandError::invalid_params)?;
        if n < 0.0 || n > 9.0 || n.fract() != 0.0 {
            return Err(CommandError::invalid_params());
        }
    }
    if let Some(tabs) = obj.get("tabs") {
        let tabs = tabs.as_array().ok_or_else(CommandError::invalid_params)?;
        let mut prev: Option<f64> = None;
        for tab in tabs {
            let pos = tab
                .get("pos")
                .and_then(as_number)
                .ok_or_else(CommandError::invalid_params)?;
            if !pos.is_finite() || pos < 0.0 {
                return Err(CommandError::invalid_params());
            }
            if let Some(p) = prev {
                if pos < p {
                    return Err(CommandError::invalid_params());
                }
            }
            prev = Some(pos);
            if let Some(leader) = tab.get("leader") {
                if leader_is_control(leader) {
                    return Err(CommandError::invalid_params());
                }
            }
        }
    }
    if let Some(list) = obj.get("list") {
        if let Some(level) = list.get("level") {
            let n = as_number(level).ok_or_else(CommandError::invalid_params)?;
            if n < 1.0 || n > 9.0 || n.fract() != 0.0 {
                return Err(CommandError::invalid_params());
            }
        }
    }
    Ok(())
}

fn leader_is_control(leader: &Value) -> bool {
    let ch = if let Some(s) = leader.as_str() {
        s.chars().next()
    } else if let Some(s) = leader.get("Character").and_then(|v| v.as_str()) {
        s.chars().next()
    } else {
        None
    };
    matches!(ch, Some('\t') | Some('\n') | Some('\r'))
}

fn para_bounds(run: &Value) -> (usize, usize) {
    let start = run.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let end = run.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    (start, end)
}

fn empty_final_paragraph(text: &str) -> Option<usize> {
    if text.is_empty() || text.ends_with(crate::commands::active_story::is_para_break) {
        Some(scalar_len(text))
    } else {
        None
    }
}

fn merge_patch(run: &Value, patch: &Value, start: usize, end: usize) -> Value {
    let mut out = run.clone();
    if let Some(obj) = patch.as_object() {
        for (key, value) in obj {
            if matches!(key.as_str(), "justification" | "initial_words")
                && let Some(existing) = out[key].as_object_mut()
                && let Some(changes) = value.as_object()
            {
                existing.extend(changes.clone());
            } else if key != "start" && key != "end" {
                out[key] = value.clone();
            }
        }
    }
    out["start"] = (start as u64).into();
    out["end"] = (end as u64).into();
    out
}

fn new_para_run(patch: &Value, start: usize, end: usize) -> Value {
    merge_patch(&json!({}), patch, start, end)
}

fn apply_para_patch_to_runs(runs: &[Value], start: usize, end: usize, patch: &Value) -> Vec<Value> {
    if start == end {
        return runs.to_vec();
    }
    let mut out: Vec<Value> = Vec::new();
    let mut covered: Vec<(usize, usize)> = Vec::new();
    for run in runs {
        if !run.is_object() {
            continue;
        }
        let (r_start, r_end) = para_bounds(run);
        if r_end <= start || r_start >= end {
            out.push(run.clone());
            continue;
        }
        if r_start < start {
            let mut left = run.clone();
            left["end"] = (start as u64).into();
            out.push(left);
        }
        let mid_start = r_start.max(start);
        let mid_end = r_end.min(end);
        if mid_start < mid_end {
            out.push(merge_patch(run, patch, mid_start, mid_end));
            covered.push((mid_start, mid_end));
        }
        if r_end > end {
            let mut right = run.clone();
            right["start"] = (end as u64).into();
            out.push(right);
        }
    }
    covered.sort_by_key(|item| item.0);
    let mut cursor = start;
    for (c_start, c_end) in covered {
        if c_start > cursor {
            out.push(new_para_run(patch, cursor, c_start));
        }
        if c_end > cursor {
            cursor = c_end;
        }
    }
    if cursor < end {
        out.push(new_para_run(patch, cursor, end));
    }
    out
}

fn shift_paras_insert(paras: &mut [Value], at: usize, count: usize, starts_existing_paragraph: bool) {
    if count == 0 {
        return;
    }
    let starts_empty_paragraph = paras.iter().any(|run| run.is_object() && para_bounds(run) == (at, at));
    let use_following_style = starts_existing_paragraph || starts_empty_paragraph;
    for run in paras.iter_mut().filter(|run| run.is_object()) {
        let (start, end) = para_bounds(run);
        if (start, end) == (at, at) {
            run["end"] = ((end + count) as u64).into();
            continue;
        }
        if start > at || (start == at && !use_following_style) {
            run["start"] = ((start + count) as u64).into();
        }
        if end > at || (end == at && !use_following_style) {
            run["end"] = ((end + count) as u64).into();
        }
    }
}

fn is_list_kind(kind: &Value) -> bool {
    kind.get("Bullet").is_some() || kind.get("Number").is_some()
}

fn continue_list_paragraphs(story: &mut crate::document::Story, at: usize, text: &str, count: usize) {
    let Some(first_break) = text.chars().position(crate::commands::active_story::is_para_break) else {
        return;
    };
    let next_start = at + first_break + 1;
    let Some(index) = story.paras.iter().rposition(|run| {
        let (start, end) = para_bounds(run);
        start < next_start && next_start <= end
    }) else {
        return;
    };
    let run = &mut story.paras[index];
    if !is_list_kind(&run["list"]["kind"]) {
        return;
    }
    let (_, end) = para_bounds(run);
    let mut continuation = run.clone();
    let Some(list) = continuation["list"].as_object_mut() else { return; };
    list.remove("restart");
    list.remove("start_at");
    if continuation != *run && next_start < end {
        run["end"] = (next_start as u64).into();
        continuation["start"] = (next_start as u64).into();
        story.paras.insert(index + 1, continuation.clone());
    }
    if at + count == end && empty_final_paragraph(&story.text) == Some(end)
        && !story.paras.iter().any(|run| para_bounds(run) == (end, end))
    {
        continuation["start"] = (end as u64).into();
        continuation["end"] = (end as u64).into();
        story.paras.push(continuation);
    }
}

fn map_deleted(pos: usize, start: usize, end: usize) -> usize {
    if pos <= start {
        pos
    } else if pos >= end {
        pos - (end - start)
    } else {
        start
    }
}

fn shift_runs_delete(runs: &mut Vec<Value>, start: usize, end: usize, empty_paragraph: Option<usize>) {
    if end <= start {
        return;
    }
    let mut next: Vec<Value> = Vec::new();
    for run in runs.iter() {
        if !run.is_object() {
            continue;
        }
        let (r_start, r_end) = para_bounds(run);
        let n_start = map_deleted(r_start, start, end);
        let n_end = map_deleted(r_end, start, end);
        let keeps_empty_style = empty_paragraph == Some(n_start) &&
            (r_start == r_end || (r_start, r_end) == (start, end));
        if n_start >= n_end && !keeps_empty_style {
            continue;
        }
        let mut kept = run.clone();
        kept["start"] = (n_start as u64).into();
        kept["end"] = (n_end as u64).into();
        next.push(kept);
    }
    *runs = next;
}

fn shift_ranges_insert(ranges: &mut [Value], at: usize, count: usize) {
    if count == 0 {
        return;
    }
    for run in ranges.iter_mut() {
        if !run.is_object() {
            continue;
        }
        let (start, end) = para_bounds(run);
        if start > at || (start == at && at > 0) {
            run["start"] = ((start + count) as u64).into();
        }
        if end >= at {
            run["end"] = ((end + count) as u64).into();
        }
    }
}

fn shift_specials_insert(specials: &mut [(usize, crate::document::Special)], at: usize, count: usize) {
    for (pos, _) in specials.iter_mut() {
        if *pos >= at {
            *pos += count;
        }
    }
}

fn shift_specials_delete(specials: &mut Vec<(usize, crate::document::Special)>, start: usize, end: usize) {
    if end <= start {
        return;
    }
    specials.retain(|(pos, _)| *pos < start || *pos >= end);
    for (pos, _) in specials.iter_mut() {
        if *pos >= end {
            *pos -= end - start;
        }
    }
}

fn apply_text_style_undo(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let ranges = cmd.params.get("ranges").and_then(Value::as_array).cloned().ok_or_else(CommandError::invalid_params)?;
    find_story_mut(doc, story_id)?.ranges = ranges;
    refresh_art_bounds(doc, story_id)
}

/// Read the same paragraph style used by text.para.query without executing a command.
pub fn query_para_style(runs: &[Value], start: usize, end: usize) -> Value {
    let mut merged = json!({});
    for run in runs {
        if !run.is_object() {
            continue;
        }
        let (r_start, r_end) = para_bounds(run);
        let matches_empty = start == end && (r_start, r_end) == (start, end);
        if !matches_empty && (r_end <= start || r_start >= end) {
            continue;
        }
        if let Some(obj) = run.as_object() {
            for (key, value) in obj {
                if key != "start" && key != "end" {
                    merged[key] = value.clone();
                }
            }
        }
    }
    merged
}

fn run_para_set(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let before = find_story(&session.document, story_id)?.clone();
    apply_para_set(cmd, &mut session.document)?;
    if find_story(&session.document, story_id)?.paras == before.paras {
        return Ok(json!({ "doc_version": session.generation }));
    }
    let linked = sync_linked_change(session, &before);
    if !push_linked_text_change(session, &cmd.name, &before, linked)? {
        let undo = Command::with_params("text.para.set.undo", json!({"story": story_id.0, "paras": before.paras}));
        push_change(session, "text.para.set", cmd.clone(), undo);
    }
    Ok(json!({ "doc_version": session.generation }))
}

fn run_para_query(cmd: &Command, session: &mut Session) -> CommandResult {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let story = find_story(&session.document, story_id)?;
    let (start, end) = param_para_range(&cmd.params, scalar_len(&story.text))?;
    let style = query_para_style(&story.paras, start, end);
    Ok(json!({
        "style": style,
        "doc_version": session.generation
    }))
}

fn apply_para_set(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let patch = cmd
        .params
        .get("patch")
        .cloned()
        .ok_or_else(CommandError::invalid_params)?;
    validate_para_patch(&patch)?;
    let story = find_story_mut(doc, story_id)?;
    let (start, end) = param_para_range(&cmd.params, scalar_len(&story.text))?;
    if start == end && empty_final_paragraph(&story.text) == Some(start) {
        if let Some(run) = story.paras.iter_mut().rev().find(|run| run.is_object() && para_bounds(run) == (start, end)) {
            *run = merge_patch(run, &patch, start, end);
        } else {
            story.paras.push(new_para_run(&patch, start, end));
        }
    } else {
        story.paras = apply_para_patch_to_runs(&story.paras, start, end, &patch);
    }
    if patch.get("initial_words").is_some() {
        refresh_art_bounds(doc, story_id)?;
    }
    Ok(())
}

fn apply_para_set_undo(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    let paras = cmd
        .params
        .get("paras")
        .cloned()
        .ok_or_else(CommandError::invalid_params)?;
    let paras: Vec<Value> =
        serde_json::from_value(paras).map_err(|_| CommandError::invalid_params())?;
    let story = find_story_mut(doc, story_id)?;
    let initial_words = story.paras.iter().chain(&paras).any(|run| run.get("initial_words").is_some());
    story.paras = paras;
    if initial_words {
        refresh_art_bounds(doc, story_id)?;
    }
    Ok(())
}

pub fn sync_frame_shape_path(doc:&mut Document,id:ObjectId)->Result<(),CommandError> {crate::commands::frame_text::sync_frame_shape_path(doc,id)}
