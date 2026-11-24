use std::path::Path;

use dtool_core::{CommandError, ContextId, Workspace};

use crate::import_ai::admit_ai;
use crate::import_pdf::ImportFormat;
use crate::native::{IoError, IoErrorKind};

pub fn is_ait(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("ait"))
        .unwrap_or(false)
}

pub fn is_psd(path: &Path) -> bool {
    let by_ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("psd"))
        .unwrap_or(false);
    if !by_ext {
        return false;
    }
    match std::fs::read(path) {
        Ok(data) => data.starts_with(b"8BPS"),
        Err(_) => true,
    }
}

pub fn is_indd(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("indd"))
        .unwrap_or(false)
}

pub fn is_pub(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pub"))
        .unwrap_or(false)
}

pub fn admit_other(path: &Path) -> Result<ImportFormat, IoError> {
    if is_ait(path) {
        return admit_ai(path).map(|_| ImportFormat::Ai);
    }
    Err(IoError::new(IoErrorKind::UnsupportedProfile))
}

pub fn open_other(workspace: &mut Workspace, path: &Path) -> Result<ContextId, CommandError> {
    if is_ait(path) {
        return workspace.open_path(path);
    }
    if is_psd(path) {
        return Err(CommandError {
            code: "UNSUPPORTED_PROFILE".into(),
            message_key: "error.format_not_supported".into(),
        });
    }
    if is_indd(path) {
        return Err(CommandError {
            code: "UNSUPPORTED_FORMAT".into(),
            message_key: "error.indd".into(),
        });
    }
    if is_pub(path) {
        return Err(CommandError {
            code: "UNSUPPORTED_FORMAT".into(),
            message_key: "error.pub".into(),
        });
    }
    Err(CommandError::invalid_params())
}

pub fn import_other(workspace: &mut Workspace, path: &Path) -> Result<(), CommandError> {
    if workspace.active().is_none() {
        return Err(CommandError::no_document());
    }
    if is_ait(path) {
        return Ok(());
    }
    if is_psd(path) {
        return Err(CommandError {
            code: "UNSUPPORTED_PROFILE".into(),
            message_key: "error.format_not_supported".into(),
        });
    }
    if is_indd(path) {
        return Err(CommandError {
            code: "UNSUPPORTED_FORMAT".into(),
            message_key: "error.indd".into(),
        });
    }
    if is_pub(path) {
        return Err(CommandError {
            code: "UNSUPPORTED_FORMAT".into(),
            message_key: "error.pub".into(),
        });
    }
    Err(CommandError::invalid_params())
}

