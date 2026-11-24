use crate::document::Document;
use crate::workspace::{ContextId, Scope, Workspace};
use dtool_protocol::{Command, CommandError, CommandResult};
use std::path::Path;

pub const TEMPLATE_EXTENSION: &str = "dtool-template";

pub fn read_document(path: &Path) -> Result<Document, CommandError> {
    crate::native_io::read_file(path).map(|(document, _)| document).map_err(|err| err.read_error())
}

pub fn run(workspace: &mut Workspace, scope: Scope, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if !matches!(command.name.as_str(), "doc.template.save" | "doc.template.open") { return None; }
    let mut target = scope.context.or_else(|| workspace.active());
    let result = (|| {
        let path = command.path_param().filter(|path| !path.as_os_str().is_empty()).ok_or_else(CommandError::invalid_params)?;
        if command.name == "doc.template.open" {
            let document = read_document(&path)?;
            let version = document.version;
            let title = path.file_stem().and_then(|name| name.to_str()).unwrap_or(TEMPLATE_EXTENSION);
            let id = workspace.insert_context_with_generation(title, None, document, version, version);
            if let Some(context) = workspace.get_mut(id) {
                context.initial_view_pending = true;
            }
            target = Some(id);
            Ok(serde_json::json!({ "context": id.0, "path": null, "doc_version": version }))
        } else {
            let id = target.ok_or_else(CommandError::no_document)?;
            let path = path.with_extension(TEMPLATE_EXTENSION);
            if workspace.path_clash(id, &path).is_some() { return Err(CommandError::invalid_params()); }
            let context = workspace.get(id).ok_or_else(CommandError::context_not_found)?;
            let document = context.session.document().ok_or_else(CommandError::no_document)?;
            Workspace::write_document(document, &path)?;
            Ok(serde_json::json!({ "path": path, "doc_version": document.version }))
        }
    })();
    Some((target, result))
}
