use super::{insert_definition, validate_definition, FormatDependencies, TableFormatDef, TableFormatId};
use crate::document::Units;
use crate::native_io::{self, NativeIoError};
use crate::{Command, CommandError, CommandResult, Document, ObjectKind, Session};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;

pub const DEFAULT_SAVE: &str = "style.table.default.save";
pub const DEFAULT_RESET: &str = "style.table.default.reset";

pub(super) fn run(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    if command.name == DEFAULT_SAVE {
        return save(&session.document, &command.params, session.generation);
    }
    let Some(source) = load()? else {
        return Ok(json!({"doc_version": session.generation}));
    };
    let mut after = session.document.clone();
    let used: HashSet<_> = after.objects.iter().filter_map(|object| match &object.kind {
        ObjectKind::Table(table) => table.format,
        _ => None,
    }).collect();
    after.table_formats.retain(|format| {
        used.contains(&format.id) || source.table_formats.iter().any(|saved| saved.name == format.name)
    });
    install(&source, &mut after)?;
    if after.default_table_format == session.document.default_table_format
        && serde_json::to_value(&after.table_formats).ok() == serde_json::to_value(&session.document.table_formats).ok()
    {
        return Ok(json!({"doc_version": session.generation}));
    }
    crate::commands::document_change::commit(session, &command.name, after, session.selection.ids.clone(), json!({}))
}

fn path() -> Option<PathBuf> {
    crate::defaults::ObjectDefaults::settings_path().ok().map(|path| path.with_file_name("table-formats.dtool"))
}

fn load() -> Result<Option<Document>, CommandError> {
    let Some(path) = path() else { return Ok(None); };
    // Read only the published file. A different app instance may be saving a replacement.
    match fs::read_to_string(path) {
        Ok(body) => native_io::parse_native(&body).map(|(document, _)| Some(document)).map_err(|error| error.read_error()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(NativeIoError::Io(error.kind()).read_error()),
    }
}

fn save(source: &Document, params: &Value, generation: u64) -> CommandResult {
    let mut library = Document::new([1.0, 1.0], Units::Points, 72.0, 1);
    let default_id = match params.get("id") {
        Some(value) => {
            let id = TableFormatId(value.as_u64().ok_or_else(CommandError::invalid_params)?);
            if !source.table_formats.iter().any(|format| format.id == id) {
                return Err(CommandError::not_found());
            }
            Some(id)
        }
        None => source.default_table_format,
    };
    copy_library(source, &mut library, default_id)?;
    let path = path().ok_or_else(|| CommandError::new("IO.OTHER"))?;
    let io_error = |error: std::io::Error| NativeIoError::Io(error.kind()).write_error();
    fs::create_dir_all(path.parent().ok_or_else(CommandError::invalid_params)?).map_err(io_error)?;
    let lock = OpenOptions::new().write(true).create(true).truncate(false)
        .open(path.with_extension("lock")).map_err(io_error)?;
    lock.lock().map_err(io_error)?;
    native_io::save_file(&library, &Value::Null, &path).map_err(|error| error.write_error())?;
    Ok(json!({"count": library.table_formats.len(), "id": default_id, "doc_version": generation}))
}

pub(crate) fn seed_new_document(document: &mut Document) {
    if let Ok(Some(source)) = load() {
        let _ = install(&source, document);
    }
}

fn install(source: &Document, document: &mut Document) -> Result<(), CommandError> {
    let mut updated = document.clone();
    copy_library(source, &mut updated, source.default_table_format)?;
    *document = updated;
    Ok(())
}

fn copy_library(source: &Document, target: &mut Document, default: Option<TableFormatId>) -> Result<(), CommandError> {
    let mut dependencies = FormatDependencies { reuse_images: true, ..Default::default() };
    let mut copied_default = None;
    for format in &source.table_formats {
        let definition = dependencies.copy(source, target, format)?;
        let id = if let Some(existing) = target.table_formats.iter().find(|existing| existing.name == format.name) {
            update_definition(target, definition, existing.id)?
        } else {
            insert_definition(target, definition, false, &format.name)?
        };
        if default == Some(format.id) {
            copied_default = Some(id);
        }
    }
    target.default_table_format = copied_default;
    Ok(())
}

fn update_definition(target: &mut Document, mut definition: TableFormatDef, id: TableFormatId) -> Result<TableFormatId, CommandError> {
    validate_definition(&definition)?;
    let existing = target.table_formats.iter_mut().find(|existing| existing.id == id).ok_or_else(CommandError::not_found)?;
    definition.id = id;
    if definition.cell_formats.len() == existing.cell_formats.len() {
        let mut mapping = std::collections::HashMap::new();
        for ((incoming, _, _), (current, _, _)) in definition.cell_formats.iter_mut().zip(&existing.cell_formats) {
            mapping.insert(*incoming as u16, *current as u16);
            *incoming = *current;
        }
        for slot in &mut definition.pattern.slots {
            *slot = mapping[slot];
        }
        *existing = definition;
        Ok(id)
    } else {
        let name = definition.name.clone();
        insert_definition(target, definition, true, &name)
    }
}
