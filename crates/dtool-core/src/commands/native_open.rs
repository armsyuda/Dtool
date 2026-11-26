use crate::workspace::{ContextId, Workspace};
use dtool_protocol::{Command, CommandError, CommandResult};

pub fn run(workspace: &mut Workspace, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if !matches!(command.name.as_str(), "open" | "doc.open" | "doc.open_recent") { return None; }
    let path = command.path_param();
    let ext = path.as_deref().and_then(|path| path.extension()).map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    let import_name = match ext.as_deref() {
        Some("pdf") => Some(crate::import_hook::IMPORT_PDF),
        Some("svg") | Some("svgz") => Some(crate::import_hook::IMPORT_SVG),
        Some("ai") => Some(crate::import_hook::IMPORT_AI),
        Some("eps") => Some(crate::import_hook::IMPORT_EPS),
        _ => None,
    };
    if let Some(name) = import_name {
        if crate::import_hook::handler().is_none() {
            return Some((None, Err(CommandError::not_implemented())));
        }
        let import = Command { name: name.to_string(), params: command.params.clone() };
        let outcome = crate::import_hook::run(workspace, &import)?;
        if outcome.1.is_ok() && let Some(path) = &path {
            workspace.push_recent(path);
        }
        return Some(outcome);
    }
    let previous_count = workspace.contexts().len();
    let result = path.as_deref().filter(|path| !path.as_os_str().is_empty()).ok_or_else(CommandError::invalid_params)
        .and_then(|path| workspace.open_path(path));
    Some(match result {
        Ok(id) => {
            let mut reply = serde_json::json!({ "context": id.0, "history": { "restored": 0 } });
            if workspace.contexts().len() > previous_count
                && let (Some(path), Some(context)) = (path.as_deref(), workspace.get_mut(id))
            {
                reply["history"] = crate::history_store::HistoryStore::restore_sidecar_into(path, &mut context.session);
                context.reconnect_saved_history();
            }
            if let Some(report) = crate::colour::convert::take_open_report() {
                reply["colour_open"] = report;
            }
            (Some(id), Ok(reply))
        }
        Err(error) => (None, Err(error)),
    })
}
