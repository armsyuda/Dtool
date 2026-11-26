use crate::dialogs::{show_recent_missing_dialog, RecentMissingDialogState};
use dtool_core::Workspace;
use dtool_protocol::CommandError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MENU_ID: &str = "file.open_recent";
pub const CLEAR_ID: &str = "file.open_recent.clear";
pub const MORE_ID: &str = "file.open_recent.more";
pub const INLINE_ROWS: usize = 10;
pub const ITEM_PREFIX: &str = "file.open_recent.item.";
pub const FILE_NAME: &str = "recent_files.json";

pub fn default_path() -> PathBuf {
    dtool_core::prefs::Prefs::default_path().with_file_name(FILE_NAME)
}

pub fn load(path: &Path) -> Vec<PathBuf> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    value
        .get("recent")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(PathBuf::from).collect())
        .unwrap_or_default()
}

pub fn save(path: &Path, recent: &[PathBuf]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let items: Vec<String> = recent.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let body = serde_json::to_string_pretty(&json!({ "recent": items }))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
    std::fs::write(path, body)
}

pub fn restore(workspace: &mut Workspace, recent: &[PathBuf]) {
    for path in recent.iter().rev() {
        workspace.push_recent(path);
    }
}

pub fn item_id(index: usize) -> String {
    format!("{}{}", ITEM_PREFIX, index)
}

pub fn item_index(id: &str) -> Option<usize> {
    id.strip_prefix(ITEM_PREFIX)?.parse().ok()
}

pub fn item_label(index: usize, path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    format!("{} {}", index + 1, name)
}

fn item_row(index: usize, path: &Path) -> Value {
    json!({
        "id": item_id(index),
        "label": item_label(index, path),
        "path": path.to_string_lossy(),
        "missing": !path.exists(),
        "enabled": true,
        "planned": false,
    })
}

pub fn inject_rows(tree: &mut Value, recent: &[PathBuf]) -> bool {
    match tree {
        Value::Array(items) => items.iter_mut().any(|item| inject_rows(item, recent)),
        Value::Object(map) => {
            if map.get("id").and_then(Value::as_str) == Some(MENU_ID) {
                let children = map.entry("children").or_insert_with(|| Value::Array(Vec::new()));
                if !children.is_array() {
                    *children = Value::Array(Vec::new());
                }
                if let Value::Array(rows) = children {
                    rows.retain(|row| row.get("id").and_then(Value::as_str).map_or(true, |id| item_index(id).is_none()));
                    let all: Vec<Value> = recent.iter().enumerate().map(|(index, path)| item_row(index, path)).collect();
                    for row in rows.iter_mut() {
                        match row.get("id").and_then(Value::as_str) {
                            Some(CLEAR_ID) => {
                                row["enabled"] = json!(!recent.is_empty());
                                row["planned"] = json!(false);
                            }
                            Some(MORE_ID) => {
                                row["children"] = Value::Array(all.clone());
                                row["enabled"] = json!(!recent.is_empty());
                                row["planned"] = json!(false);
                            }
                            _ => {}
                        }
                    }
                    for (index, row) in all.into_iter().take(INLINE_ROWS).enumerate() {
                        rows.insert(index, row);
                    }
                }
                true
            } else {
                map.get_mut("children").is_some_and(|children| inject_rows(children, recent))
            }
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecentOutcome {
    Opened,
    Missing(PathBuf),
    Cleared,
    Ignored,
}

#[derive(Default)]
pub struct RecentFiles {
    pub path: Option<PathBuf>,
    pub saved: Vec<PathBuf>,
    pub missing: Option<RecentMissingDialogState>,
}

impl RecentFiles {
    pub fn at(path: PathBuf) -> Self {
        Self { path: Some(path), saved: Vec::new(), missing: None }
    }

    pub fn restore_into(&mut self, workspace: &mut Workspace) {
        if let Some(path) = &self.path {
            restore(workspace, &load(path));
        }
        self.saved = workspace.recent().to_vec();
    }

    pub fn adopt_core_store(&mut self, workspace: &mut Workspace, store: PathBuf) {
        if !store.exists() {
            if let Some(legacy) = &self.path {
                restore(workspace, &load(legacy));
            }
        }
        workspace.enable_recent_persistence(store);
        self.path = None;
        self.saved = workspace.recent().to_vec();
    }

    pub fn sync(&mut self, workspace: &Workspace) -> bool {
        if workspace.recent() == self.saved.as_slice() {
            return false;
        }
        self.saved = workspace.recent().to_vec();
        match &self.path {
            Some(path) => save(path, &self.saved).is_ok(),
            None => false,
        }
    }

    pub fn handle(&mut self, workspace: &mut Workspace, id: &str) -> Result<RecentOutcome, CommandError> {
        if id == CLEAR_ID {
            workspace.clear_recent();
            return Ok(RecentOutcome::Cleared);
        }
        match item_index(id) {
            Some(index) => self.open(workspace, index),
            None => Ok(RecentOutcome::Ignored),
        }
    }

    pub fn open(&mut self, workspace: &mut Workspace, index: usize) -> Result<RecentOutcome, CommandError> {
        let Some(path) = workspace.recent().get(index).cloned() else {
            return Ok(RecentOutcome::Ignored);
        };
        if !path.exists() {
            self.missing = Some(RecentMissingDialogState::new(&path.to_string_lossy()));
            return Ok(RecentOutcome::Missing(path));
        }
        let command = dtool_protocol::Command::with_params("doc.open", json!({ "path": path }));
        workspace.run(dtool_core::Scope::active(), command).1.map(|_| RecentOutcome::Opened)
    }

    pub fn resolve_missing(&mut self, workspace: &mut Workspace, remove: bool) {
        if let Some(state) = self.missing.take() {
            if remove {
                workspace.remove_recent(Path::new(&state.path));
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) -> Option<bool> {
        let answer = show_recent_missing_dialog(ui, self.missing.as_mut()?)?;
        self.resolve_missing(workspace, answer);
        Some(answer)
    }
}
