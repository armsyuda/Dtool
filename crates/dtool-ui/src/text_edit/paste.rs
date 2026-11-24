use dtool_core::serde_json::json;
use dtool_core::undo::Change;
use dtool_core::{Command, Session};

pub fn is_url(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains(' ') {
        return false;
    }
    trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("mailto:")
}

fn type_text(session: &mut Session, text: &str) -> bool {
    session.run(&Command::with_params("text.edit.type", json!({ "text": text }))).is_ok()
}

pub fn paste_at_caret(session: &mut Session, text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let Some(edit) = session.text_edit.as_ref() else { return false };
    if !is_url(text) {
        return type_text(session, text);
    }
    let (story, start) = (edit.story.0, edit.caret.min(edit.anchor));
    dtool_core::text_cmd::commit_text_edit(session);
    if !type_text(session, text) {
        return false;
    }
    dtool_core::text_cmd::commit_text_edit(session);
    let link = Command::with_params("text.link.set", json!({ "story": story, "range": { "start": start, "end": start + text.chars().count() }, "url": text.trim() }));
    if session.run(&link).is_err() {
        return true;
    }
    let linked = session.document.stories.iter().find(|item| item.id.0 == story).cloned();
    let (Some(linked), Some(_), Some(typed)) = (linked, session.undo.pop_last(), session.undo.pop_last()) else { return true };
    let redo = Command::with_params("text.story.restore", json!({ "story": linked }));
    session.undo.push(Change { name: "text.paste".to_string(), redo, undo: typed.undo });
    true
}