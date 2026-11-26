use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_KEYMAP_JSON: &str = include_str!("../assets/default_shortcuts.json");
pub const CMD_LIST: &str = "ui.shortcuts.list";
pub const CMD_SET: &str = "ui.shortcut.set";
pub const CMD_RESET: &str = "ui.shortcuts.reset";
pub const CMD_IMPORT: &str = "ui.shortcuts.import";
pub const CMD_EXPORT: &str = "ui.shortcuts.export";
pub const EXPORT_OPEN_COMMAND: &str = "ui.export.open";
pub const AFFINITY_EXPORT_COMMAND: &str = "Serif.Affinity.Commands.ExportDocumentCommand";
pub const EXPORT_OPEN_KEY: &str = "W";
pub const EXPORT_OPEN_MODIFIERS: Modifiers = Modifiers { ctrl: true, shift: true, alt: true };
pub const FIND_OPEN_COMMAND: &str = "ui.find.open";
pub const FIND_OPEN_KEY: &str = "F";
pub const FIND_OPEN_MODIFIERS: Modifiers = Modifiers { ctrl: true, shift: false, alt: false };
const DELETE_COMMAND: &str = "Serif.Interop.Persona.Commands.DeleteObjectsCommand";

pub use dtool_core::shortcuts::{Focus, Modifiers};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub command: String,
    pub key: String,
    pub modifiers: Modifiers,
}

fn modifiers_from_json(value: Option<&serde_json::Value>) -> Modifiers {
    match value {
        Some(serde_json::Value::String(value)) => Modifiers::parse(value),
        Some(serde_json::Value::Object(value)) => Modifiers {
            ctrl: value.get("ctrl").and_then(serde_json::Value::as_bool).unwrap_or(false),
            shift: value.get("shift").and_then(serde_json::Value::as_bool).unwrap_or(false),
            alt: value.get("alt").and_then(serde_json::Value::as_bool).unwrap_or(false),
        },
        _ => Modifiers::default(),
    }
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShortcutMap {
    bindings: HashMap<String, Vec<Shortcut>>,
}

fn collect(value: &serde_json::Value, map: &mut ShortcutMap) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect(item, map);
            }
        }
        serde_json::Value::Object(obj) => {
            let key = obj.get("key").and_then(|v| v.as_str());
            let command = match obj.get("command").and_then(|v| v.as_str()) {
                Some(c) => Some(c.to_string()),
                None => obj
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .map(|t| format!("tool.{t}")),
            };
            match (command, key) {
                (Some(command), Some(key)) => {
                    if !key.is_empty() {
                        let modifiers = modifiers_from_json(
                            obj.get("modifierKeys").or_else(|| obj.get("modifiers")),
                        );
                        map.add(Shortcut {
                            command,
                            key: key.to_string(),
                            modifiers,
                        });
                    }
                }
                _ => {
                    for child in obj.values() {
                        collect(child, map);
                    }
                }
            }
        }
        _ => {}
    }
}

impl ShortcutMap {
    pub fn affinity_default() -> Self {
        Self::from_affinity_json(DEFAULT_KEYMAP_JSON).with_export_default().with_find_default().with_delete_default()
    }

    pub fn with_find_default(mut self) -> Self {
        let taken = self.bindings.get(&FIND_OPEN_KEY.to_lowercase()).is_some_and(|list| {
            list.iter().any(|s| s.modifiers == FIND_OPEN_MODIFIERS && s.command != FIND_OPEN_COMMAND && crate::shortcut_commands::target_for(&s.command).is_some())
        });
        if self.bindings_for(FIND_OPEN_COMMAND).is_empty() && !taken {
            self.add(Shortcut { command: FIND_OPEN_COMMAND.to_string(), key: FIND_OPEN_KEY.to_string(), modifiers: FIND_OPEN_MODIFIERS });
        }
        self
    }

    pub fn with_export_default(mut self) -> Self {
        let taken = self
            .bindings
            .get(&EXPORT_OPEN_KEY.to_lowercase())
            .is_some_and(|list| list.iter().any(|s| s.modifiers == EXPORT_OPEN_MODIFIERS));
        if self.bindings_for(EXPORT_OPEN_COMMAND).is_empty() && !taken {
            self.add(Shortcut {
                command: EXPORT_OPEN_COMMAND.to_string(),
                key: EXPORT_OPEN_KEY.to_string(),
                modifiers: EXPORT_OPEN_MODIFIERS,
            });
        }
        self
    }

    fn with_delete_default(mut self) -> Self {
        let modifiers = Modifiers::default();
        let taken = self.bindings.get("x").is_some_and(|list| {
            list.iter().any(|shortcut| shortcut.modifiers == modifiers
                && crate::shortcut_commands::target_for(&shortcut.command).is_some())
        });
        if self.bindings_for(DELETE_COMMAND).is_empty() && !taken {
            self.add(Shortcut { command: DELETE_COMMAND.to_string(), key: "X".to_string(), modifiers });
        }
        self
    }

    pub fn dispatch_command(command: &str) -> String {
        if command == AFFINITY_EXPORT_COMMAND {
            EXPORT_OPEN_COMMAND.to_string()
        } else {
            command.to_string()
        }
    }

    pub fn from_affinity_json(json: &str) -> Self {
        let mut map = ShortcutMap::default();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
            collect(&value, &mut map);
        }
        map
    }

    fn parse_any(content: &str) -> Option<Self> {
        if let Ok(map) = serde_json::from_str::<ShortcutMap>(content) {
            return Some(map);
        }
        let map = Self::from_affinity_json(content);
        if map.is_empty() {
            None
        } else {
            Some(map)
        }
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Some(map) = Self::parse_any(&content) {
                    return map.with_export_default().with_find_default().with_delete_default();
                }
            }
        }
        Self::affinity_default()
    }

    pub fn save(&self) {
        let _ = self.save_checked();
    }

    pub fn save_checked(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or_else(|| "IO_ERROR".to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }

    pub fn reset(&mut self) {
        *self = Self::affinity_default();
    }

    pub fn import_file(path: &str) -> Result<Self, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::parse_any(&content).filter(|map| !map.is_empty()).ok_or_else(|| "INVALID_PARAMS".to_string())
    }

    pub fn export_file(&self, path: &str) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn dispatch(&self, focus: Focus, key: &str, modifiers: Modifiers) -> Option<String> {
        if focus == Focus::TextField || focus == Focus::Dialog {
            return None;
        }

        let key_lower = key.to_lowercase();

        if let Some(shortcuts) = self.bindings.get(&key_lower) {
            for shortcut in shortcuts {
                if shortcut.modifiers == modifiers {
                    return Some(Self::dispatch_command(&shortcut.command));
                }
            }
        }

        None
    }

    pub fn dispatch_all(&self, focus: Focus, key: &str, modifiers: Modifiers) -> Vec<String> {
        if focus == Focus::TextField || focus == Focus::Dialog {
            return Vec::new();
        }
        self.bindings
            .get(&key.to_lowercase())
            .map(|list| list.iter().filter(|s| s.modifiers == modifiers).map(|s| s.command.clone()).collect())
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.values().all(|v| v.is_empty())
    }

    pub fn list(&self) -> Vec<Shortcut> {
        let mut all: Vec<Shortcut> = self.bindings.values().flatten().cloned().collect();
        all.sort_by(|a, b| a.command.cmp(&b.command).then(a.key.cmp(&b.key)));
        all
    }

    pub fn commands(&self) -> BTreeSet<String> {
        self.bindings
            .values()
            .flatten()
            .map(|s| s.command.clone())
            .collect()
    }

    pub fn bindings_for(&self, command: &str) -> Vec<Shortcut> {
        let mut found: Vec<Shortcut> = self
            .bindings
            .values()
            .flatten()
            .filter(|s| s.command == command)
            .cloned()
            .collect();
        found.sort_by(|a, b| a.key.cmp(&b.key));
        found
    }

    pub fn clear_command(&mut self, command: &str) {
        for list in self.bindings.values_mut() {
            list.retain(|s| s.command != command);
        }
        self.bindings.retain(|_, list| !list.is_empty());
    }

    pub fn clear_all(&mut self) {
        self.bindings.clear();
    }

    pub fn set(&mut self, command: &str, key: &str, modifiers: Modifiers) {
        self.clear_command(command);
        if key.is_empty() {
            return;
        }
        let key_lower = key.to_lowercase();
        if let Some(list) = self.bindings.get_mut(&key_lower) {
            list.retain(|s| s.modifiers != modifiers);
        }
        self.add(Shortcut {
            command: command.to_string(),
            key: key.to_string(),
            modifiers,
        });
    }

    pub fn run(&mut self, name: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        match name {
            CMD_LIST => serde_json::to_value(self.list()).map_err(|e| e.to_string()),
            CMD_SET => {
                let command = params
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "INVALID_PARAMS".to_string())?
                    .to_string();
                let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let modifiers = modifiers_from_json(params.get("modifiers"));
                self.set(&command, &key, modifiers);
                self.save_checked()?;
                serde_json::to_value(self.bindings_for(&command)).map_err(|e| e.to_string())
            }
            CMD_RESET => {
                self.reset();
                self.save_checked()?;
                Ok(serde_json::json!({ "count": self.list().len() }))
            }
            CMD_IMPORT => {
                let path = params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "INVALID_PARAMS".to_string())?;
                *self = Self::import_file(path)?;
                self.save_checked()?;
                Ok(serde_json::json!({ "count": self.list().len() }))
            }
            CMD_EXPORT => {
                let path = params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "INVALID_PARAMS".to_string())?;
                self.export_file(path)?;
                Ok(serde_json::json!({ "path": path }))
            }
            _ => Err("UNKNOWN_COMMAND".to_string()),
        }
    }

    fn add(&mut self, shortcut: Shortcut) {
        let key_lower = shortcut.key.to_lowercase();
        let list = self.bindings.entry(key_lower).or_default();
        if !list.contains(&shortcut) {
            list.push(shortcut);
        }
    }

    fn config_path() -> Option<PathBuf> {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Some(PathBuf::from(appdata).join("DTool").join("shortcuts.json"));
        }
        None
    }
}

pub const TAB_KEY: &str = "Tab";
pub const TOGGLE_UI_COMMAND: &str = "Serif.Affinity.Commands.ToggleUICommand";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    ToggleUi,
    NextField,
    Ignore,
}

impl ShortcutMap {
    pub fn tab_action(&self, focus: Focus, modifiers: Modifiers) -> TabAction {
        self.tab_action_for_edit(focus, modifiers, false)
    }

    pub fn tab_action_for_edit(&self, focus: Focus, modifiers: Modifiers, text_edit_active: bool) -> TabAction {
        if text_edit_active && matches!(focus, Focus::Canvas) {
            return TabAction::Ignore;
        }
        match focus {
            Focus::TextField | Focus::Dialog => {
                if modifiers.ctrl || modifiers.alt {
                    TabAction::Ignore
                } else {
                    TabAction::NextField
                }
            }
            Focus::Canvas => match self.dispatch(focus, TAB_KEY, modifiers) {
                Some(command) if command == TOGGLE_UI_COMMAND => TabAction::ToggleUi,
                _ => TabAction::Ignore,
            },
            Focus::Menu | Focus::Other => TabAction::Ignore,
        }
    }
}

struct SharedShortcuts {
    map: ShortcutMap,
    revision: u64,
    focus: Focus,
}

static SHARED: std::sync::OnceLock<std::sync::Mutex<SharedShortcuts>> = std::sync::OnceLock::new();

pub fn focus_key(focus: Focus) -> &'static str {
    match focus {
        Focus::Canvas => "canvas",
        Focus::TextField => "text_field",
        Focus::Dialog => "dialog",
        Focus::Menu => "menu",
        Focus::Other => "other",
    }
}

fn error_code(message: String) -> dtool_core::CommandError {
    if !message.is_empty() && message.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        dtool_core::CommandError::new(&message)
    } else {
        dtool_core::CommandError::new("IO_ERROR")
    }
}

fn shared_action(command: &dtool_core::Command) -> dtool_core::CommandResult {
    let shared = SHARED.get().ok_or_else(|| dtool_core::CommandError::new("NOT_AVAILABLE"))?;
    let mut shared = shared.lock().map_err(|_| dtool_core::CommandError::new("EDITOR_BUSY"))?;
    if command.name == "ui.focus.query" {
        return Ok(serde_json::json!({ "focus": focus_key(shared.focus) }));
    }
    let result = shared.map.run(&command.name, &command.params).map_err(error_code)?;
    if matches!(command.name.as_str(), CMD_SET | CMD_RESET | CMD_IMPORT) {
        shared.revision += 1;
    }
    Ok(result)
}

pub fn install(map: ShortcutMap) -> bool {
    SHARED.set(std::sync::Mutex::new(SharedShortcuts { map, revision: 0, focus: Focus::Canvas })).is_ok()
        && dtool_core::commands::shortcuts::register_action(shared_action)
}

pub fn publish_focus(focus: Focus) {
    if let Some(Ok(mut shared)) = SHARED.get().map(|shared| shared.lock()) {
        shared.focus = focus;
    }
}

pub fn publish_map(map: &ShortcutMap) {
    if let Some(Ok(mut shared)) = SHARED.get().map(|shared| shared.lock()) {
        shared.map = map.clone();
    }
}

pub fn sync_shared(local: &mut ShortcutMap, seen: &mut u64) -> bool {
    let Some(Ok(shared)) = SHARED.get().map(|shared| shared.lock()) else {
        return false;
    };
    if shared.revision == *seen {
        return false;
    }
    *seen = shared.revision;
    *local = shared.map.clone();
    true
}
