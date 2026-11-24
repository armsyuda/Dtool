use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dtool_protocol::CommandError;
use serde::{Deserialize, Serialize};

pub const CMD_AUTOSAVE_SET: &str = "pref.autosave.set";
pub const DOC_SAVE: &str = "doc.save";
pub const TITLE_KEY: &str = "pref.autosave";
pub const DEFAULT_RECOVERY_INTERVAL_S: u32 = 300;
pub const RECOVERY_EXTENSION: &str = "dtoolrecovery";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutosaveSettings {
    pub enabled: bool,
    pub recovery_interval_s: u32,
}

impl Default for AutosaveSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            recovery_interval_s: DEFAULT_RECOVERY_INTERVAL_S,
        }
    }
}

impl AutosaveSettings {
    pub fn from_prefs(recovery_interval_s: u32) -> Self {
        Self {
            enabled: recovery_interval_s > 0,
            recovery_interval_s,
        }
    }

    pub fn is_active(&self) -> bool {
        self.enabled && self.recovery_interval_s > 0
    }

    pub fn interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.recovery_interval_s))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutosaveSetArgs {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, alias = "recovery_interval_s")]
    pub interval_s: Option<u32>,
}

pub fn recovery_dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join("DTool").join("recovery"),
        None => std::env::temp_dir().join("DTool").join("recovery"),
    }
}

fn file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "untitled".to_string()
    } else {
        cleaned
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

#[derive(Debug, Clone)]
pub struct Autosave {
    pub settings: AutosaveSettings,
    pub dir: PathBuf,
    pub saved_path: Option<PathBuf>,
    pub last_file: Option<PathBuf>,
    last_mark: Instant,
    written_generation: u64,
}

impl Default for Autosave {
    fn default() -> Self {
        Self::new()
    }
}

impl Autosave {
    pub fn new() -> Self {
        Self::with_dir(recovery_dir())
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            settings: AutosaveSettings::from_prefs(crate::prefs::Prefs::load(&crate::prefs::Prefs::default_path()).recovery_interval_s),
            dir,
            saved_path: None,
            last_file: None,
            last_mark: Instant::now(),
            written_generation: 0,
        }
    }

    pub fn set(&mut self, args: AutosaveSetArgs) -> AutosaveSettings {
        if let Some(interval) = args.interval_s {
            self.settings.recovery_interval_s = interval;
            if interval == 0 {
                self.settings.enabled = false;
            }
        }
        if let Some(enabled) = args.enabled {
            self.settings.enabled = enabled && self.settings.recovery_interval_s > 0;
        }
        self.settings
    }

    pub fn run_set(&mut self, params: &serde_json::Value) -> Result<AutosaveSettings, CommandError> {
        let args: AutosaveSetArgs = if params.is_null() {
            AutosaveSetArgs::default()
        } else {
            serde_json::from_value(params.clone()).map_err(|_| CommandError::new("INVALID_PARAMS"))?
        };
        Ok(self.set(args))
    }

    pub fn observe(&mut self, command_name: &str, path: Option<&Path>, generation: u64) {
        if command_name == DOC_SAVE {
            if let Some(path) = path {
                self.saved_path = Some(path.to_path_buf());
            }
            self.written_generation = generation;
            self.last_mark = Instant::now();
        }
    }

    pub fn recovery_path(&self, name: &str) -> PathBuf {
        self.dir.join(file_stem(name)).with_extension(RECOVERY_EXTENSION)
    }

    pub fn due(&self, generation: u64, now: Instant) -> bool {
        self.settings.is_active()
            && generation != self.written_generation
            && now.saturating_duration_since(self.last_mark) >= self.settings.interval()
    }

    pub fn tick(
        &mut self,
        generation: u64,
        now: Instant,
        name: &str,
        own_path: Option<&Path>,
        contents: impl FnOnce() -> String,
    ) -> Result<Option<PathBuf>, CommandError> {
        if !self.due(generation, now) {
            return Ok(None);
        }
        let target = self.recovery_path(name);
        if self.protects(&target, own_path) {
            return Err(CommandError::new("PATH_NOT_ALLOWED"));
        }
        std::fs::create_dir_all(&self.dir).map_err(|_| CommandError::new("IO_ERROR"))?;
        let document: crate::document::Document = serde_json::from_str(&contents())
            .map_err(|_| CommandError::new("IO_ERROR"))?;
        crate::workspace::Workspace::write_document(&document, &target)?;
        let metadata = target.with_extension("meta.json");
        if self.protects(&metadata, own_path) { return Err(CommandError::new("PATH_NOT_ALLOWED")); }
        let original = own_path.or(self.saved_path.as_deref());
        let value = serde_json::json!({ "original_path": original });
        let body = serde_json::to_vec(&value).map_err(|_| CommandError::new("IO_ERROR"))?;
        let temporary = metadata.with_extension("json.tmp");
        let written = std::fs::write(&temporary, body).and_then(|_| std::fs::rename(&temporary, &metadata));
        if written.is_err() { let _ = std::fs::remove_file(&temporary); return Err(CommandError::new("IO_ERROR")); }
        self.written_generation = generation;
        self.last_mark = now;
        self.last_file = Some(target.clone());
        Ok(Some(target))
    }

    pub fn write_history(
        &self,
        history: &crate::history_store::HistoryStore<'_>,
        recovery_file: &Path,
        own_path: Option<&Path>,
    ) -> Result<PathBuf, CommandError> {
        let target = crate::history_store::sidecar_path(recovery_file);
        if self.protects(&target, own_path) || self.protects(recovery_file, own_path) {
            return Err(CommandError::new("PATH_NOT_ALLOWED"));
        }
        history.save_sidecar(recovery_file)
    }

    fn protects(&self, target: &Path, own_path: Option<&Path>) -> bool {
        own_path
            .into_iter()
            .chain(self.saved_path.as_deref())
            .any(|path| same_path(path, target))
    }
}

#[derive(Debug, Clone)]
pub struct RecoveryCandidate {
    pub path: PathBuf,
    pub original_path: Option<PathBuf>,
    pub modified: std::time::SystemTime,
}

pub fn recovery_candidates(dir: &Path) -> Result<Vec<RecoveryCandidate>, CommandError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(CommandError::new("IO_ERROR")),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| CommandError::new("IO_ERROR"))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some(RECOVERY_EXTENSION) || !path.is_file() { continue; }
        let Some(modified) = entry.metadata().ok().and_then(|metadata| metadata.modified().ok()) else { continue; };
        let original_path = std::fs::read(path.with_extension("meta.json")).ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value.get("original_path").and_then(serde_json::Value::as_str).map(PathBuf::from));
        let saved = original_path.as_ref().and_then(|path| std::fs::metadata(path).ok()).and_then(|metadata| metadata.modified().ok());
        if saved.is_some_and(|saved| modified <= saved) { continue; }
        let valid = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<crate::document::Document>(&bytes).ok()).is_some();
        if valid { candidates.push(RecoveryCandidate { path, original_path, modified }); }
    }
    candidates.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    Ok(candidates)
}

pub fn recovered_history(recovery_file: &Path) -> Result<Option<crate::history_store::HistoryStore<'static>>, CommandError> {
    crate::history_store::HistoryStore::recover_sidecar(recovery_file)
}