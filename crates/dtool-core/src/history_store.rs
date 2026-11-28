use crate::undo::{Change, UndoStack};
use dtool_protocol::{Command, CommandError};
use std::borrow::Cow;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub index: u64,
    pub label_key: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryCursor {
    pub index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub id: u64,
    pub name: String,
    pub history_index: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryBudget {
    pub max_steps: u32,
    pub memory_estimate_bytes: u64,
    pub reachable_depth: u32,
}

#[derive(Debug, Clone)]
pub struct DocumentSnapshot {
    pub id: u64,
    pub name: String,
    pub history_index: u64,
    pub document: crate::document::Document,
}

#[derive(Debug, Clone)]
pub struct RecoverPoint {
    pub applied_steps: usize,
    pub after_jump_steps: usize,
    pub total_steps: usize,
    pub document: crate::document::Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushSource {
    History(u64),
    Snapshot(u64),
}

#[derive(Debug, Clone, Default)]
pub struct SessionHistory {
    pub snapshots: Vec<DocumentSnapshot>,
    pub next_snapshot_id: u64,
    pub save_with_document: bool,
    pub recover: Option<RecoverPoint>,
    pub brush_source: Option<(BrushSource, std::sync::Arc<crate::Document>)>,
}

pub fn change_payload_bytes(change: &crate::undo::Change) -> u64 {
    let size = |command: &dtool_protocol::Command| command.name.len() as u64 + serde_json::to_vec(&command.params).map(|bytes| bytes.len() as u64).unwrap_or(0);
    change.name.len() as u64 + size(&change.undo) + size(&change.redo)
}

const MAX_HISTORY_FILE_BYTES: u64 = 64 * 1024 * 1024;

const HISTORY_MAGIC: &[u8] = b"DTOLHIST";
const HISTORY_VERSION_LABELS: u8 = 1;
const HISTORY_VERSION: u8 = 2;
const RECOVERED_MAX_STEPS: u32 = 1024;
pub const HISTORY_SIDECAR_EXTENSION: &str = "dtoolhistory";

pub fn sidecar_path(document: &Path) -> PathBuf {
    let mut name = document.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(".");
    name.push(HISTORY_SIDECAR_EXTENSION);
    document.with_file_name(name)
}

fn change_value(change: &Change) -> serde_json::Value {
    serde_json::json!({ "name": change.name, "redo": change.redo, "undo": change.undo })
}

fn change_from_value(value: serde_json::Value) -> Option<Change> {
    let name = value.get("name")?.as_str()?.to_string();
    let redo: Command = serde_json::from_value(value.get("redo")?.clone()).ok()?;
    let undo: Command = serde_json::from_value(value.get("undo")?.clone()).ok()?;
    if name.is_empty() || redo.name.is_empty() || undo.name.is_empty() {
        return None;
    }
    Some(Change { name, redo, undo })
}

fn history_entries(undo: &UndoStack) -> Vec<HistoryEntry> {
    undo.chronological_changes()
        .enumerate()
        .map(|(index, change)| HistoryEntry {
            index: index as u64,
            label_key: format!("history.{}", change.name),
            label: change.name.clone(),
        })
        .collect()
}

fn io_error() -> CommandError {
    CommandError::validation("IO_ERROR")
}

pub struct HistoryStore<'a> {
    undo: Cow<'a, UndoStack>,
    entries: Vec<HistoryEntry>,
    cursor_index: u64,
    snapshots: Vec<Snapshot>,
    max_steps: u32,
    save_history_with_document: bool,
}

impl<'a> HistoryStore<'a> {
    pub fn from_undo(undo: &'a UndoStack) -> Self {
        Self {
            undo: Cow::Borrowed(undo),
            entries: history_entries(undo),
            cursor_index: undo.applied_steps().saturating_sub(1) as u64,
            snapshots: Vec::new(),
            max_steps: u32::try_from(undo.limit()).unwrap_or(u32::MAX),
            save_history_with_document: false,
        }
    }

    pub fn list(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn cursor(&self) -> HistoryCursor {
        HistoryCursor {
            index: self.cursor_index,
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    pub fn jump(&mut self, index: u64) -> Result<(), CommandError> {
        if index >= self.entries.len() as u64 {
            return Err(CommandError::invalid_params());
        }
        self.cursor_index = index;
        Ok(())
    }

    pub fn add_snapshot(&mut self, name: &str) -> Result<Snapshot, CommandError> {
        let id = self.snapshots.len() as u64 + 1;
        let snapshot = Snapshot {
            id,
            name: name.to_string(),
            history_index: self.cursor_index,
        };
        self.snapshots.push(snapshot.clone());
        Ok(snapshot)
    }

    pub fn restore_snapshot(&mut self, id: u64) -> Result<(), CommandError> {
        let snapshot = self.snapshots.iter().find(|s| s.id == id).ok_or_else(|| {
            CommandError::not_found()
        })?;

        if snapshot.history_index >= self.entries.len() as u64 {
             return Err(CommandError::validation("error.history_index_invalid"));
        }

        self.cursor_index = snapshot.history_index;
        Ok(())
    }

    pub fn layer_from_snapshot(&mut self, id: u64) -> Result<(), CommandError> {
        if !self.snapshots.iter().any(|s| s.id == id) {
            return Err(CommandError::not_found());
        }
        Ok(())
    }

    pub fn set_max_steps(&mut self, n: u32) {
        self.max_steps = n;
    }

    pub fn budget(&self) -> HistoryBudget {
        let applied = u32::try_from(self.undo.applied_steps()).unwrap_or(u32::MAX);
        let reachable_depth = std::cmp::min(applied, self.max_steps);
        let memory_estimate_bytes = self.undo.chronological_changes().map(change_payload_bytes).sum();
        HistoryBudget {
            max_steps: self.max_steps,
            memory_estimate_bytes,
            reachable_depth,
        }
    }

    pub fn undo_stack(&self) -> &UndoStack {
        &self.undo
    }

    pub fn into_undo(self) -> UndoStack {
        self.undo.into_owned()
    }

    pub fn restore_into(self, target: &mut UndoStack) -> usize {
        let limit = target.limit();
        let mut restored = self.into_undo();
        restored.set_limit(limit);
        let reachable = restored.applied_steps();
        *target = restored;
        reachable
    }

    pub fn encode(&self) -> Result<Vec<u8>, CommandError> {
        let changes: Vec<&Change> = self.undo.chronological_changes().collect();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(HISTORY_MAGIC);
        bytes.push(HISTORY_VERSION);
        let count = u32::try_from(changes.len()).map_err(|_| CommandError::validation("error.history_too_large"))?;
        let applied = u32::try_from(self.undo.applied_steps()).map_err(|_| CommandError::validation("error.history_too_large"))?;
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&applied.to_le_bytes());
        for (entry, change) in self.entries.iter().zip(changes) {
            let body = serde_json::to_vec(&change_value(change)).map_err(|_| io_error())?;
            bytes.extend_from_slice(&entry.index.to_le_bytes());
            for part in [entry.label_key.as_bytes(), entry.label.as_bytes(), body.as_slice()] {
                let len = u32::try_from(part.len()).map_err(|_| CommandError::validation("error.history_too_large"))?;
                bytes.extend_from_slice(&len.to_le_bytes());
                bytes.extend_from_slice(part);
            }
        }
        if bytes.len() as u64 > MAX_HISTORY_FILE_BYTES {
            return Err(CommandError::validation("error.history_too_large"));
        }
        Ok(bytes)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), CommandError> {
        let bytes = self.encode()?;
        let mut file = fs::File::create(path).map_err(|_| io_error())?;
        file.write_all(&bytes).map_err(|_| io_error())?;
        file.sync_all().map_err(|_| io_error())
    }

    pub fn save_sidecar(&self, document: &Path) -> Result<PathBuf, CommandError> {
        let target = sidecar_path(document);
        let mut temporary = target.clone().into_os_string();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        let written = self.save_to_path(&temporary).and_then(|_| fs::rename(&temporary, &target).map_err(|_| io_error()));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written.map(|_| target)
    }

    pub fn restore_sidecar_into(document: &Path, session: &mut crate::command::Session) -> serde_json::Value {
        match Self::recover_sidecar(document) {
            Ok(Some(store)) => {
                let restored = store.restore_into(&mut session.undo);
                session.history.save_with_document = true;
                serde_json::json!({ "restored": restored })
            }
            Ok(None) => serde_json::json!({ "restored": 0 }),
            Err(error) => serde_json::json!({ "restored": 0, "error": error.code }),
        }
    }

    pub fn recover_sidecar(document: &Path) -> Result<Option<HistoryStore<'static>>, CommandError> {
        let path = sidecar_path(document);
        if !path.is_file() {
            return Ok(None);
        }
        Self::recover(&path).map(Some)
    }

    pub fn recover(path: &Path) -> Result<HistoryStore<'static>, CommandError> {
        let file = fs::File::open(path).map_err(|_| io_error())?;
        let mut bytes = Vec::new();
        file.take(MAX_HISTORY_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(|_| io_error())?;
        if bytes.len() as u64 > MAX_HISTORY_FILE_BYTES {
            return Err(CommandError::validation("error.history_too_large"));
        }
        Self::decode(&bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<HistoryStore<'static>, CommandError> {
        let mut reader = ByteReader { bytes, at: 0 };
        if reader.take(HISTORY_MAGIC.len()) != Some(HISTORY_MAGIC) {
            return Err(CommandError::validation("error.history_invalid_format"));
        }
        let version = reader.take(1).map(|b| b[0]);
        if version != Some(HISTORY_VERSION) && version != Some(HISTORY_VERSION_LABELS) {
            return Err(CommandError::validation("error.history_invalid_version"));
        }
        let count = reader.u32().ok_or_else(|| CommandError::validation("error.history_truncated"))?;
        if version == Some(HISTORY_VERSION_LABELS) {
            let mut entries = Vec::new();
            for _ in 0..count {
                let Some(entry) = reader.entry()? else { break };
                entries.push(entry);
            }
            return Ok(HistoryStore {
                undo: Cow::Owned(UndoStack::new()),
                entries,
                cursor_index: 0,
                snapshots: Vec::new(),
                max_steps: RECOVERED_MAX_STEPS,
                save_history_with_document: false,
            });
        }
        let applied = reader.u32().ok_or_else(|| CommandError::validation("error.history_truncated"))?;
        let mut changes = Vec::new();
        for position in 0..u64::from(count) {
            let Some(entry) = reader.entry()? else { break };
            let Some(body) = reader.raw() else { break };
            let Some(change) = serde_json::from_slice::<serde_json::Value>(body).ok().and_then(change_from_value) else { break };
            if entry.index != position || entry.label != change.name || entry.label_key != format!("history.{}", change.name) {
                break;
            }
            changes.push(change);
        }
        let verified = changes.len();
        let applied = (applied as usize).min(verified);
        let mut undo = UndoStack::new();
        for change in changes {
            undo.push(change);
        }
        for _ in applied..verified {
            undo.undo();
        }
        Ok(HistoryStore {
            entries: history_entries(&undo),
            cursor_index: applied.saturating_sub(1) as u64,
            undo: Cow::Owned(undo),
            snapshots: Vec::new(),
            max_steps: RECOVERED_MAX_STEPS,
            save_history_with_document: false,
        })
    }
    pub fn save_history_with_document(&self) -> bool {
        self.save_history_with_document
    }

    pub fn set_save_history_with_document(&mut self, enabled: bool) {
        self.save_history_with_document = enabled;
    }
}

struct ByteReader<'b> {
    bytes: &'b [u8],
    at: usize,
}

impl<'b> ByteReader<'b> {
    fn take(&mut self, len: usize) -> Option<&'b [u8]> {
        let end = self.at.checked_add(len)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    fn raw(&mut self) -> Option<&'b [u8]> {
        let len = self.u32()?;
        self.take(len as usize)
    }

    fn text(&mut self) -> Result<Option<String>, CommandError> {
        let Some(raw) = self.raw() else { return Ok(None) };
        String::from_utf8(raw.to_vec()).map(Some).map_err(|_| CommandError::validation("error.history_invalid_encoding"))
    }

    fn entry(&mut self) -> Result<Option<HistoryEntry>, CommandError> {
        let Some(index) = self.u64() else { return Ok(None) };
        let Some(label_key) = self.text()? else { return Ok(None) };
        let Some(label) = self.text()? else { return Ok(None) };
        Ok(Some(HistoryEntry { index, label_key, label }))
    }
}
