use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use dtool_protocol::CommandError;
use serde_json::Value;

use crate::document::Document;

#[derive(Debug, Clone)]
pub enum NativeIoError {
    Io(std::io::ErrorKind),
    Parse,
    Validate(CommandError),
    Encode,
}

impl NativeIoError {
    fn from_io(err: &std::io::Error) -> Self {
        Self::Io(err.kind())
    }

    pub fn read_error(&self) -> CommandError {
        match self {
            Self::Io(std::io::ErrorKind::NotFound) => CommandError::new("IO.NOT_FOUND"),
            Self::Io(std::io::ErrorKind::PermissionDenied) => CommandError::new("IO.PERMISSION"),
            Self::Io(std::io::ErrorKind::InvalidData) | Self::Parse => CommandError::new("IO.INVALID_FORMAT"),
            Self::Validate(err) => err.clone(),
            Self::Io(_) | Self::Encode => CommandError::new("IO.OTHER"),
        }
    }

    pub fn write_error(&self) -> CommandError {
        match self {
            Self::Io(std::io::ErrorKind::PermissionDenied) => CommandError::validation("IO.PERMISSION"),
            _ => CommandError::validation("IO.OTHER"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeOpen {
    pub document: Document,
    pub file_version: u64,
    pub converted: bool,
    pub report: Option<Value>,
    pub unknown_fields: Value,
}

pub fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

pub fn cleanup_stale_temp(path: &Path) {
    let tmp = temp_path(path);
    if tmp.is_file() {
        let _ = fs::remove_file(&tmp);
    }
}

fn native_value(doc: &Document, extras: &Value) -> Result<Value, NativeIoError> {
    let mut value = serde_json::to_value(doc).map_err(|_| NativeIoError::Encode)?;
    if let (Some(object), Some(extra_map)) = (value.as_object_mut(), extras.as_object()) {
        for (key, extra) in extra_map {
            if !object.contains_key(key) {
                object.insert(key.clone(), extra.clone());
            }
        }
    }
    Ok(value)
}

pub fn native_body(doc: &Document, extras: &Value) -> Result<String, NativeIoError> {
    serde_json::to_string_pretty(&native_value(doc, extras)?).map_err(|_| NativeIoError::Encode)
}

pub fn write_atomic(path: &Path, body: &[u8]) -> Result<(), NativeIoError> {
    let tmp = temp_path(path);
    let written = fs::File::create(&tmp).and_then(|mut file| {
        file.write_all(body)?;
        file.sync_all()
    });
    if let Err(err) = written {
        let _ = fs::remove_file(&tmp);
        return Err(NativeIoError::from_io(&err));
    }
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(NativeIoError::from_io(&err));
    }
    Ok(())
}

pub fn save_file(doc: &Document, extras: &Value, path: &Path) -> Result<(), NativeIoError> {
    let mut value = native_value(doc, extras)?;
    let target = std::path::absolute(path).map_err(|error| NativeIoError::from_io(&error))?;
    if let (Some(directory), Some(resources)) = (target.parent(), value["resources"].as_array_mut()) {
        let canonical_dir = fs::canonicalize(directory).ok();
        for (resource, row) in doc.resources.iter().zip(resources) {
            let Some(source) = resource.path.as_ref().filter(|_| resource.is_linked()) else { continue; };
            let source = std::path::absolute(source).map_err(|error| NativeIoError::from_io(&error))?;
            let relative = source.strip_prefix(directory).ok().map(Path::to_path_buf).or_else(|| {
                let canonical_source = fs::canonicalize(&source).ok()?;
                canonical_source.strip_prefix(canonical_dir.as_deref()?).ok().map(Path::to_path_buf)
            });
            let stored = relative.filter(|path| {
                !path.as_os_str().is_empty() && path.components().all(|part| matches!(part, Component::Normal(_)))
            }).unwrap_or(source);
            row["path"] = serde_json::to_value(stored).map_err(|_| NativeIoError::Encode)?;
        }
    }
    let body = serde_json::to_string_pretty(&value).map_err(|_| NativeIoError::Encode)?;
    write_atomic(path, body.as_bytes())
}

pub fn parse_native(body: &str) -> Result<(Document, Value), NativeIoError> {
    let mut raw: Value = serde_json::from_str(body).map_err(|_| NativeIoError::Parse)?;
    crate::data_merge::drop_unknown_sources(&mut raw);
    let mut document: Document = serde_json::from_value(raw.clone()).map_err(|_| NativeIoError::Parse)?;
    document.validate_and_hydrate().map_err(|err| NativeIoError::Validate(err.into()))?;
    document.migrate_geometry();
    crate::commands::picture_frame::restore_legacy_frame_kinds(&mut document);
    crate::commands::brush::refresh_saved_blend_appearance(&mut document);
    let known = serde_json::to_value(&document).unwrap_or(Value::Null);
    Ok((document, unknown_fields(&raw, &known)))
}

pub fn open_str(body: &str) -> Result<NativeOpen, NativeIoError> {
    let (mut document, unknown_fields) = parse_native(body)?;
    let file_version = document.version;
    let report = match crate::colour::services().read() {
        Ok(services) => crate::colour::convert::open_policy(&mut document, &services),
        Err(_) => None,
    };
    let converted = report.as_ref().and_then(|value| value.get("action")).and_then(Value::as_str) == Some("converted");
    if converted {
        document.version = file_version + 1;
    }
    crate::colour::convert::set_open_report(report.clone());
    Ok(NativeOpen { document, file_version, converted, report, unknown_fields })
}

pub fn read_body(path: &Path) -> Result<String, NativeIoError> {
    cleanup_stale_temp(path);
    fs::read_to_string(path).map_err(|err| NativeIoError::from_io(&err))
}

pub fn open_file(path: &Path) -> Result<NativeOpen, NativeIoError> {
    let mut result = open_str(&read_body(path)?)?;
    resolve_file_links(&mut result.document, path)?;
    Ok(result)
}

pub fn read_file(path: &Path) -> Result<(Document, Value), NativeIoError> {
    let (mut document, extras) = parse_native(&read_body(path)?)?;
    resolve_file_links(&mut document, path)?;
    Ok((document, extras))
}

fn resolve_file_links(document: &mut Document, path: &Path) -> Result<(), NativeIoError> {
    let path = std::path::absolute(path).map_err(|error| NativeIoError::from_io(&error))?;
    if let Some(directory) = path.parent() {
        resolve_image_links(document, directory);
    }
    Ok(())
}

pub(crate) fn resolve_image_links(document: &mut Document, directory: &Path) {
    for resource in &mut document.resources {
        if !resource.embedded && let Some(path) = resource.path.as_mut() && path.is_relative() {
            *path = directory.join(&*path);
        }
    }
}

pub const RETIRED_KEYS: &[&str] = &["snap", "snap_prefs", "snap_enabled", "snap_to_grid", "snap_overlay", "snap_feedback", "overlay"];

fn unknown_fields(raw: &Value, known: &Value) -> Value {
    let (Some(raw_map), Some(known_map)) = (raw.as_object(), known.as_object()) else {
        return Value::Null;
    };
    let extras: serde_json::Map<String, Value> =
        raw_map.iter().filter(|(key, _)| !known_map.contains_key(*key) && !RETIRED_KEYS.contains(&key.as_str())).map(|(key, value)| (key.clone(), value.clone())).collect();
    if extras.is_empty() { Value::Null } else { Value::Object(extras) }
}
