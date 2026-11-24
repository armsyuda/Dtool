use crate::batch::{BatchHandle, BatchSnapshot};
use dtool_core::CommandError;
use std::path::PathBuf;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct BatchStore { pub root: PathBuf }

impl Default for BatchStore {
    fn default() -> Self {
        let root = std::env::var_os("APPDATA").filter(|value| !value.is_empty())
            .map(|path| PathBuf::from(path).join("DTool").join("batch"))
            .unwrap_or_else(|| PathBuf::from("batch"));
        Self { root }
    }
}

impl BatchStore {
    pub fn new(root: PathBuf) -> Self { Self { root } }

    fn path(&self, handle: &BatchHandle) -> Result<PathBuf, CommandError> {
        if handle.0.is_empty() || !handle.0.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
            return Err(CommandError::invalid_params());
        }
        Ok(self.root.join(format!("{}.json", handle.0)))
    }

    pub fn load(&self, handle: &BatchHandle) -> Result<Option<BatchSnapshot>, CommandError> {
        let bytes = match std::fs::read(self.path(handle)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(CommandError::new("BATCH_STORE_READ_FAILED")),
        };
        let snapshot: BatchSnapshot = serde_json::from_slice(&bytes).map_err(|_| CommandError::new("BATCH_STORE_INVALID"))?;
        if snapshot.handle != *handle { return Err(CommandError::new("BATCH_STORE_INVALID")); }
        Ok(Some(snapshot))
    }

    pub fn load_all(&self) -> Result<Vec<BatchSnapshot>, CommandError> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(CommandError::new("BATCH_STORE_READ_FAILED")),
        };
        let mut handles = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| CommandError::new("BATCH_STORE_READ_FAILED"))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) { handles.push(BatchHandle(stem.to_owned())); }
        }
        handles.sort_by(|a,b| a.0.cmp(&b.0));
        let mut snapshots = Vec::new();
        for handle in handles { if let Some(snapshot) = self.load(&handle)? { snapshots.push(snapshot); } }
        Ok(snapshots)
    }

    pub fn save(&self, snapshot: &BatchSnapshot) -> Result<(), CommandError> {
        let path = self.path(&snapshot.handle)?;
        let bytes = serde_json::to_vec(snapshot).map_err(|_| CommandError::new("BATCH_STORE_INVALID"))?;
        std::fs::create_dir_all(&self.root).map_err(|_| CommandError::new("BATCH_STORE_WRITE_FAILED"))?;
        let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = self.root.join(format!("{}.{}.{}.tmp", snapshot.handle.0, std::process::id(), sequence));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|_| CommandError::new("BATCH_STORE_WRITE_FAILED"))?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &path)
        })();
        if result.is_err() { let _ = std::fs::remove_file(&temporary); }
        result.map_err(|_| CommandError::new("BATCH_STORE_WRITE_FAILED"))
    }
}
