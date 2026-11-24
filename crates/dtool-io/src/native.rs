use std::path::{Path, PathBuf};

use dtool_core::document::Document;
use dtool_core::native_io::{NativeIoError, NativeOpen};
use dtool_core::table::model::TableNode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const NATIVE_EXTENSION: &str = "dtool";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoErrorKind {
    NotFound,
    Permission,
    InvalidFormat,
    UnsupportedProfile,
    Other,
}

impl IoErrorKind {
    pub fn message_key(&self) -> &'static str {
        match self {
            IoErrorKind::NotFound => "error.io.not_found",
            IoErrorKind::Permission => "error.io.permission",
            IoErrorKind::InvalidFormat => "error.io.invalid_format",
            IoErrorKind::UnsupportedProfile => "error.unsupported_profile",
            IoErrorKind::Other => "error.io.other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoError {
    pub kind: IoErrorKind,
    pub message_key: String,
}

impl IoError {
    pub fn new(kind: IoErrorKind) -> Self {
        Self {
            kind,
            message_key: kind.message_key().to_string(),
        }
    }

    pub fn from_io(err: &std::io::Error) -> Self {
        let kind = match err.kind() {
            std::io::ErrorKind::NotFound => IoErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => IoErrorKind::Permission,
            std::io::ErrorKind::InvalidData => IoErrorKind::InvalidFormat,
            _ => IoErrorKind::Other,
        };
        Self::new(kind)
    }

    pub fn code(&self) -> &'static str {
        match self.kind {
            IoErrorKind::NotFound => "NOT_FOUND",
            IoErrorKind::Permission => "PERMISSION_DENIED",
            IoErrorKind::InvalidFormat => "INVALID_FORMAT",
            IoErrorKind::UnsupportedProfile => "UNSUPPORTED_PROFILE",
            IoErrorKind::Other => "IO_ERROR",
        }
    }
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message_key)
    }
}

impl std::error::Error for IoError {}

#[derive(Debug, Clone)]
pub struct OpenOutcome {
    pub document: Document,
    pub file_version: u64,
    pub converted: bool,
    pub report: Option<Value>,
    pub unknown_fields: Value,
}

pub fn temp_path(path: &Path) -> PathBuf {
    dtool_core::native_io::temp_path(path)
}

pub fn cleanup_stale_temp(path: &Path) {
    dtool_core::native_io::cleanup_stale_temp(path)
}

fn io_error(err: NativeIoError) -> IoError {
    match err {
        NativeIoError::Io(kind) => IoError::from_io(&std::io::Error::from(kind)),
        NativeIoError::Parse | NativeIoError::Validate(_) => IoError::new(IoErrorKind::InvalidFormat),
        NativeIoError::Encode => IoError::new(IoErrorKind::Other),
    }
}

pub fn save(doc: &Document, path: &Path) -> Result<(), IoError> {
    save_with_extras(doc, &Value::Null, path)
}

pub fn save_with_extras(doc: &Document, extras: &Value, path: &Path) -> Result<(), IoError> {
    dtool_core::native_io::save_file(doc, extras, path).map_err(io_error)
}

pub fn open(path: &Path) -> Result<Document, IoError> {
    Ok(open_with_status(path)?.document)
}

pub fn open_with_status(path: &Path) -> Result<OpenOutcome, IoError> {
    dtool_core::native_io::open_file(path).map(outcome).map_err(io_error)
}

pub fn open_from_str(body: &str) -> Result<OpenOutcome, IoError> {
    dtool_core::native_io::open_str(body).map(outcome).map_err(io_error)
}

fn outcome(opened: NativeOpen) -> OpenOutcome {
    OpenOutcome {
        document: opened.document,
        file_version: opened.file_version,
        converted: opened.converted,
        report: opened.report,
        unknown_fields: opened.unknown_fields,
    }
}

pub fn save_table(table: &TableNode) -> Value {
    serde_json::to_value(table).unwrap_or(Value::Null)
}

pub fn load_table(value: &Value) -> Result<TableNode, String> {
    let mut table: TableNode = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let max_id = table.cells.iter().map(|c| c.id.0).max().unwrap_or(0);
    if max_id > table.alloc.high_water {
        table.alloc.high_water = max_id;
    }
    Ok(table)
}

pub const TEMPLATE_EXTENSION: &str = "dtool-template";

pub fn save_template(doc: &Document, path: &Path) -> Result<(), IoError> {
    save(doc, &path.with_extension(TEMPLATE_EXTENSION))
}

pub fn open_template(path: &Path) -> Result<Document, IoError> {
    open(path)
}
