use dtool_protocol::{Command, CommandError, CommandResult};

use crate::command::Session;
use crate::selection::{PixelSelection, PixelSurface};

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        "selection.invert" | "selection.all" | "selection.none" => Some(session.has_document),
        "selection.reselect" => Some(session.has_document && reselectable(session).is_some()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if can_execute(session, command).is_none() {
        return None;
    }
    Some(run_named(session, command))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if can_execute(session, command).is_none() {
        return None;
    }
    if !session.has_document {
        return Some(Err(CommandError::no_document()));
    }
    Some(apply_selection(session, command))
}

fn run_named(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let before = session.document.pixel_selection.clone();
    if command.name != "selection.none" {
        apply_selection(session, command)?;
        return Ok(if matches!(command.name.as_str(), "selection.reselect" | "selection.all") {
            record_mask(session, command, before)
        } else {
            record(session, command, before)
        });
    }
    let previous = session.last_deselected.clone();
    apply_selection(session, command)?;
    let redo = restore_after_deselect(&None, &session.last_deselected);
    let undo = restore_after_deselect(&before, &previous);
    Ok(push_change(session, command.name.clone(), redo, undo))
}

pub fn record(session: &mut Session, command: &Command, before: Option<PixelSelection>) -> serde_json::Value {
    record_change(session, command.name.clone(), command.clone(), before)
}

pub(crate) fn record_mask(session: &mut Session, command: &Command, before: Option<PixelSelection>) -> serde_json::Value {
    let redo = Command::with_params(crate::command::PIXEL_SELECTION_RESTORE, serde_json::json!({
        "selection": session.document.pixel_selection
    }));
    record_change(session, command.name.clone(), redo, before)
}

fn record_change(session: &mut Session, name: String, redo: Command, before: Option<PixelSelection>) -> serde_json::Value {
    let undo = Command::with_params(crate::command::PIXEL_SELECTION_RESTORE, serde_json::json!({ "selection": before }));
    push_change(session, name, redo, undo)
}

fn push_change(session: &mut Session, name: String, redo: Command, undo: Command) -> serde_json::Value {
    session.undo.push(crate::undo::Change { name: name.clone(), redo, undo });
    session.generation += 1;
    let mut value = crate::command::pixel_selection_summary(&session.document);
    value["doc_version"] = serde_json::json!(session.generation);
    value["changes"] = serde_json::json!([{ "name": name }]);
    value
}

fn restore_after_deselect(selection: &Option<PixelSelection>, last: &Option<PixelSelection>) -> Command {
    Command::with_params(crate::command::PIXEL_SELECTION_RESTORE, serde_json::json!({
        "selection": selection,
        "last_deselected": last,
    }))
}

fn reselectable(session: &Session) -> Option<&PixelSelection> {
    if session.document.pixel_selection.as_ref().is_some_and(|mask| !mask.is_empty()) {
        return None;
    }
    session.last_deselected.as_ref()
        .filter(|mask| crate::selection::selection_size_matches_document(&session.document, mask))
}

fn apply_selection(session: &mut Session, command: &Command) -> Result<(), CommandError> {
    let doc = &mut session.document;
    match command.name.as_str() {
        "selection.invert" => {
            if doc.pixel_selection.is_none() {
                return Err(CommandError::new("NOTHING_SELECTED"));
            }
            if let Some(ref mut sel) = doc.pixel_selection {
                sel.invert();
            }
            Ok(())
        }
        "selection.all" => {
            let surface = PixelSurface::for_command(doc, &command.params)?;
            let [w, h] = surface.size;
            if u64::from(w) * u64::from(h) > 32 * 1024 * 1024 {
                return Err(CommandError::new("REQUEST_TOO_LARGE"));
            }
            let mut mask = PixelSelection::all(w, h);
            mask.page_id = command.params["page_id"].as_u64().map(crate::document::PageId);
            doc.pixel_selection = Some(mask);
            Ok(())
        }
        "selection.none" => {
            if let Some(mask) = doc.pixel_selection.take().filter(|mask| !mask.is_empty()) {
                session.last_deselected = Some(mask);
            }
            Ok(())
        }
        "selection.reselect" => {
            let mask = reselectable(session).cloned().ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
            session.document.pixel_selection = Some(mask);
            Ok(())
        }
        _ => Err(CommandError::not_implemented()),
    }
}
