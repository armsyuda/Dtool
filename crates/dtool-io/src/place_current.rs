use dtool_core::clipboard::{collect_bundle, paste_bundle};
use dtool_core::commands::arrange::{commit_snapshot, snapshot};
use dtool_core::selection::SelectOp;
use dtool_core::serde_json::json;
use dtool_core::{CommandError, CommandResult, Document, ObjectId, Session};

pub const IMPORT_PLACE: &str = "import.place";

pub fn place_document(session: &mut Session, imported: &Document) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let roots: Vec<ObjectId> = imported.objects.iter().filter(|object| !object.parent_id.is_some_and(|parent| imported.get_object(parent).is_some())).map(|object| object.id).collect();
    if roots.is_empty() {
        return Err(CommandError::invalid_params());
    }
    let bundle = collect_bundle(imported, &roots);
    let before = snapshot(session);
    let ids = paste_bundle(&mut session.document, &bundle, None, [0.0, 0.0]);
    let placed: Vec<u64> = ids.iter().map(|id| id.0).collect();
    let selected: Vec<ObjectId> = ids.iter().copied().filter(|id| session.document.get_object(*id).is_some_and(|object| !object.parent_id.is_some_and(|parent| ids.contains(&parent)))).collect();
    session.selection.set(selected, SelectOp::Replace);
    commit_snapshot(session, IMPORT_PLACE, before, json!({ "ids": placed }))
}