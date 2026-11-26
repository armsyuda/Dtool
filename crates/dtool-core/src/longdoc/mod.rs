pub mod books;
pub mod sections;
pub mod fields;
pub mod helpers;
pub mod anchors;
pub mod xref;
pub mod stats;
pub mod merge;
pub mod index;
pub mod marks;
pub mod notes;
pub mod notes_layout;
pub mod pages;
pub(crate) mod page_import;
pub use page_import::insert_imported_pages;
pub mod toc;

pub use stats::DocStats;

use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name == DOC_RESTORE { return Some(session.has_document); }
    if let Some(v) = sections::can_execute(session, command) { return Some(v); }
    if let Some(v) = fields::can_execute(session, command) { return Some(v); }
    if let Some(v) = anchors::can_execute(session, command) { return Some(v); }
    if let Some(v) = xref::can_execute(session, command) { return Some(v); }
    if let Some(v) = stats::can_execute(session, command) { return Some(v); }
    if let Some(v) = merge::can_execute(session, command) { return Some(v); }
    if let Some(v) = notes::can_execute(session, command) { return Some(v); }
    if let Some(v) = toc::can_execute(session, command) { return Some(v); }
    if let Some(v) = index::can_execute(session, command) { return Some(v); }
    books::can_execute(command)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name == DOC_RESTORE { return Some(restore_doc(session, command)); }
    if let Some(v) = sections::run(session, command) { return Some(v); }
    if let Some(v) = fields::run(session, command) { return Some(v); }
    if let Some(v) = anchors::run(session, command) { return Some(v); }
    if let Some(v) = xref::run(session, command) { return Some(v); }
    if let Some(v) = stats::run(session, command) { return Some(v); }
    if let Some(v) = merge::run(session, command) { return Some(v); }
    if let Some(v) = notes::run(session, command) { return Some(v); }
    if let Some(v) = toc::run(session, command) { return Some(v); }
    if let Some(v) = index::run(session, command) { return Some(v); }
    books::run(session, command)
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name == fields::DOC_METADATA_SET {
        return fields::apply(session, command);
    }
    match run(session, command) {
        Some(Ok(_)) => Some(Ok(())),
        Some(Err(err)) => Some(Err(err)),
        None => None,
    }
}

pub const DOC_RESTORE: &str = "longdoc.doc.restore";

pub(crate) fn doc_snapshot(doc: &crate::document::Document) -> serde_json::Value {
    serde_json::json!({
        "stories": doc.stories,
        "objects": doc.objects,
        "next_object_id": doc.next_object_id,
        "toc": doc.toc,
        "index": doc.index,
        "links": anchors::links_snapshot(doc),
        "notes": doc.notes
    })
}

pub(crate) fn record_doc(session: &mut Session, command: &Command, before: serde_json::Value) {
    let undo = Command::with_params(DOC_RESTORE, serde_json::json!({ "snapshot": before }));
    crate::text_cmd::push_change(session, &command.name, command.clone(), undo);
}

fn restore_doc(session: &mut Session, command: &Command) -> CommandResult {
    let snap = command.params.get("snapshot").ok_or_else(CommandError::invalid_params)?;
    let doc = &mut session.document;
    if let Some(v) = snap.get("stories") {
        doc.stories = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(v) = snap.get("objects") {
        doc.objects = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(v) = snap.get("next_object_id").and_then(serde_json::Value::as_u64) {
        doc.next_object_id = v;
    }
    if let Some(v) = snap.get("toc") {
        doc.toc = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(v) = snap.get("index") {
        doc.index = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(links) = snap.get("links") {
        anchors::links_restore(doc, links)?;
    }
    if let Some(notes) = snap.get("notes") {
        doc.notes = serde_json::from_value(notes.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    crate::text_cmd::bump(session);
    Ok(serde_json::json!({ "doc_version": session.generation }))
}
