use crate::command::Session;
use crate::document::Document;
use crate::longdoc::sections::{format_number, NumberStyle};
use crate::text_cmd::push_change;
use crate::CommandError;
use dtool_protocol::{Command, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const NOTE_RESTORE: &str = "note.restore";
pub const SUPPORTED: &[NoteKind] = &[NoteKind::Footnote, NoteKind::Endnote, NoteKind::Sidenote];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteKind {
    Footnote,
    Endnote,
    Sidenote,
}

impl NoteKind {
    pub fn from_name(name: &str) -> Option<NoteKind> {
        match name.to_ascii_lowercase().as_str() {
            "footnote" => Some(NoteKind::Footnote),
            "endnote" => Some(NoteKind::Endnote),
            "sidenote" => Some(NoteKind::Sidenote),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            NoteKind::Footnote => "footnote",
            NoteKind::Endnote => "endnote",
            NoteKind::Sidenote => "sidenote",
        }
    }

    pub fn supported(self) -> bool {
        SUPPORTED.contains(&self)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub kind: NoteKind,
    pub story_pos: usize,
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NoteRestart {
    #[default]
    Continuous,
    Page,
    Section,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteOptions {
    pub custom: bool,
    pub style: NumberStyle,
    pub start: u32,
    pub restart: NoteRestart,
    pub prefix: String,
    pub suffix: String,
}

impl Default for NoteOptions {
    fn default() -> Self {
        Self {
            custom: false,
            style: NumberStyle::Arabic,
            start: 1,
            restart: NoteRestart::Continuous,
            prefix: String::new(),
            suffix: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NotesData {
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub footnote: NoteOptions,
    #[serde(default)]
    pub endnote: NoteOptions,
    #[serde(default)]
    pub sidenote: NoteOptions,
    #[serde(default)]
    pub show_marks: bool,
    #[serde(default)]
    pub endnote_frame: Option<u64>,
}

impl NotesData {
    pub fn options(&self, kind: NoteKind) -> &NoteOptions {
        match kind {
            NoteKind::Footnote => &self.footnote,
            NoteKind::Endnote => &self.endnote,
            NoteKind::Sidenote => &self.sidenote,
        }
    }

    pub fn options_mut(&mut self, kind: NoteKind) -> &mut NoteOptions {
        match kind {
            NoteKind::Footnote => &mut self.footnote,
            NoteKind::Endnote => &mut self.endnote,
            NoteKind::Sidenote => &mut self.sidenote,
        }
    }
}

fn section_index(doc: &Document, page_index: usize) -> usize {
    let physical = page_index + 1;
    doc.sections().iter().filter(|s| s.start_page <= physical).count()
}

pub fn ordered_notes(doc: &Document, kind: NoteKind) -> Vec<&Note> {
    notes_by_page(doc, kind).into_iter().map(|(_, note)| note).collect()
}

fn notes_by_page(doc: &Document, kind: NoteKind) -> Vec<(usize, &Note)> {
    let mut notes: Vec<_> = doc.notes.notes.iter().filter(|note| note.kind == kind)
        .map(|note| (doc.page_index_for_story_pos(note.story_pos), note)).collect();
    notes.sort_by_key(|(page, note)| (*page, note.story_pos, note.id));
    notes
}

pub fn note_numbers(doc: &Document, kind: NoteKind) -> Vec<(u64, u32, String)> {
    let options = doc.notes.options(kind);
    let mut out = Vec::new();
    let mut counter = options.start.max(1);
    let mut group: Option<usize> = None;
    for (page, note) in notes_by_page(doc, kind) {
        let key = match options.restart {
            NoteRestart::Continuous => 0,
            NoteRestart::Page => page,
            NoteRestart::Section => section_index(doc, page),
        };
        if group.is_some_and(|g| g != key) {
            counter = options.start.max(1);
        }
        group = Some(key);
        let label = format!("{}{}{}", options.prefix, format_number(counter, &options.style), options.suffix);
        out.push((note.id, counter, label));
        counter = counter.saturating_add(1);
    }
    out
}

pub fn note_label(doc: &Document, id: u64) -> Option<String> {
    let kind = doc.notes.notes.iter().find(|n| n.id == id)?.kind;
    note_numbers(doc, kind).into_iter().find(|(nid, _, _)| *nid == id).map(|(_, _, label)| label)
}

pub fn insert_note(doc: &mut Document, kind: NoteKind, story_pos: usize, body: String) -> Note {
    doc.notes.next_id = doc.notes.next_id.max(doc.notes.notes.iter().map(|n| n.id).max().unwrap_or(0)) + 1;
    let note = Note { id: doc.notes.next_id, kind, story_pos, body };
    doc.notes.notes.push(note.clone());
    note
}

pub fn delete_note(doc: &mut Document, id: u64) -> Result<(), CommandError> {
    let before = doc.notes.notes.len();
    doc.notes.notes.retain(|n| n.id != id);
    if doc.notes.notes.len() == before { Err(CommandError::not_found()) } else { Ok(()) }
}

pub fn convert_notes(doc: &mut Document, from: NoteKind, to: NoteKind, id: Option<u64>) -> usize {
    let mut count = 0;
    for note in doc.notes.notes.iter_mut().filter(|n| n.kind == from && id.map_or(true, |id| n.id == id)) {
        note.kind = to;
        count += 1;
    }
    count
}

fn snapshot(doc: &Document) -> Value {
    serde_json::to_value(&doc.notes).unwrap_or(Value::Null)
}

fn record(session: &mut Session, command: &Command, before: Value) {
    let undo = Command::with_params(NOTE_RESTORE, json!({ "notes": before }));
    push_change(session, &command.name, command.clone(), undo);
}

fn param_kind(command: &Command, key: &str) -> Result<NoteKind, CommandError> {
    command.params.get(key).and_then(Value::as_str).and_then(NoteKind::from_name).ok_or_else(CommandError::invalid_params)
}

fn param_id(command: &Command) -> Option<u64> {
    command.params.get("id").and_then(Value::as_u64)
}

fn caret_pos(session: &Session) -> Option<usize> {
    session.text_edit.as_ref().map(|edit| session.document.story_pos(edit.story, edit.caret))
}

fn note_at_caret(session: &Session) -> Option<u64> {
    let pos = caret_pos(session)?;
    session.document.notes.notes.iter().filter(|n| n.story_pos <= pos).max_by_key(|n| n.story_pos).map(|n| n.id)
}

fn notes_json(doc: &Document) -> Value {
    let mut list = Vec::new();
    for kind in [NoteKind::Footnote, NoteKind::Endnote, NoteKind::Sidenote] {
        for (id, number, label) in note_numbers(doc, kind) {
            if let Some(note) = doc.notes.notes.iter().find(|n| n.id == id) {
                list.push(json!({
                    "id": id, "kind": kind.name(), "number": number, "label": label,
                    "story_pos": note.story_pos, "page": doc.page_index_for_story_pos(note.story_pos), "body": note.body
                }));
            }
        }
    }
    json!({ "notes": list, "show_marks": doc.notes.show_marks })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let has = session.has_document;
    match command.name.as_str() {
        "note.insert" => Some(has && (command.params.get("story_pos").is_some() || session.text_edit.is_some())
            && command.params.get("kind").and_then(Value::as_str).and_then(NoteKind::from_name).map_or(true, NoteKind::supported)),
        "note.convert" => Some(has && !session.document.notes.notes.is_empty()),
        "note.go" => Some(has && !session.document.notes.notes.is_empty()),
        "note.endnote_frame.set" => Some(has),
        "note.delete" | "note.set_body" | "note.options.set" | "note.list" | "note.marks.toggle" | NOTE_RESTORE => Some(has),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let result = match command.name.as_str() {
        "note.insert" => (|| {
            let kind = command.params.get("kind").and_then(Value::as_str).map_or(Some(NoteKind::Footnote), NoteKind::from_name).ok_or_else(CommandError::invalid_params)?;
            if !kind.supported() {
                return Err(CommandError::not_implemented());
            }
            let pos = command.params.get("story_pos").and_then(Value::as_u64).map(|v| v as usize).or_else(|| caret_pos(session)).ok_or_else(CommandError::invalid_params)?;
            let body = command.params.get("body").and_then(Value::as_str).unwrap_or("").to_string();
            let before = snapshot(&session.document);
            let note = insert_note(&mut session.document, kind, pos, body);
            record(session, command, before);
            Ok(json!({ "id": note.id, "label": note_label(&session.document, note.id), "doc_version": session.generation }))
        })(),
        "note.delete" => (|| {
            let id = param_id(command).or_else(|| note_at_caret(session)).ok_or_else(CommandError::invalid_params)?;
            let before = snapshot(&session.document);
            delete_note(&mut session.document, id)?;
            record(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "note.set_body" => (|| {
            let id = param_id(command).ok_or_else(CommandError::invalid_params)?;
            let body = command.params.get("body").and_then(Value::as_str).ok_or_else(CommandError::invalid_params)?.to_string();
            let before = snapshot(&session.document);
            let note = session.document.notes.notes.iter_mut().find(|n| n.id == id).ok_or_else(CommandError::not_found)?;
            note.body = body;
            record(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "note.convert" => (|| {
            let from = param_kind(command, "from")?;
            let to = param_kind(command, "to")?;
            if !to.supported() {
                return Err(CommandError::not_implemented());
            }
            let before = snapshot(&session.document);
            let count = convert_notes(&mut session.document, from, to, param_id(command));
            if count == 0 {
                return Err(CommandError::not_found());
            }
            record(session, command, before);
            Ok(json!({ "converted": count, "doc_version": session.generation }))
        })(),
        "note.go" => (|| {
            let to = command.params.get("to").and_then(Value::as_str).unwrap_or("body");
            let id = param_id(command).or_else(|| note_at_caret(session)).ok_or_else(CommandError::not_found)?;
            let note = session.document.notes.notes.iter().find(|n| n.id == id).cloned().ok_or_else(CommandError::not_found)?;
            match to {
                "reference" => {
                    if let Some((story, at)) = session.document.story_at_pos(note.story_pos) {
                        if let Some(edit) = session.text_edit.as_mut() {
                            if edit.story == story {
                                edit.caret = at;
                                edit.anchor = at;
                            }
                        }
                    }
                    Ok(json!({ "id": id, "to": "reference", "story_pos": note.story_pos, "page": session.document.page_index_for_story_pos(note.story_pos) }))
                }
                "body" => Ok(json!({ "id": id, "to": "body", "body": note.body, "label": note_label(&session.document, id) })),
                _ => Err(CommandError::invalid_params()),
            }
        })(),
        "note.options.set" => (|| {
            let kind = param_kind(command, "kind")?;
            let options: NoteOptions = serde_json::from_value(command.params.get("options").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())?;
            if options.start == 0 {
                return Err(CommandError::invalid_params());
            }
            let before = snapshot(&session.document);
            *session.document.notes.options_mut(kind) = options;
            record(session, command, before);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        "note.marks.toggle" => {
            session.document.notes.show_marks = !session.document.notes.show_marks;
            Ok(json!({ "show_marks": session.document.notes.show_marks }))
        }
        "note.endnote_frame.set" => (|| {
            let id = command.params.get("id").and_then(Value::as_u64);
            if let Some(id) = id {
                let object = session.document.get_object(crate::object::ObjectId(id)).ok_or_else(CommandError::not_found)?;
                if object.story_id().is_none() {
                    return Err(CommandError::invalid_params());
                }
            }
            let before = snapshot(&session.document);
            session.document.notes.endnote_frame = id;
            record(session, command, before);
            Ok(json!({ "endnote_frame": id, "doc_version": session.generation }))
        })(),
        "note.list" => Ok(notes_json(&session.document)),
        NOTE_RESTORE => (|| {
            let data = serde_json::from_value(command.params.get("notes").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())?;
            session.document.notes = data;
            crate::text_cmd::bump(session);
            Ok(json!({ "doc_version": session.generation }))
        })(),
        _ => return None,
    };
    Some(result)
}
