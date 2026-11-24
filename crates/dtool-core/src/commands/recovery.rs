use crate::workspace::{ContextId, Scope, Workspace};
use dtool_protocol::{Command, CommandError, CommandResult};

pub fn run_workspace(workspace: &mut Workspace, scope: Scope, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if command.name != "doc.recover" { return None; }
    let mut id = scope.context.or_else(|| workspace.active());
    let result = (|| {
        if scope.context.is_some_and(|id| workspace.get(id).is_none()) { return Err(CommandError::context_not_found()); }
        let path = command.path_param().ok_or_else(CommandError::invalid_params)?;
        let root = std::fs::canonicalize(crate::autosave::recovery_dir()).map_err(|_| CommandError::new("IO.NOT_FOUND"))?;
        let path = std::fs::canonicalize(path).map_err(|_| CommandError::new("IO.NOT_FOUND"))?;
        if !path.starts_with(root) || path.extension().and_then(|value| value.to_str()) != Some(crate::autosave::RECOVERY_EXTENSION) { return Err(CommandError::new("PATH_NOT_ALLOWED")); }
        let bytes = std::fs::read(&path).map_err(|_| CommandError::new("IO.OTHER"))?;
        let body = std::str::from_utf8(&bytes).map_err(|_| CommandError::new("IO.INVALID_FORMAT"))?;
        let mut document = crate::document::Document::from_native_json(body)?;
        let saved_version = document.version;
        document.version = document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        let version = document.version;
        let title = path.file_stem().and_then(|value| value.to_str()).unwrap_or(crate::autosave::RECOVERY_EXTENSION);
        let created = workspace.insert_context_with_generation(title, None, document, saved_version, version);
        id = Some(created);
        let history = match (crate::autosave::recovered_history(&path), workspace.get_mut(created)) {
            (Ok(Some(store)), Some(context)) => {
                context.session.history.save_with_document = true;
                serde_json::json!({ "restored": store.restore_into(&mut context.session.undo) })
            }
            (Err(error), _) => serde_json::json!({ "restored": 0, "error": error.code }),
            _ => serde_json::json!({ "restored": 0 }),
        };
        Ok(serde_json::json!({ "context": created.0, "doc_version": version, "recovered": true, "path": null, "history": history }))
    })();
    Some((id, result))
}
