use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::command::Session;
use crate::document::{Document, Story, StoryId};
use crate::object::ObjectId;

pub const PIN_INLINE: &str = "pin.inline";
pub const PIN_FLOAT: &str = "pin.float";
pub const PIN_UNPIN: &str = "pin.unpin";
pub const PIN_COMMANDS: [&str; 3] = [PIN_INLINE, PIN_FLOAT, PIN_UNPIN];
pub const PIN_GLYPH: char = '\u{FFFC}';

const KIND_INLINE: &str = "inline";
const KIND_FLOAT: &str = "float";
const DEFAULT_SIZE_PT: f64 = 12.0;
const FIXTURE_WIDTH_RATIO: f64 = 0.5;
const BASELINE_RATIO: f64 = 0.8;
const LINE_HEIGHT_RATIO: f64 = 1.2;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Pin {
    Inline { story: StoryId, index: usize },
    Floating { story: StoryId, index: usize, offset: [f64; 2] },
}

impl Pin {
    pub fn story(&self) -> StoryId {
        match self {
            Pin::Inline { story, .. } | Pin::Floating { story, .. } => *story,
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Pin::Inline { index, .. } | Pin::Floating { index, .. } => *index,
        }
    }

    pub fn offset(&self) -> [f64; 2] {
        match self {
            Pin::Inline { .. } => [0.0, 0.0],
            Pin::Floating { offset, .. } => *offset,
        }
    }

    pub fn is_inline(&self) -> bool {
        matches!(self, Pin::Inline { .. })
    }

    pub fn is_floating(&self) -> bool {
        matches!(self, Pin::Floating { .. })
    }

    pub fn kind(&self) -> &'static str {
        if self.is_inline() { KIND_INLINE } else { KIND_FLOAT }
    }

    pub fn command_name(&self) -> &'static str {
        if self.is_inline() { PIN_INLINE } else { PIN_FLOAT }
    }

    fn with_index(self, index: usize) -> Pin {
        match self {
            Pin::Inline { story, .. } => Pin::Inline { story, index },
            Pin::Floating { story, offset, .. } => Pin::Floating { story, index, offset },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleTo {
    #[default]
    None,
    FontSize,
    LineHeight,
}

impl ScaleTo {
    pub const ALL: [ScaleTo; 3] = [ScaleTo::None, ScaleTo::FontSize, ScaleTo::LineHeight];

    pub fn key(&self) -> &'static str {
        match self {
            ScaleTo::None => "none",
            ScaleTo::FontSize => "font_size",
            ScaleTo::LineHeight => "line_height",
        }
    }

    pub fn from_key(key: &str) -> Option<ScaleTo> {
        ScaleTo::ALL.iter().copied().find(|v| v.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasePosition {
    #[default]
    Baseline,
    Top,
    Middle,
    Bottom,
}

impl BasePosition {
    pub const ALL: [BasePosition; 4] = [BasePosition::Baseline, BasePosition::Top, BasePosition::Middle, BasePosition::Bottom];

    pub fn key(&self) -> &'static str {
        match self {
            BasePosition::Baseline => "baseline",
            BasePosition::Top => "top",
            BasePosition::Middle => "middle",
            BasePosition::Bottom => "bottom",
        }
    }

    pub fn from_key(key: &str) -> Option<BasePosition> {
        BasePosition::ALL.iter().copied().find(|v| v.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PinSettings {
    pub scale_to: ScaleTo,
    pub base_position: BasePosition,
    pub offset: [f64; 2],
    pub prevent_wrap: bool,
    pub borders: bool,
}

impl Default for PinSettings {
    fn default() -> Self {
        Self {
            scale_to: ScaleTo::None,
            base_position: BasePosition::Baseline,
            offset: [0.0, 0.0],
            prevent_wrap: false,
            borders: false,
        }
    }
}

impl PinSettings {
    pub fn to_params(&self) -> Value {
        json!({
            "scale_to": self.scale_to.key(),
            "base_position": self.base_position.key(),
            "offset": [self.offset[0], self.offset[1]],
            "prevent_wrap": self.prevent_wrap,
            "borders": self.borders,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PinRecord {
    pub object: ObjectId,
    pub pin: Pin,
    pub settings: PinSettings,
    pub restore_pos: [f32; 2],
    pub restore_size: [f32; 2],
}

impl PinRecord {
    pub fn to_range(&self) -> Value {
        let index = self.pin.index();
        let end = if self.pin.is_inline() { index + 1 } else { index };
        json!({
            "start": index,
            "end": end,
            "pin": self.pin.kind(),
            "object": self.object.0,
            "story": self.pin.story().0,
            "index": index,
            "offset": [self.settings.offset[0], self.settings.offset[1]],
            "scale_to": self.settings.scale_to.key(),
            "base_position": self.settings.base_position.key(),
            "prevent_wrap": self.settings.prevent_wrap,
            "borders": self.settings.borders,
            "restore_pos": [self.restore_pos[0], self.restore_pos[1]],
            "restore_size": [self.restore_size[0], self.restore_size[1]],
        })
    }

    pub fn to_params(&self) -> Value {
        self.to_range()
    }

    pub fn to_command(&self) -> Command {
        Command::with_params(self.pin.command_name(), self.to_params())
    }

    pub fn from_value(value: &Value, fallback_story: Option<StoryId>) -> Option<PinRecord> {
        let kind = value.get("pin").and_then(|v| v.as_str())?;
        let object = value.get("object").and_then(parse_u64).map(ObjectId)?;
        let story = value
            .get("story")
            .and_then(parse_u64)
            .map(StoryId)
            .or(fallback_story)?;
        let index = value
            .get("start")
            .or_else(|| value.get("index"))
            .and_then(|v| v.as_u64())? as usize;
        let settings = parse_settings(value).ok()?;
        let pin = match kind {
            KIND_INLINE => Pin::Inline { story, index },
            KIND_FLOAT => Pin::Floating { story, index, offset: settings.offset },
            _ => return None,
        };
        let restore_pos = value.get("restore_pos").and_then(parse_pair_f32).unwrap_or([0.0, 0.0]);
        let restore_size = value.get("restore_size").and_then(parse_pair_f32).unwrap_or([0.0, 0.0]);
        Some(PinRecord { object, pin, settings, restore_pos, restore_size })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinRequest {
    pub object: ObjectId,
    pub pin: Pin,
    pub settings: PinSettings,
    pub restore: Option<([f32; 2], [f32; 2])>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinChange {
    pub record: PinRecord,
    pub previous: Option<PinRecord>,
}

pub fn is_pin_command(name: &str) -> bool {
    PIN_COMMANDS.contains(&name)
}

pub fn is_pin_range(range: &Value) -> bool {
    matches!(range.get("pin").and_then(|v| v.as_str()), Some(KIND_INLINE) | Some(KIND_FLOAT))
        && range.get("object").and_then(parse_u64).is_some()
}

pub fn pin_of(doc: &Document, object: ObjectId) -> Option<PinRecord> {
    locate_pin(doc, object).map(|(_, _, record)| record)
}

pub fn is_pinned(doc: &Document, object: ObjectId) -> bool {
    pin_of(doc, object).is_some()
}

pub fn pins_in_story(doc: &Document, story: StoryId) -> Vec<PinRecord> {
    doc.stories
        .iter()
        .filter(|s| s.id == story)
        .flat_map(|s| story_pins(s))
        .collect()
}

pub fn all_pins(doc: &Document) -> Vec<PinRecord> {
    doc.stories.iter().flat_map(|s| story_pins(s)).collect()
}

pub fn story_len(doc: &Document, story: StoryId) -> Option<usize> {
    find_story(doc, story).map(|s| s.text.chars().count())
}

pub fn story_for_object(doc: &Document, object: ObjectId) -> Option<StoryId> {
    doc.get_object(object).and_then(|o| o.story_id())
}

pub fn frame_for_story(doc: &Document, story: StoryId) -> Option<ObjectId> {
    doc.objects.iter().find(|o| o.story_id() == Some(story)).map(|o| o.id)
}

pub fn resolve_story(doc: &Document, params: &Value) -> Result<StoryId, CommandError> {
    if let Some(raw) = params.get("story").filter(|v| !v.is_null()) {
        let id = parse_u64(raw).ok_or_else(CommandError::invalid_params)?;
        if find_story(doc, StoryId(id)).is_some() {
            return Ok(StoryId(id));
        }
        if let Some(story) = story_for_object(doc, ObjectId(id)) {
            if find_story(doc, story).is_some() {
                return Ok(story);
            }
        }
        return Err(CommandError::not_found());
    }
    for key in ["frame", "text_frame"] {
        if let Some(raw) = params.get(key).filter(|v| !v.is_null()) {
            let id = parse_u64(raw).ok_or_else(CommandError::invalid_params)?;
            let story = story_for_object(doc, ObjectId(id)).ok_or_else(CommandError::not_found)?;
            if find_story(doc, story).is_none() {
                return Err(CommandError::not_found());
            }
            return Ok(story);
        }
    }
    Err(CommandError::invalid_params())
}

pub fn parse_request(doc: &Document, cmd: &Command) -> Result<PinRequest, CommandError> {
    let inline = match cmd.name.as_str() {
        PIN_INLINE => true,
        PIN_FLOAT => false,
        _ => return Err(CommandError::invalid_params()),
    };
    let object = param_object(&cmd.params).ok_or_else(CommandError::invalid_params)?;
    let target = doc.get_object(object).ok_or_else(CommandError::not_found)?;
    let story = resolve_story(doc, &cmd.params)?;
    if target.story_id() == Some(story) {
        return Err(CommandError::invalid_params());
    }
    let index = param_index(&cmd.params).ok_or_else(CommandError::invalid_params)?;
    let len = story_len(doc, story).ok_or_else(CommandError::not_found)?;
    if index > len {
        return Err(CommandError::invalid_params());
    }
    let settings = parse_settings(&cmd.params)?;
    let pin = if inline {
        Pin::Inline { story, index }
    } else {
        Pin::Floating { story, index, offset: settings.offset }
    };
    let restore = match (
        cmd.params.get("restore_pos").and_then(parse_pair_f32),
        cmd.params.get("restore_size").and_then(parse_pair_f32),
    ) {
        (Some(pos), Some(size)) => Some((pos, size)),
        _ => None,
    };
    Ok(PinRequest { object, pin, settings, restore })
}

pub fn pin_object(doc: &mut Document, request: PinRequest) -> Result<PinChange, CommandError> {
    if doc.get_object(request.object).is_none() {
        return Err(CommandError::not_found());
    }
    let len = story_len(doc, request.pin.story()).ok_or_else(CommandError::not_found)?;
    if request.pin.index() > len {
        return Err(CommandError::invalid_params());
    }
    let previous = if is_pinned(doc, request.object) {
        Some(unpin_object(doc, request.object)?)
    } else {
        None
    };
    let mut index = request.pin.index();
    if let Some(prev) = previous {
        if prev.pin.is_inline() && prev.pin.story() == request.pin.story() && prev.pin.index() < index {
            index -= 1;
        }
    }
    let (restore_pos, restore_size) = match request.restore {
        Some(restore) => restore,
        None => {
            let object = doc.get_object(request.object).ok_or_else(CommandError::not_found)?;
            (object.pos, object.size)
        }
    };
    let pin = match request.pin {
        Pin::Floating { story, .. } => Pin::Floating { story, index, offset: request.settings.offset },
        other => other.with_index(index),
    };
    let record = PinRecord { object: request.object, pin, settings: request.settings, restore_pos, restore_size };
    apply_record(doc, &record)?;
    Ok(PinChange { record, previous })
}

pub fn apply_record(doc: &mut Document, record: &PinRecord) -> Result<(), CommandError> {
    if doc.get_object(record.object).is_none() {
        return Err(CommandError::not_found());
    }
    let story_id = record.pin.story();
    let index = record.pin.index();
    {
        let story = find_story_mut(doc, story_id).ok_or_else(CommandError::not_found)?;
        if index > story.text.chars().count() {
            return Err(CommandError::invalid_params());
        }
        if record.pin.is_inline() {
            insert_glyph(story, index)?;
        }
        story.ranges.push(record.to_range());
    }
    layout_pin(doc, record);
    Ok(())
}

pub fn unpin_object(doc: &mut Document, object: ObjectId) -> Result<PinRecord, CommandError> {
    let (story_pos, range_pos, record) = locate_pin(doc, object).ok_or_else(CommandError::not_found)?;
    {
        let story = &mut doc.stories[story_pos];
        story.ranges.remove(range_pos);
        if record.pin.is_inline() {
            remove_glyph(story, record.pin.index());
        }
    }
    if let Some(target) = doc.find_object_mut(object.0) {
        target.pos = record.restore_pos;
        target.size = record.restore_size;
    }
    Ok(record)
}

pub fn reflow_story(doc: &mut Document, story: StoryId) -> usize {
    let records = pins_in_story(doc, story);
    for record in &records {
        layout_pin(doc, record);
    }
    records.len()
}

pub fn reflow_all(doc: &mut Document) -> usize {
    let records = all_pins(doc);
    for record in &records {
        layout_pin(doc, record);
    }
    records.len()
}

pub fn pin_position(doc: &Document, record: &PinRecord) -> Option<([f32; 2], [f32; 2])> {
    let story = find_story(doc, record.pin.story())?;
    let size_pt = story_size_pt(story);
    let char_width = size_pt * FIXTURE_WIDTH_RATIO;
    let frame = doc
        .objects
        .iter()
        .find(|o| o.story_id() == Some(record.pin.story()) && o.id != record.object);
    let origin = frame.map(|f| [f.pos[0] as f64, f.pos[1] as f64]).unwrap_or([0.0, 0.0]);
    let frame_size = frame.map(|f| [f.size[0] as f64, f.size[1] as f64]).unwrap_or([0.0, 0.0]);
    let mut line = 0usize;
    let mut column = 0usize;
    for (i, ch) in story.text.chars().enumerate() {
        if i >= record.pin.index() {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 0;
            continue;
        }
        if frame_size[0] > 0.0 && column > 0 && (column + 1) as f64 * char_width > frame_size[0] {
            line += 1;
            column = 0;
        }
        column += 1;
    }
    let base_w = record.restore_size[0] as f64;
    let base_h = record.restore_size[1] as f64;
    let target_h = match record.settings.scale_to {
        ScaleTo::None => base_h,
        ScaleTo::FontSize => size_pt,
        ScaleTo::LineHeight => size_pt * LINE_HEIGHT_RATIO,
    };
    let (w, h) = if base_h > 0.0 && target_h != base_h {
        (base_w * target_h / base_h, target_h)
    } else {
        (base_w, base_h)
    };
    let line_top = origin[1] + line as f64 * size_pt;
    let x = origin[0] + column as f64 * char_width;
    let y = match record.settings.base_position {
        BasePosition::Baseline => line_top + size_pt * BASELINE_RATIO - h,
        BasePosition::Top => line_top,
        BasePosition::Middle => line_top + (size_pt - h) / 2.0,
        BasePosition::Bottom => line_top + size_pt - h,
    };
    let mut pos = [x + record.settings.offset[0], y + record.settings.offset[1]];
    if record.settings.borders && frame.is_some() {
        pos[0] = clamp_into(pos[0], origin[0], frame_size[0], w);
        pos[1] = clamp_into(pos[1], origin[1], frame_size[1], h);
    }
    let out_pos = [pos[0] as f32, pos[1] as f32];
    let out_size = [w as f32, h as f32];
    if out_pos.iter().chain(out_size.iter()).all(|v| v.is_finite()) {
        Some((out_pos, out_size))
    } else {
        None
    }
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    if !is_pin_command(&cmd.name) {
        return None;
    }
    if !session.has_document {
        return Some(false);
    }
    Some(match cmd.name.as_str() {
        PIN_UNPIN => param_object(&cmd.params).map(|o| is_pinned(&session.document, o)).unwrap_or(false),
        _ => parse_request(&session.document, cmd).is_ok(),
    })
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    if !is_pin_command(&cmd.name) {
        return None;
    }
    Some(run_pin(session, cmd))
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<Result<(), CommandError>> {
    if !is_pin_command(&cmd.name) {
        return None;
    }
    Some(apply_document(&mut session.document, cmd))
}

pub fn apply_document(doc: &mut Document, cmd: &Command) -> Result<(), CommandError> {
    match cmd.name.as_str() {
        PIN_UNPIN => {
            let object = param_object(&cmd.params).ok_or_else(CommandError::invalid_params)?;
            let previous = match cmd.params.get("previous").filter(|v| !v.is_null()) {
                Some(value) => Some(PinRecord::from_value(value, None).ok_or_else(CommandError::invalid_params)?),
                None => None,
            };
            if let Some(prev) = previous {
                if prev.object != object {
                    return Err(CommandError::invalid_params());
                }
            }
            unpin_object(doc, object)?;
            if let Some(prev) = previous {
                apply_record(doc, &prev)?;
            }
            Ok(())
        }
        PIN_INLINE | PIN_FLOAT => {
            let request = parse_request(doc, cmd)?;
            pin_object(doc, request).map(|_| ())
        }
        _ => Err(CommandError::invalid_params()),
    }
}

fn run_pin(session: &mut Session, cmd: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::new("NO_DOCUMENT"));
    }
    let (object, undo, record) = match cmd.name.as_str() {
        PIN_UNPIN => {
            if cmd.params.get("previous").map(|v| !v.is_null()).unwrap_or(false) {
                return Err(CommandError::invalid_params());
            }
            let object = param_object(&cmd.params).ok_or_else(CommandError::invalid_params)?;
            let removed = unpin_object(&mut session.document, object)?;
            (object, removed.to_command(), None)
        }
        _ => {
            let request = parse_request(&session.document, cmd)?;
            let change = pin_object(&mut session.document, request)?;
            let previous = change.previous.map(|p| p.to_params()).unwrap_or(Value::Null);
            let undo = Command::with_params(PIN_UNPIN, json!({ "object": request.object.0, "previous": previous }));
            (request.object, undo, Some(change.record))
        }
    };
    session.undo.push(crate::undo::Change { name: cmd.name.clone(), redo: cmd.clone(), undo });
    session.generation += 1;
    session.document.version += 1;
    let pin = record.map(|r| r.to_range()).unwrap_or(Value::Null);
    let position = session.document.get_object(object).map(|o| json!([o.pos[0], o.pos[1]])).unwrap_or(Value::Null);
    Ok(json!({
        "doc_version": session.generation,
        "changes": [{ "name": cmd.name }],
        "object": object.0,
        "pin": pin,
        "pos": position,
    }))
}

fn layout_pin(doc: &mut Document, record: &PinRecord) {
    if let Some((pos, size)) = pin_position(doc, record) {
        if let Some(target) = doc.find_object_mut(record.object.0) {
            target.pos = pos;
            target.size = size;
        }
    }
}

fn story_pins(story: &Story) -> Vec<PinRecord> {
    story
        .ranges
        .iter()
        .filter(|r| is_pin_range(r))
        .filter_map(|r| PinRecord::from_value(r, Some(story.id)))
        .map(|mut r| {
            r.pin = match r.pin {
                Pin::Inline { index, .. } => Pin::Inline { story: story.id, index },
                Pin::Floating { index, offset, .. } => Pin::Floating { story: story.id, index, offset },
            };
            r
        })
        .collect()
}

fn locate_pin(doc: &Document, object: ObjectId) -> Option<(usize, usize, PinRecord)> {
    for (story_pos, story) in doc.stories.iter().enumerate() {
        for (range_pos, range) in story.ranges.iter().enumerate() {
            if !is_pin_range(range) {
                continue;
            }
            if range.get("object").and_then(parse_u64) != Some(object.0) {
                continue;
            }
            if let Some(mut record) = PinRecord::from_value(range, Some(story.id)) {
                record.pin = match record.pin {
                    Pin::Inline { index, .. } => Pin::Inline { story: story.id, index },
                    Pin::Floating { index, offset, .. } => Pin::Floating { story: story.id, index, offset },
                };
                return Some((story_pos, range_pos, record));
            }
        }
    }
    None
}

fn find_story(doc: &Document, id: StoryId) -> Option<&Story> {
    doc.stories.iter().find(|s| s.id == id)
}

fn find_story_mut(doc: &mut Document, id: StoryId) -> Option<&mut Story> {
    doc.stories.iter_mut().find(|s| s.id == id)
}

fn story_size_pt(story: &Story) -> f64 {
    story
        .ranges
        .iter()
        .filter_map(|r| r.get("size_pt").and_then(|v| v.as_f64()))
        .find(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(DEFAULT_SIZE_PT)
}

fn char_byte(text: &str, at: usize) -> Option<usize> {
    if at == 0 {
        return Some(0);
    }
    let mut count = 0usize;
    for (byte, _) in text.char_indices() {
        if count == at {
            return Some(byte);
        }
        count += 1;
    }
    if count == at { Some(text.len()) } else { None }
}

fn insert_glyph(story: &mut Story, index: usize) -> Result<(), CommandError> {
    let byte = char_byte(&story.text, index).ok_or_else(CommandError::invalid_params)?;
    story.text.insert(byte, PIN_GLYPH);
    for range in story.ranges.iter_mut() {
        shift_range_insert(range, index);
    }
    for (pos, _) in story.specials.iter_mut() {
        if *pos >= index {
            *pos += 1;
        }
    }
    for range in story.paras.iter_mut() {
        shift_range_insert(range, index);
    }
    Ok(())
}

fn remove_glyph(story: &mut Story, index: usize) {
    let start = match char_byte(&story.text, index) {
        Some(byte) => byte,
        None => return,
    };
    if story.text[start..].chars().next() != Some(PIN_GLYPH) {
        return;
    }
    story.text.replace_range(start..start + PIN_GLYPH.len_utf8(), "");
    for range in story.ranges.iter_mut() {
        shift_range_remove(range, index);
    }
    for (pos, _) in story.specials.iter_mut() {
        if *pos > index {
            *pos -= 1;
        }
    }
    for range in story.paras.iter_mut() {
        shift_range_remove(range, index);
    }
}

fn shift_range_insert(range: &mut Value, index: usize) {
    let (start, end) = match range_bounds(range) {
        Some(bounds) => bounds,
        None => return,
    };
    let (start, end) = if start >= index {
        (start + 1, end + 1)
    } else if end > index {
        (start, end + 1)
    } else {
        (start, end)
    };
    set_range_bounds(range, start, end);
}

fn shift_range_remove(range: &mut Value, index: usize) {
    let (start, end) = match range_bounds(range) {
        Some(bounds) => bounds,
        None => return,
    };
    let start = if start > index { start - 1 } else { start };
    let end = if end > index { end - 1 } else { end };
    set_range_bounds(range, start, end.max(start));
}

fn range_bounds(range: &Value) -> Option<(usize, usize)> {
    let start = range.get("start").and_then(|v| v.as_u64())? as usize;
    let end = range.get("end").and_then(|v| v.as_u64())? as usize;
    Some((start, end))
}

fn set_range_bounds(range: &mut Value, start: usize, end: usize) {
    if let Some(map) = range.as_object_mut() {
        map.insert("start".to_string(), json!(start));
        map.insert("end".to_string(), json!(end));
        if map.contains_key("pin") && map.contains_key("index") {
            map.insert("index".to_string(), json!(start));
        }
    }
}

fn clamp_into(value: f64, origin: f64, span: f64, extent: f64) -> f64 {
    if span <= 0.0 || extent >= span {
        return origin;
    }
    value.max(origin).min(origin + span - extent)
}

fn parse_u64(value: &Value) -> Option<u64> {
    if let Some(id) = value.as_u64() {
        return Some(id);
    }
    let text = value.as_str()?;
    let raw = text
        .strip_prefix("node:")
        .or_else(|| text.strip_prefix("story:"))
        .unwrap_or(text);
    raw.parse::<u64>().ok()
}

fn param_object(params: &Value) -> Option<ObjectId> {
    for key in ["object", "id", "node"] {
        if let Some(id) = params.get(key).and_then(parse_u64) {
            return Some(ObjectId(id));
        }
    }
    None
}

fn param_index(params: &Value) -> Option<usize> {
    for key in ["index", "at"] {
        if let Some(value) = params.get(key) {
            return value.as_u64().map(|v| v as usize);
        }
    }
    None
}

fn setting<'a>(params: &'a Value, key: &str) -> Option<&'a Value> {
    params
        .get(key)
        .filter(|v| !v.is_null())
        .or_else(|| params.get("settings").and_then(|s| s.get(key)).filter(|v| !v.is_null()))
}

fn parse_pair_f64(value: &Value) -> Option<[f64; 2]> {
    let items = value.as_array()?;
    if items.len() != 2 {
        return None;
    }
    let x = items[0].as_f64()?;
    let y = items[1].as_f64()?;
    if x.is_finite() && y.is_finite() { Some([x, y]) } else { None }
}

fn parse_pair_f32(value: &Value) -> Option<[f32; 2]> {
    parse_pair_f64(value).map(|[x, y]| [x as f32, y as f32])
}

fn parse_settings(params: &Value) -> Result<PinSettings, CommandError> {
    let mut settings = PinSettings::default();
    if let Some(value) = setting(params, "scale_to") {
        settings.scale_to = value.as_str().and_then(ScaleTo::from_key).ok_or_else(CommandError::invalid_params)?;
    }
    if let Some(value) = setting(params, "base_position") {
        settings.base_position = value.as_str().and_then(BasePosition::from_key).ok_or_else(CommandError::invalid_params)?;
    }
    if let Some(value) = setting(params, "offset") {
        settings.offset = parse_pair_f64(value).ok_or_else(CommandError::invalid_params)?;
    }
    if let Some(value) = setting(params, "prevent_wrap") {
        settings.prevent_wrap = value.as_bool().ok_or_else(CommandError::invalid_params)?;
    }
    if let Some(value) = setting(params, "borders") {
        settings.borders = value.as_bool().ok_or_else(CommandError::invalid_params)?;
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, ObjectKind};

    fn session_with_story(text: &str) -> (Session, StoryId, ObjectId, ObjectId) {
        let mut doc = Document::default();
        let story = StoryId(7);
        doc.stories.push(Story { id: story, text: text.to_string(), ..Default::default() });
        let frame = doc.create_object(Object { pos: [10.0, 20.0], size: [200.0, 100.0], kind: ObjectKind::FrameText { story }, ..Object::default() });
        let shape = doc.create_object(Object { pos: [300.0, 300.0], size: [24.0, 12.0], ..Object::default() });
        (Session::new(doc), story, frame, shape)
    }

    fn undo(session: &mut Session) {
        let cmd = session.undo.undo().expect("undo");
        apply(session, &cmd).expect("pin command").expect("apply");
    }

    fn redo(session: &mut Session) {
        let cmd = session.undo.redo().expect("redo");
        apply(session, &cmd).expect("pin command").expect("apply");
    }

    #[test]
    fn inline_inserts_glyph_and_undo_restores() {
        let (mut session, story, _frame, shape) = session_with_story("abcd");
        let cmd = Command::with_params(PIN_INLINE, json!({ "object": shape.0, "story": story.0, "index": 2 }));
        assert_eq!(can_execute(&session, &cmd), Some(true));
        run(&mut session, &cmd).expect("pin").expect("ok");
        let text = session.document.stories[0].text.clone();
        assert_eq!(text, "ab\u{FFFC}cd");
        let record = pin_of(&session.document, shape).expect("pinned");
        assert_eq!(record.pin, Pin::Inline { story, index: 2 });
        undo(&mut session);
        assert_eq!(session.document.stories[0].text, "abcd");
        assert!(session.document.stories[0].ranges.is_empty());
        assert_eq!(session.document.get_object(shape).map(|o| o.pos), Some([300.0, 300.0]));
        redo(&mut session);
        assert_eq!(session.document.stories[0].text, "ab\u{FFFC}cd");
    }

    #[test]
    fn frame_id_resolves_story_and_float_keeps_text() {
        let (mut session, story, frame, shape) = session_with_story("hello");
        let cmd = Command::with_params(PIN_FLOAT, json!({ "object": format!("node:{}", shape.0), "frame": frame.0, "index": 1, "offset": [5.0, -2.0] }));
        run(&mut session, &cmd).expect("pin").expect("ok");
        assert_eq!(session.document.stories[0].text, "hello");
        let record = pin_of(&session.document, shape).expect("pinned");
        assert_eq!(record.pin, Pin::Floating { story, index: 1, offset: [5.0, -2.0] });
        undo(&mut session);
        assert!(pin_of(&session.document, shape).is_none());
    }

    #[test]
    fn unpin_and_undo_restores_pin() {
        let (mut session, story, _frame, shape) = session_with_story("abc");
        run(&mut session, &Command::with_params(PIN_INLINE, json!({ "object": shape.0, "story": story.0, "index": 1 }))).expect("pin").expect("ok");
        run(&mut session, &Command::with_params(PIN_UNPIN, json!({ "object": shape.0 }))).expect("pin").expect("ok");
        assert_eq!(session.document.stories[0].text, "abc");
        assert_eq!(session.document.get_object(shape).map(|o| o.pos), Some([300.0, 300.0]));
        undo(&mut session);
        assert_eq!(session.document.stories[0].text, "a\u{FFFC}bc");
        assert!(is_pinned(&session.document, shape));
    }

    #[test]
    fn repin_replaces_and_undo_restores_previous() {
        let (mut session, story, _frame, shape) = session_with_story("abcdef");
        run(&mut session, &Command::with_params(PIN_INLINE, json!({ "object": shape.0, "story": story.0, "index": 1 }))).expect("pin").expect("ok");
        run(&mut session, &Command::with_params(PIN_INLINE, json!({ "object": shape.0, "story": story.0, "index": 5 }))).expect("pin").expect("ok");
        assert_eq!(session.document.stories[0].text, "abcd\u{FFFC}ef");
        assert_eq!(pins_in_story(&session.document, story).len(), 1);
        undo(&mut session);
        assert_eq!(session.document.stories[0].text, "a\u{FFFC}bcdef");
        assert_eq!(pin_of(&session.document, shape).map(|r| r.pin.index()), Some(1));
    }

    #[test]
    fn rejects_missing_and_out_of_range() {
        let (mut session, story, _frame, shape) = session_with_story("ab");
        let past = Command::with_params(PIN_INLINE, json!({ "object": shape.0, "story": story.0, "index": 9 }));
        assert_eq!(run(&mut session, &past).expect("pin").unwrap_err().code, "INVALID_PARAMS");
        let missing = Command::with_params(PIN_INLINE, json!({ "object": 999, "story": story.0, "index": 0 }));
        assert_eq!(run(&mut session, &missing).expect("pin").unwrap_err().code, "NOT_FOUND");
        let unpin = Command::with_params(PIN_UNPIN, json!({ "object": shape.0 }));
        assert_eq!(can_execute(&session, &unpin), Some(false));
        assert!(!session.undo.can_undo());
    }

    #[test]
    fn inline_shifts_later_ranges_and_specials() {
        let (mut session, story, _frame, shape) = session_with_story("abcd");
        session.document.stories[0].ranges.push(json!({ "start": 2, "end": 4, "size_pt": 12.0 }));
        session.document.stories[0].paras.push(json!({ "start": 2, "end": 4 }));
        session.document.stories[0].specials.push((3, crate::document::Special::IndentToHere));
        run(&mut session, &Command::with_params(PIN_INLINE, json!({ "object": shape.0, "story": story.0, "index": 1 }))).expect("pin").expect("ok");
        assert_eq!(session.document.stories[0].ranges[0]["start"], json!(3));
        assert_eq!(session.document.stories[0].paras[0]["start"], json!(3));
        assert_eq!(session.document.stories[0].specials[0].0, 4);
        undo(&mut session);
        assert_eq!(session.document.stories[0].ranges[0]["start"], json!(2));
        assert_eq!(session.document.stories[0].ranges[0]["end"], json!(4));
        assert_eq!(session.document.stories[0].paras[0]["start"], json!(2));
        assert_eq!(session.document.stories[0].paras[0]["end"], json!(4));
        assert_eq!(session.document.stories[0].specials[0].0, 3);
    }

    #[test]
    fn float_pin_moves_with_offset_and_undo_restores() {
        let (mut session, story, _frame, shape) = session_with_story("hello");
        let cmd = Command::with_params(PIN_FLOAT, json!({ "object": shape.0, "story": story.0, "index": 2, "offset": [10.0, -5.0] }));
        assert_eq!(can_execute(&session, &cmd), Some(true));
        run(&mut session, &cmd).expect("pin").expect("ok");
        let record = pin_of(&session.document, shape).expect("pinned");
        assert_eq!(record.pin, Pin::Floating { story, index: 2, offset: [10.0, -5.0] });
        assert_eq!(session.document.stories[0].text, "hello");
        undo(&mut session);
        assert!(pin_of(&session.document, shape).is_none());
        assert_eq!(session.document.get_object(shape).map(|o| o.pos), Some([300.0, 300.0]));
    }

    #[test]
    fn float_reflow_updates_position() {
        let (mut session, story, _frame, shape) = session_with_story("hello");
        run(&mut session, &Command::with_params(PIN_FLOAT, json!({ "object": shape.0, "story": story.0, "index": 1, "offset": [3.0, 2.0] }))).expect("pin").expect("ok");
        let before = session.document.get_object(shape).map(|o| o.pos);
        session.document.stories[0].text = "hXello".to_string();
        reflow_story(&mut session.document, story);
        let after = session.document.get_object(shape).map(|o| o.pos);
        assert_ne!(before, after);
    }
}
