use std::path::Path;

use dtool_core::import_hook::{
    IMPORT_ADMIT, IMPORT_AF_INSPECT, IMPORT_AI, IMPORT_EPS, IMPORT_PDF, IMPORT_PLACE, IMPORT_SVG, format_param, path_param, profile_json,
    set_handler, unsupported_profile,
};
use dtool_core::{Command, CommandError, CommandResult, ContextId, Session, Workspace};
use serde_json::{Map, Value, json};

use crate::af::inspect::FeatureStatus;
use crate::{
    IoError, IoErrorKind, ImportFormat, ImportOptions, ImportProfile, ImportReport, PageSel, admit, admit_ai,
    admit_svg, import_pdf, import_svg, inspect_af,
};

pub const ENGINE_IO: &str = "io";

pub fn install() {
    set_handler(Some(handle));
    crate::image_import::install();
}

pub fn uninstall() {
    set_handler(None);
}

pub fn handle(workspace: &mut Workspace, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    let params = &command.params;
    match command.name.as_str() {
        IMPORT_ADMIT => Some((None, admit_command(params))),
        IMPORT_AF_INSPECT => Some((None, inspect_command(params))),
        IMPORT_PDF => Some(import_command(workspace, params, ImportFormat::Pdf)),
        IMPORT_AI => Some(import_command(workspace, params, ImportFormat::Ai)),
        IMPORT_SVG => Some(import_command(workspace, params, ImportFormat::Svg)),
        IMPORT_EPS => Some(import_eps_command(workspace, params)),
        IMPORT_PLACE => Some((workspace.active(), path_param(params).and_then(|path| crate::import_into_current(workspace, path)))),
        _ => None,
    }
}

fn format_name(format: ImportFormat) -> &'static str {
    match format {
        ImportFormat::Pdf => "pdf",
        ImportFormat::Ai => "ai",
        ImportFormat::Svg => "svg",
    }
}

fn exists(path: &Path) -> Result<(), CommandError> {
    if path.exists() { Ok(()) } else { Err(CommandError::new("IO.NOT_FOUND")) }
}

fn map_error(error: &IoError, password: bool) -> CommandError {
    match error.kind {
        IoErrorKind::UnsupportedProfile => unsupported_profile(),
        IoErrorKind::Permission if password => CommandError::new("WRONG_PASSWORD"),
        IoErrorKind::Permission => CommandError::new("PASSWORD_REQUIRED"),
        IoErrorKind::NotFound => CommandError::new("IO.NOT_FOUND"),
        IoErrorKind::InvalidFormat => CommandError::new("INVALID_FORMAT"),
        _ => CommandError::new(error.code()),
    }
}

fn profile_value(profile: &ImportProfile) -> Value {
    let reason = profile.reason_key.as_deref().map(|reason| match reason {
        "PASSWORD_REQUIRED" => "error.password_required",
        other => other,
    });
    profile_json(
        format_name(profile.format),
        &profile.version,
        profile.encrypted,
        profile.pages,
        profile.supported,
        reason,
        ENGINE_IO,
    )
}

fn admit_command(params: &Value) -> CommandResult {
    let path = path_param(params)?;
    let format = format_param(params, path)?;
    exists(path)?;
    let profile = match format.as_str() {
        "svg" => admit_svg(path),
        "ai" => admit_ai(path),
        _ => admit(path, ImportFormat::Pdf),
    }
    .map_err(|error| match error.kind {
        IoErrorKind::UnsupportedProfile if format == "ai" => {
            CommandError { code: "UNSUPPORTED_PROFILE".into(), message_key: "error.ai.no_pdf".into() }
        }
        _ => map_error(&error, false),
    })?;
    Ok(profile_value(&profile))
}

fn status_name(status: FeatureStatus) -> &'static str {
    match status {
        FeatureStatus::Supported => "supported",
        FeatureStatus::Unsupported => "unsupported",
        FeatureStatus::Unknown => "unknown",
    }
}

fn inspect_command(params: &Value) -> CommandResult {
    let path = path_param(params)?;
    exists(path)?;
    let report = inspect_af(path).map_err(|error| map_error(&error, false))?;
    let mut counts = Map::new();
    let mut names: Vec<&String> = report.node_counts.keys().collect();
    names.sort();
    for name in names {
        counts.insert(name.clone(), json!(report.node_counts[name]));
    }
    let mut features = Map::new();
    let mut tags: Vec<&String> = report.feature_status.keys().collect();
    tags.sort();
    for tag in tags {
        features.insert(tag.clone(), json!(status_name(report.feature_status[tag])));
    }
    Ok(json!({
        "version": report.version,
        "entries": report.entries,
        "node_counts": counts,
        "feature_status": features,
        "unresolved_items": report.unresolved_items,
        "millis": report.millis as u64,
        "peak_bytes": report.peak_bytes,
        "verdict_go": report.verdict_go,
        "reason_key": if report.verdict_go { Value::Null } else { json!("af.report.nogo") },
        "engine": ENGINE_IO,
    }))
}

pub fn options_from(params: &Value) -> Result<ImportOptions, CommandError> {
    let mut options = ImportOptions::default();
    let Some(raw) = params.get("options").filter(|value| !value.is_null()) else {
        return Ok(options);
    };
    if !raw.is_object() {
        return Err(CommandError::invalid_params());
    }
    if let Some(password) = raw.get("password").and_then(Value::as_str) {
        options.password = Some(password.to_string());
    }
    if let Some(dpi) = raw.get("dpi").and_then(Value::as_u64) {
        options.dpi = dpi.min(u32::MAX as u64) as u32;
    }
    match raw.get("pages") {
        None | Some(Value::Null) => {}
        Some(Value::String(all)) if all == "all" => options.pages = PageSel::All,
        Some(Value::Array(range)) if range.len() == 2 => {
            let start = range[0].as_u64().ok_or_else(CommandError::invalid_params)? as u32;
            let end = range[1].as_u64().ok_or_else(CommandError::invalid_params)? as u32;
            options.pages = PageSel::Range(start, end);
        }
        Some(_) => return Err(CommandError::invalid_params()),
    }
    if let Some(flag) = raw.get("favour_editable_text").and_then(Value::as_bool) {
        options.favour_editable_text = flag;
    }
    if let Some(flag) = raw.get("outline_embedded_text").and_then(Value::as_bool) {
        options.outline_embedded_text = flag;
    }
    if let Some(flag) = raw.get("group_text_lines").and_then(Value::as_bool) {
        options.group_text_lines = flag;
    }
    Ok(options)
}

pub fn report_value(report: &ImportReport) -> Value {
    let losses: Vec<Value> = report
        .losses
        .iter()
        .map(|loss| {
            json!({
                "kind": loss.kind.to_string(),
                "count": loss.count,
                "required": loss.required,
                "detail": loss.detail,
            })
        })
        .collect();
    json!({
        "objects": report.objects,
        "texts": report.texts,
        "images": report.images,
        "losses": losses,
        "acceptable": report.is_acceptable(),
        "title_key": "import.report.title",
        "loss_key": "import.report.loss",
    })
}

fn import_command(workspace: &mut Workspace, params: &Value, format: ImportFormat) -> (Option<ContextId>, CommandResult) {
    let result = (|| {
        let path = path_param(params)?;
        exists(path)?;
        let options = options_from(params)?;
        let password = options.password.is_some();
        let imported = match format {
            ImportFormat::Pdf => import_pdf(path, &options),
            ImportFormat::Ai => crate::import_ai::import_ai(path, &options),
            ImportFormat::Svg => import_svg(path),
        };
        let (document, report) = imported.map_err(|error| match error.kind {
            IoErrorKind::UnsupportedProfile if format == ImportFormat::Ai => {
                CommandError { code: "UNSUPPORTED_PROFILE".into(), message_key: "error.ai.no_pdf".into() }
            }
            _ => map_error(&error, password),
        })?;
        let title = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| path.display().to_string());
        Ok((title, document, report))
    })();
    match result {
        Ok((title, document, report)) => {
            let id = workspace.new_document(&title);
            if let Some(context) = workspace.get_mut(id) {
                context.session = Session::new(document);
            }
            let value = json!({
                "context": id.0,
                "title": title,
                "format": format_name(format),
                "report": report_value(&report),
            });
            (Some(id), Ok(value))
        }
        Err(error) => (None, Err(error)),
    }
}

fn import_eps_command(workspace: &mut Workspace, params: &Value) -> (Option<ContextId>, CommandResult) {
    let result = (|| {
        let path = path_param(params)?;
        exists(path)?;
        let (document, report) = crate::import_eps(path).map_err(|error| map_error(&error, false))?;
        let title = path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string).unwrap_or_else(|| path.display().to_string());
        Ok((title, document, report))
    })();
    match result {
        Ok((title, document, report)) => {
            let id = workspace.new_document(&title);
            if let Some(context) = workspace.get_mut(id) {
                context.session = Session::new(document);
            }
            (Some(id), Ok(json!({ "context": id.0, "title": title, "format": "eps", "report": report_value(&report) })))
        }
        Err(error) => (None, Err(error)),
    }
}
