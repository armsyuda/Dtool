use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub const CREATE: &str = "export.options_preset.create";
pub const LIST: &str = "export.options_preset.list";
pub const LOAD: &str = "export.options_preset.load";
pub const RENAME: &str = "export.options_preset.rename";
pub const UPDATE: &str = "export.options_preset.update";
pub const DELETE: &str = "export.options_preset.delete";
pub const NAMES: [&str; 6] = [CREATE, LIST, LOAD, RENAME, UPDATE, DELETE];
pub const OPTIONS_KEY: &str = "dtool.export_options";
pub const STORE_FILE: &str = "export_presets.json";
pub const PRESET_EXISTS: &str = "PRESET_EXISTS";
pub const PRESET_FORMAT: &str = "PRESET_FORMAT";
pub const FORMAT_KEY: &str = "format";
pub const FORMATS: [&str; 9] = ["Png", "Jpeg", "Webp", "Tiff", "Gif", "Pdf", "Svg", "Eps", "Wmf"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportPreset {
    pub id: u64,
    pub name: String,
    pub options: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExportPresetStore {
    #[serde(default)]
    pub presets: Vec<ExportPreset>,
}

thread_local! {
    static STORE_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub fn set_store_path(path: Option<PathBuf>) {
    STORE_OVERRIDE.with(|slot| *slot.borrow_mut() = path);
}

pub fn store_path() -> PathBuf {
    if let Some(path) = STORE_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return path;
    }
    crate::prefs::Prefs::default_path().with_file_name(STORE_FILE)
}

pub fn load_store(path: &Path) -> Result<ExportPresetStore, CommandError> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| CommandError::new(PRESET_FORMAT)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ExportPresetStore::default()),
        Err(_) => Err(CommandError::new("IO.OTHER")),
    }
}

pub fn save_store(path: &Path, store: &ExportPresetStore) -> Result<(), CommandError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| CommandError::new("IO.OTHER"))?;
    }
    let text = serde_json::to_string_pretty(store).map_err(|_| CommandError::invalid_params())?;
    crate::commands::package::write_document(path, text.as_bytes()).map_err(|_| CommandError::new("IO.OTHER"))
}

pub fn current_options(document: &crate::document::Document) -> Option<Map<String, Value>> {
    document.export_defaults.get(OPTIONS_KEY).and_then(Value::as_object).cloned()
}

pub fn canonical_format(value: &str) -> Option<&'static str> {
    let value = value.trim();
    FORMATS.iter().copied().find(|format| format.eq_ignore_ascii_case(value)).or_else(|| match value.to_ascii_lowercase().as_str() {
        "jpg" => Some("Jpeg"),
        "tif" => Some("Tiff"),
        _ => None,
    })
}

pub fn normalize_options(options: &mut Map<String, Value>) -> Result<(), CommandError> {
    let canonical = match options.get(FORMAT_KEY) {
        None => return Ok(()),
        Some(Value::String(value)) => canonical_format(value).ok_or_else(CommandError::invalid_params)?,
        Some(_) => return Err(CommandError::invalid_params()),
    };
    options.insert(FORMAT_KEY.to_string(), Value::String(canonical.to_string()));
    Ok(())
}

pub fn normalize_known_format(options: &mut Map<String, Value>) -> bool {
    if normalize_options(options).is_ok() {
        return true;
    }
    options.remove(FORMAT_KEY);
    false
}

pub fn merged_options(base: Value, saved: &Map<String, Value>) -> Value {
    let mut out = match base {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    normalize_known_format(&mut out);
    let mut saved = saved.clone();
    normalize_known_format(&mut saved);
    for (key, value) in saved {
        out.insert(key, value);
    }
    Value::Object(out)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        LOAD => Some(session.has_document),
        CREATE | UPDATE => Some(command.params.get("options").is_some_and(Value::is_object) || session.has_document),
        LIST | RENAME | DELETE => Some(true),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let params = &command.params;
    match command.name.as_str() {
        LIST => Some(list()),
        CREATE => Some(create(session, params)),
        UPDATE => Some(update(session, params)),
        RENAME => Some(rename(params)),
        DELETE => Some(delete(params)),
        LOAD => Some(load(session, params)),
        _ => None,
    }
}

fn preset_json(preset: &ExportPreset) -> Value {
    json!({ "preset_id": preset.id, "name": preset.name, "options": preset.options })
}

fn list() -> CommandResult {
    let store = load_store(&store_path())?;
    Ok(json!({ "presets": store.presets.iter().map(preset_json).collect::<Vec<_>>() }))
}

fn name_param(params: &Value) -> Result<String, CommandError> {
    params.get("name").and_then(Value::as_str).map(str::trim).filter(|name| !name.is_empty()).map(str::to_string).ok_or_else(CommandError::invalid_params)
}

fn options_param(session: &Session, params: &Value) -> Result<Map<String, Value>, CommandError> {
    match params.get("options") {
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(_) => Err(CommandError::invalid_params()),
        None if session.has_document => Ok(current_options(&session.document).unwrap_or_default()),
        None => Err(CommandError::no_document()),
    }
}

fn find(store: &ExportPresetStore, params: &Value) -> Result<usize, CommandError> {
    let index = match (params.get("preset_id").and_then(Value::as_u64), params.get("preset").and_then(Value::as_str)) {
        (Some(id), _) => store.presets.iter().position(|preset| preset.id == id),
        (None, Some(name)) => store.presets.iter().position(|preset| preset.name == name),
        (None, None) => return Err(CommandError::invalid_params()),
    };
    index.ok_or_else(CommandError::not_found)
}

fn name_taken(store: &ExportPresetStore, name: &str, except: Option<u64>) -> bool {
    store.presets.iter().any(|preset| preset.name == name && Some(preset.id) != except)
}

fn create(session: &Session, params: &Value) -> CommandResult {
    let name = name_param(params)?;
    let mut options = options_param(session, params)?;
    normalize_options(&mut options)?;
    let path = store_path();
    let mut store = load_store(&path)?;
    if name_taken(&store, &name, None) {
        return Err(CommandError::new(PRESET_EXISTS));
    }
    let id = store.presets.iter().map(|preset| preset.id).max().map_or(1, |max| max + 1);
    let preset = ExportPreset { id, name, options };
    let reply = preset_json(&preset);
    store.presets.push(preset);
    save_store(&path, &store)?;
    Ok(reply)
}

fn update(session: &Session, params: &Value) -> CommandResult {
    let mut options = options_param(session, params)?;
    normalize_options(&mut options)?;
    let path = store_path();
    let mut store = load_store(&path)?;
    let index = find(&store, params)?;
    store.presets[index].options = options;
    let reply = preset_json(&store.presets[index]);
    save_store(&path, &store)?;
    Ok(reply)
}

fn rename(params: &Value) -> CommandResult {
    let name = name_param(params)?;
    let path = store_path();
    let mut store = load_store(&path)?;
    let index = find(&store, params)?;
    if name_taken(&store, &name, Some(store.presets[index].id)) {
        return Err(CommandError::new(PRESET_EXISTS));
    }
    store.presets[index].name = name;
    let reply = preset_json(&store.presets[index]);
    save_store(&path, &store)?;
    Ok(reply)
}

fn delete(params: &Value) -> CommandResult {
    let path = store_path();
    let mut store = load_store(&path)?;
    let index = find(&store, params)?;
    let removed = store.presets.remove(index);
    save_store(&path, &store)?;
    Ok(json!({ "preset_id": removed.id }))
}

fn load(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let store = load_store(&store_path())?;
    let mut preset = store.presets[find(&store, params)?].clone();
    normalize_known_format(&mut preset.options);
    let before = session.document.clone();
    if !session.document.export_defaults.is_object() {
        session.document.export_defaults = json!({});
    }
    session.document.export_defaults[OPTIONS_KEY] = Value::Object(preset.options.clone());
    let after = session.document.clone();
    session.undo.push(crate::undo::Change {
        name: LOAD.to_string(),
        undo: Command::with_params(crate::commands::feature_restore::FEATURE_RESTORE, json!({ "document": before })),
        redo: Command::with_params(crate::commands::feature_restore::FEATURE_RESTORE, json!({ "document": after })),
    });
    session.generation += 1;
    session.document.version += 1;
    let mut reply = preset_json(&preset);
    reply["doc_version"] = json!(session.generation);
    Ok(reply)
}