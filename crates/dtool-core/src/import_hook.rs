use std::path::Path;
use std::sync::RwLock;
use std::time::Instant;

use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{Value, json};

use crate::workspace::{ContextId, Workspace};

pub const IMPORT_ADMIT: &str = "import.admit";
pub const IMPORT_PDF: &str = "import.pdf";
pub const IMPORT_AI: &str = "import.ai";
pub const IMPORT_SVG: &str = "import.svg";
pub const IMPORT_AF_INSPECT: &str = "import.af.inspect";
pub const IMPORT_EPS: &str = "import.eps";
pub const IMPORT_PLACE: &str = "import.place";
pub const IMPORT_COMMANDS: [&str; 7] = [IMPORT_ADMIT, IMPORT_PDF, IMPORT_AI, IMPORT_SVG, IMPORT_EPS, IMPORT_AF_INSPECT, IMPORT_PLACE];
pub const AF_MAGIC: u32 = 0x496E666E;
pub const ENGINE_CORE: &str = "core";

pub type ImportHandler = fn(&mut Workspace, &Command) -> Option<(Option<ContextId>, CommandResult)>;

static HANDLER: RwLock<Option<ImportHandler>> = RwLock::new(None);

pub fn set_handler(handler: Option<ImportHandler>) {
    if let Ok(mut slot) = HANDLER.write() {
        *slot = handler;
    }
}

pub fn handler() -> Option<ImportHandler> {
    HANDLER.read().ok().and_then(|slot| *slot)
}

pub fn is_import(name: &str) -> bool {
    IMPORT_COMMANDS.contains(&name)
}

pub fn run(workspace: &mut Workspace, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if !is_import(&command.name) {
        return None;
    }
    if let Some(handler) = handler() {
        if let Some(result) = handler(workspace, command) {
            return Some(result);
        }
    }
    let result = match command.name.as_str() {
        IMPORT_ADMIT => admit_command(&command.params),
        IMPORT_AF_INSPECT => inspect_command(&command.params),
        _ => Err(not_available()),
    };
    Some((None, result))
}

pub fn not_available() -> CommandError {
    CommandError { code: "NOT_AVAILABLE".into(), message_key: "error.import.not_available".into() }
}

pub fn unsupported_profile() -> CommandError {
    CommandError { code: "UNSUPPORTED_PROFILE".into(), message_key: "error.unsupported_profile".into() }
}

pub fn path_param(params: &Value) -> Result<&Path, CommandError> {
    params
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(Path::new)
        .ok_or_else(CommandError::invalid_params)
}

pub fn format_param(params: &Value, path: &Path) -> Result<String, CommandError> {
    let format = match params.get("format").and_then(Value::as_str) {
        Some(format) => format.to_ascii_lowercase(),
        None => path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(CommandError::invalid_params)?,
    };
    match format.as_str() {
        "pdf" | "ai" | "svg" => Ok(format),
        "svgz" => Ok("svg".to_string()),
        _ => Err(unsupported_profile()),
    }
}

pub fn read_error(error: &std::io::Error) -> CommandError {
    match error.kind() {
        std::io::ErrorKind::NotFound => CommandError::new("IO.NOT_FOUND"),
        std::io::ErrorKind::PermissionDenied => CommandError::new("IO.PERMISSION"),
        _ => CommandError::new("IO.OTHER"),
    }
}

pub fn profile_json(format: &str, version: &str, encrypted: bool, pages: u32, supported: bool, reason_key: Option<&str>, engine: &str) -> Value {
    json!({
        "format": format,
        "version": version,
        "encrypted": encrypted,
        "pages": pages,
        "supported": supported,
        "reason_key": reason_key,
        "engine": engine,
    })
}

fn admit_command(params: &Value) -> CommandResult {
    let path = path_param(params)?;
    let format = format_param(params, path)?;
    let bytes = std::fs::read(path).map_err(|error| read_error(&error))?;
    match format.as_str() {
        "svg" => admit_svg_bytes(&bytes),
        "ai" => {
            if bytes.starts_with(b"%PDF-") {
                admit_pdf_bytes(&bytes, "ai")
            } else {
                Err(CommandError { code: "UNSUPPORTED_PROFILE".into(), message_key: "error.ai.no_pdf".into() })
            }
        }
        _ => admit_pdf_bytes(&bytes, "pdf"),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn count_pages(bytes: &[u8]) -> u32 {
    let mut count = 0u32;
    for needle in [&b"/Type /Page"[..], &b"/Type/Page"[..]] {
        let mut start = 0usize;
        while let Some(found) = find(&bytes[start..], needle) {
            let end = start + found + needle.len();
            let next = bytes.get(end).copied().unwrap_or(b' ');
            if !next.is_ascii_alphanumeric() {
                count += 1;
            }
            start = end;
        }
    }
    count
}

pub fn pdf_version(bytes: &[u8]) -> Option<String> {
    let rest = bytes.strip_prefix(b"%PDF-")?;
    let end = rest
        .iter()
        .position(|byte| byte.is_ascii_whitespace() || *byte == b'%')
        .unwrap_or(rest.len().min(8));
    let version = std::str::from_utf8(&rest[..end]).ok()?.trim().to_string();
    if version.is_empty() { None } else { Some(version) }
}

fn admit_pdf_bytes(bytes: &[u8], format: &str) -> CommandResult {
    let version = pdf_version(bytes).ok_or_else(unsupported_profile)?;
    let tail_start = bytes.len().saturating_sub(2048);
    if find(&bytes[tail_start..], b"%%EOF").is_none() || find(bytes, b"startxref").is_none() {
        return Err(unsupported_profile());
    }
    if find(bytes, b"/Encrypt").is_some() {
        return Ok(profile_json(format, &version, true, 0, false, Some("error.password_required"), ENGINE_CORE));
    }
    let pages = count_pages(bytes);
    if pages == 0 {
        if find(bytes, b"/ObjStm").is_some() {
            return Err(not_available());
        }
        return Err(unsupported_profile());
    }
    Ok(profile_json(format, &version, false, pages, true, None, ENGINE_CORE))
}

fn admit_svg_bytes(bytes: &[u8]) -> CommandResult {
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let Some(start) = text.find("<svg") else {
        return Err(unsupported_profile());
    };
    let tag_end = text[start..].find('>').map(|end| start + end).unwrap_or(text.len());
    let tag = &text[start..tag_end];
    let version = tag
        .find("version=\"")
        .and_then(|at| {
            let rest = &tag[at + 9..];
            rest.find('"').map(|end| rest[..end].to_string())
        })
        .unwrap_or_else(|| "1.1".to_string());
    Ok(profile_json("svg", &version, false, 1, true, None, ENGINE_CORE))
}

fn le_u16(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn le_u32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn le_u64(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8).map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfEntryInfo {
    pub name: String,
    pub stored_size: u64,
    pub size: u64,
    pub compression: u8,
}

fn read_entry_info(data: &[u8], pos: &mut usize) -> Option<AfEntryInfo> {
    let name_len = *data.get(*pos)? as usize;
    *pos += 1;
    if name_len == 0 {
        return None;
    }
    let name = String::from_utf8_lossy(data.get(*pos..*pos + name_len)?).to_string();
    *pos += name_len;
    let _offset = le_u64(data, *pos)?;
    let stored_size = le_u64(data, *pos + 8)?;
    let size = le_u64(data, *pos + 16)?;
    let _crc = le_u32(data, *pos + 24)?;
    let compression = *data.get(*pos + 28)?;
    *pos += 29;
    if compression > 2 {
        return None;
    }
    Some(AfEntryInfo { name, stored_size, size, compression })
}

pub fn af_container(data: &[u8]) -> Result<(u16, Vec<AfEntryInfo>), CommandError> {
    let invalid = || CommandError::new("INVALID_FORMAT");
    if le_u32(data, 0) != Some(AF_MAGIC) {
        return Err(invalid());
    }
    let version = le_u16(data, 4).ok_or_else(invalid)?;
    let marker = data.get(16..20).ok_or_else(invalid)?;
    let mut entries = Vec::new();
    if version >= 8 {
        if marker != b"torP" {
            return Err(invalid());
        }
        let fat_size = le_u32(data, 20).ok_or_else(invalid)? as usize;
        let fat = data.get(24..24 + fat_size).ok_or_else(invalid)?;
        let mut pos = 0usize;
        while pos + 4 <= fat.len() {
            entries.push(read_entry_info(fat, &mut pos).ok_or_else(invalid)?);
        }
    } else {
        if marker != b"fnI#" {
            return Err(invalid());
        }
        let count = le_u32(data, 20).ok_or_else(invalid)? as usize;
        let mut pos = 24usize;
        for _ in 0..count {
            entries.push(read_entry_info(data, &mut pos).ok_or_else(invalid)?);
        }
    }
    Ok((version, entries))
}

fn inspect_command(params: &Value) -> CommandResult {
    let start = Instant::now();
    let path = path_param(params)?;
    let data = std::fs::read(path).map_err(|error| read_error(&error))?;
    let (version, entries) = af_container(&data)?;
    let unresolved: Vec<String> = entries.iter().map(|entry| format!("decoder_unavailable:{}", entry.name)).collect();
    let peak = entries.iter().map(|entry| entry.stored_size as usize).fold(data.len(), usize::max);
    Ok(json!({
        "version": version.to_string(),
        "entries": entries.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>(),
        "node_counts": {},
        "feature_status": { "container": "supported", "tree": "unknown" },
        "unresolved_items": unresolved,
        "millis": start.elapsed().as_millis() as u64,
        "peak_bytes": peak,
        "verdict_go": false,
        "reason_key": "af.report.nogo",
        "engine": ENGINE_CORE,
    }))
}