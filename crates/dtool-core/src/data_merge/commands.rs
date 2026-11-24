use std::path::PathBuf;

use serde_json::{Value, json};

use super::{DataSource, FileFormat, MergeBinding, PageRepeat, RecordRange, SourceKind};
use crate::workspace::{ContextId, Scope, Workspace};
use crate::{Command, CommandError, CommandResult, ObjectId, Session};

pub fn qr_field<'a>(session: &'a Session, object: ObjectId) -> Option<&'a str> {
    session.document.merge_bindings.iter().find_map(|binding| match binding {
        MergeBinding::Qr { object_id, field } if *object_id == object => Some(field.as_str()),
        _ => None,
    })
}

pub fn field_names(session: &Session) -> Vec<&str> {
    let mut fields = Vec::new();
    for source in session.document.merge_sources.iter().filter(|source| source.enabled) {
        for field in &source.fields {
            if !fields.contains(&field.as_str()) {
                fields.push(field.as_str());
            }
        }
    }
    fields
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    matches!(cmd.name.as_str(), "merge.source.add" | "merge.source.query" | "merge.bind")
        .then_some(session.has_document)
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    Some(match cmd.name.as_str() {
        "merge.source.query" => Ok(json!({"sources": describe_sources(session), "fields": field_names(session)})),
        "merge.source.add" => add_source(session, &cmd.params),
        "merge.bind" => bind(session, &cmd.params),
        _ => return None,
    })
}

fn describe_sources(session: &Session) -> Vec<Value> {
    session.document.merge_sources.iter().map(|source| json!({
        "name": source.name,
        "enabled": source.enabled,
        "fields": source.fields,
        "record_count": source.records.len()
    })).collect()
}

fn add_source(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let kind = match (params.get("path"), params.get("kind")) {
        (Some(path), None) => SourceKind::File {
            path: PathBuf::from(path.as_str().ok_or_else(CommandError::invalid_params)?),
        },
        (None, Some(kind)) => serde_json::from_value(kind.clone()).map_err(|_| CommandError::invalid_params())?,
        _ => return Err(CommandError::invalid_params()),
    };
    let SourceKind::File { path } = &kind else { return Err(CommandError::invalid_params()) };
    let name = path.file_name().and_then(|name| name.to_str()).ok_or_else(CommandError::invalid_params)?.to_owned();
    let format = match params.get("format") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?,
        None => match path.extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("csv") => FileFormat::Csv { delimiter: ',', quote: Some('"') },
            Some("tsv") => FileFormat::Csv { delimiter: '\t', quote: Some('"') },
            Some("json") => FileFormat::Json,
            _ => return Err(CommandError::invalid_params()),
        },
    };
    let mut source = DataSource {
        name, kind, format, fields: Vec::new(), records: Vec::new(),
        enabled: true, include_headers: true, numbers_as_text: true, case_sensitive: false,
        records_range: RecordRange::All, pages: PageRepeat::All, rules: Vec::new(), scripts: Vec::new(),
        fetched_hash: String::new(),
    };
    let warnings = super::load_source(&mut source)?;
    if source.fields.is_empty() || source.records.is_empty() {
        return Err(CommandError::new("MERGE_NO_DATA"));
    }
    let mut document = session.document.clone();
    let same = |item: &DataSource| matches!((&item.kind, &source.kind),
        (SourceKind::File { path: existing }, SourceKind::File { path: incoming }) if existing == incoming);
    let reply = json!({"name": source.name, "fields": source.fields, "record_count": source.records.len(), "warnings": warnings});
    if let Some(slot) = document.merge_sources.iter_mut().find(|item| same(item)) {
        *slot = source;
    } else {
        document.merge_sources.push(source);
    }
    crate::commands::document_change::commit(session, "merge.source.add", document, session.selection.ids.clone(), reply)
}

fn bind(session: &mut Session, params: &Value) -> CommandResult {
    let id = ObjectId(params.get("object_id").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?);
    let Some(field) = params.get("field") else { return Err(CommandError::invalid_params()) };
    let field = if field.is_null() { None } else { Some(field.as_str().ok_or_else(CommandError::invalid_params)?) };
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if object.locked || crate::commands::qr::payload(object).is_none() {
        return Err(CommandError::invalid_params());
    }
    if let Some(field) = field {
        if !session.document.merge_sources.iter().any(|source| source.enabled && super::field_value(source, &[], field).is_some()) {
            return Err(CommandError::new("MERGE_BAD_FIELD"));
        }
    }
    if qr_field(session, id) == field {
        return Ok(json!({"object_id": id.0, "field": field, "changes": []}));
    }
    let mut document = session.document.clone();
    document.merge_bindings.retain(|binding| !matches!(binding, MergeBinding::Qr { object_id, .. } if *object_id == id));
    if let Some(field) = field {
        document.merge_bindings.push(MergeBinding::Qr { object_id: id, field: field.to_owned() });
    }
    crate::commands::document_change::commit(session, "merge.bind", document, session.selection.ids.clone(), json!({"object_id": id.0, "field": field}))
}

pub fn run_workspace(workspace: &mut Workspace, scope: Scope, cmd: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if cmd.name != "merge.run" {
        return None;
    }
    let target = scope.context.or(workspace.active());
    let result = (|| {
        let context = workspace.get(target.ok_or_else(CommandError::no_document)?)
            .ok_or_else(CommandError::context_not_found)?;
        let document = context.session.document().ok_or_else(CommandError::no_document)?;
        if !document.merge_bindings.iter().any(|binding| matches!(binding, MergeBinding::Qr { object_id, .. }
            if document.get_object(*object_id).is_some_and(|object| crate::commands::qr::payload(object).is_some())))
        {
            return Err(CommandError::new("MERGE_BAD_FIELD"));
        }
        if document.merge_bindings.iter().any(|binding| matches!(binding, MergeBinding::Image { .. })) {
            return Err(CommandError::new("MERGE_IMAGE_UNAVAILABLE"));
        }
        let title = format!("{} · {}", context.title, dtool_strings::text("qr.merge_result"));
        let (mut output, warnings) = super::run_merge(document)?;
        output.merge_bindings.clear();
        output.merge_layouts.clear();
        output.merge_sources.clear();
        output.version = 1;
        let pages = output.pages.len();
        let id = workspace.insert_context(&title, None, output, 0);
        Ok((id, json!({"context_id": id.0, "pages": pages, "warnings": warnings})))
    })();
    Some(match result {
        Ok((id, value)) => (Some(id), Ok(value)),
        Err(error) => (target, Err(error)),
    })
}
