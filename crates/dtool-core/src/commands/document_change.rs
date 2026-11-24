use crate::{Command, CommandResult, Document, ObjectId, Session};
use serde_json::{json, Value};

pub(crate) fn restore(document: &Document, selection: &[ObjectId]) -> Command {
    Command::with_params(super::feature_restore::FEATURE_RESTORE, json!({
        "document": document,
        "selection": selection.iter().map(|id| id.0).collect::<Vec<_>>()
    }))
}

pub fn commit(session: &mut Session, name: &str, document: Document, selection: Vec<ObjectId>, extra: Value) -> CommandResult {
    commit_change(session, name, document, selection, extra, None, None)
}

pub(crate) fn commit_table(
    session: &mut Session, name: &str, document: Document,
    selection: Vec<ObjectId>, cells: crate::table::selection::TableSelection, extra: Value,
) -> CommandResult {
    commit_change(session, name, document, selection, extra, Some(cells), None)
}

pub(crate) fn commit_guides(
    session: &mut Session, name: &str, document: Document, selection: Vec<ObjectId>,
    guides: crate::guides::Guides, extra: Value,
) -> CommandResult {
    commit_change(session, name, document, selection, extra, None, Some(guides))
}


fn commit_change(
    session: &mut Session, name: &str, mut document: Document, selection: Vec<ObjectId>, extra: Value,
    cells: Option<crate::table::selection::TableSelection>,
    guides: Option<crate::guides::Guides>,
) -> CommandResult {
    let mut undo = restore(&session.document, &session.selection.ids);
    document.version = session.document.version + 1;
    document.next_object_id = document.next_object_id.max(session.document.next_object_id);
    let mut redo = restore(&document, &selection);
    if let Some(cells) = cells {
        undo.params["table_selection"] = json!(crate::table::selection::current(session));
        redo.params["table_selection"] = json!(cells);
        session.table_selection = cells;
    }
    if let Some(guides) = guides {
        undo.params["guides"] = json!(session.guides);
        redo.params["guides"] = json!(guides);
        session.guides.replace_with(guides);
    }
    session.undo.push(crate::undo::Change { name: name.to_string(), redo, undo });
    session.document = document;
    session.selection.ids = selection;
    session.generation += 1;
    let mut result = json!({"doc_version": session.generation, "changes": [{"name": name}]});
    if let (Some(target), Some(source)) = (result.as_object_mut(), extra.as_object()) {
        target.extend(source.clone());
    }
    Ok(result)
}
