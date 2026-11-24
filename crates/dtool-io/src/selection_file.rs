use std::path::{Path, PathBuf};

use dtool_core::selection::{
    decode_afselection, encode_afselection, read_afselection_file, with_afselection_extension,
    write_afselection_file, PixelSelection, AFSELECTION_EXT,
};
use dtool_core::CommandError;

use crate::{IoError, IoErrorKind};

pub const SELECTION_FILE_EXTENSION: &str = AFSELECTION_EXT;

fn map_command_error(err: CommandError) -> IoError {
    let kind = match err.code.as_str() {
        "IO.NOT_FOUND" | "NOT_FOUND" => IoErrorKind::NotFound,
        "IO.PERMISSION" | "PERMISSION_DENIED" => IoErrorKind::Permission,
        "IO.INVALID_FORMAT" | "INVALID_FORMAT" => IoErrorKind::InvalidFormat,
        _ => IoErrorKind::Other,
    };
    IoError::new(kind)
}

pub fn save_selection(path: &Path, selection: &PixelSelection) -> Result<PathBuf, IoError> {
    write_afselection_file(path, selection).map_err(map_command_error)
}

pub fn load_selection(path: &Path) -> Result<PixelSelection, IoError> {
    read_afselection_file(path).map_err(map_command_error)
}

pub fn selection_file_bytes(selection: &PixelSelection) -> Vec<u8> {
    encode_afselection(selection)
}

pub fn selection_from_bytes(bytes: &[u8]) -> Result<PixelSelection, IoError> {
    decode_afselection(bytes).map_err(map_command_error)
}

pub fn with_extension(path: &Path) -> PathBuf {
    with_afselection_extension(path)
}
