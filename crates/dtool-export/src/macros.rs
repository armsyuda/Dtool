use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use dtool_core::{CommandError, CommandResult};

pub const MACRO_EXTENSION: &str = "afmacro";
pub const MACRO_CATEGORY_EXTENSION: &str = "afmacros";
pub const MACRO_FILE_FORMAT: &str = "dtool.macro";
pub const MACRO_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroStep {
    pub command: String,
    pub params: serde_json::Value,
    pub enabled: bool,
    pub show_name: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub name: String,
    pub category: String,
    pub steps: Vec<MacroStep>,
}

impl Macro {
    pub fn enabled_steps(&self) -> impl Iterator<Item = &MacroStep> {
        self.steps.iter().filter(|step| step.enabled)
    }
}

#[derive(Debug, Clone)]
pub struct MacroRecorder {
    pub recording: bool,
    pub steps: Vec<MacroStep>,
}

impl MacroRecorder {
    pub fn new() -> Self {
        Self {
            recording: false,
            steps: Vec::new(),
        }
    }

    pub fn start(&mut self) {
        self.recording = true;
        self.steps.clear();
    }

    pub fn stop(&mut self) -> Macro {
        self.recording = false;
        Macro {
            name: String::new(),
            category: String::new(),
            steps: self.steps.clone(),
        }
    }

    pub fn record_step(&mut self, command: &str, params: serde_json::Value) {
        if self.recording {
            self.steps.push(MacroStep {
                command: command.to_string(),
                params,
                enabled: true,
                show_name: false,
            });
        }
    }

    pub fn reset(&mut self) {
        self.recording = false;
        self.steps.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MacroLibrary {
    pub categories: Vec<(String, Vec<Macro>)>,
}

fn clean_name(name: &str) -> Result<String, CommandError> {
    let name = name.trim();
    if name.is_empty() {
        Err(CommandError::invalid_params())
    } else {
        Ok(name.to_string())
    }
}

pub fn step_allowed(catalog: &dtool_protocol::CommandCatalog, command: &str) -> bool {
    dtool_core::macro_recorder::is_recordable(catalog, command)
}

struct PlayRollback {
    document: dtool_core::Document,
    generation: u64,
    undo: dtool_core::undo::UndoStack,
    selection: dtool_core::selection::Selection,
    table_selection: dtool_core::table::selection::TableSelection,
    guides: dtool_core::guides::Guides,
    view: dtool_core::view_state::ViewState,
    clipboard: std::sync::Arc<dtool_core::clipboard::ClipboardPayload>,
    last_duplicate_ids: Vec<dtool_core::ObjectId>,
    last_transform_delta: Option<[f32; 2]>,
    duplicate_revision: Option<u64>,
    last_duplicate_steps: Vec<dtool_core::command::TransformSelectionBoxArgs>,
    text_edit: Option<dtool_core::command::TextEditState>,
    active_tool: String,
    tool_options: dtool_core::context_toolbar::ToolOptionStore,
    history: dtool_core::history_store::SessionHistory,
    change_log: dtool_core::query::ChangeLog,
    edit: Option<dtool_core::edit_session::EditSession>,
    effect_previews: Vec<dtool_core::commands::layer_style::StagedEffect>,
}

impl PlayRollback {
    fn capture(session: &dtool_core::Session) -> Self {
        Self {
            document: session.document.clone(),
            generation: session.generation,
            undo: session.undo.clone(),
            selection: session.selection.clone(),
            table_selection: session.table_selection.clone(),
            guides: session.guides.clone(),
            view: session.view.clone(),
            clipboard: session.clipboard.get_payload(),
            last_duplicate_ids: session.last_duplicate_ids.clone(),
            last_transform_delta: session.last_transform_delta,
            duplicate_revision: session.duplicate_revision,
            last_duplicate_steps: session.last_duplicate_steps.clone(),
            text_edit: session.text_edit.clone(),
            active_tool: session.active_tool.clone(),
            tool_options: session.tool_options.clone(),
            history: session.history.clone(),
            change_log: session.change_log.clone(),
            edit: session.edit.clone(),
            effect_previews: dtool_core::commands::layer_style::snapshot_staged(session),
        }
    }

    fn restore(self, session: &mut dtool_core::Session) {
        session.document = self.document;
        session.generation = self.generation;
        session.undo = self.undo;
        session.selection = self.selection;
        session.table_selection = self.table_selection;
        session.guides = self.guides;
        session.view = self.view;
        session.clipboard.restore_payload(self.clipboard);
        session.last_duplicate_ids = self.last_duplicate_ids;
        session.last_transform_delta = self.last_transform_delta;
        session.duplicate_revision = self.duplicate_revision;
        session.last_duplicate_steps = self.last_duplicate_steps;
        session.text_edit = self.text_edit;
        session.active_tool = self.active_tool;
        session.tool_options = self.tool_options;
        session.history = self.history;
        session.change_log = self.change_log;
        session.edit = self.edit;
        dtool_core::commands::layer_style::restore_staged(session, self.effect_previews);
    }
}

pub fn play_steps<'a>(session: &mut dtool_core::Session, steps: impl IntoIterator<Item = &'a MacroStep>) -> Result<usize, CommandError> {
    let steps: Vec<&MacroStep> = steps.into_iter().filter(|step| step.enabled).collect();
    if steps.iter().any(|step| !step_allowed(&session.catalog, step.command.trim())) {
        return Err(CommandError::new("MACRO_STEP_NOT_ALLOWED"));
    }
    let before = session.document.clone();
    let before_selection = session.selection.ids.clone();
    let undo = session.undo.clone();
    let generation = session.generation;
    let rollback = PlayRollback::capture(session);
    let mut played = 0usize;
    let mut refs = dtool_core::macro_recorder::ReplayRefs::default();
    for step in &steps {
        let name = step.command.trim();
        let params = if step.params.is_null() { serde_json::json!({}) } else { step.params.clone() };
        // "$sel"/"$sel:N"/"$new:N" -> this session's selection / objects created earlier in this play.
        let (params, context) = match refs.prepare(session, name, &params) {
            Ok(prepared) => prepared,
            Err(error) => {
                rollback.restore(session);
                return Err(error);
            }
        };
        if let Err(error) = session.run(&dtool_core::Command::with_params(name, params)) {
            rollback.restore(session);
            return Err(error);
        }
        refs.finish(session, &context);
        played += 1;
    }
    if session.generation != generation {
        session.undo = undo;
        session.commit_replacement(MACRO_PLAY, &before, &before_selection);
    }
    Ok(played)
}

pub fn check_steps(steps: &[MacroStep]) -> Result<(), CommandError> {
    let catalog = dtool_protocol::CommandCatalog::new();
    if steps.iter().all(|step| crate::batch::BATCH_ACTIONS.contains(&step.command.as_str()) || step_allowed(&catalog, &step.command)) {
        Ok(())
    } else {
        Err(CommandError::invalid_params())
    }
}

impl MacroLibrary {
    pub fn new() -> Self {
        Self {
            categories: Vec::new(),
        }
    }

    pub fn default_path() -> PathBuf {
        let root = match std::env::var_os("APPDATA") {
            Some(appdata) => PathBuf::from(appdata).join("DTool"),
            None => PathBuf::from("."),
        };
        root.join("macros").join("library.json")
    }

    pub fn load_default() -> Self {
        Self::load_from_file(&Self::default_path()).unwrap_or_default()
    }

    pub fn save_default(&self) -> Result<(), CommandError> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| CommandError::new("IO_ERROR"))?;
        }
        self.save_to_file(&path)
    }

    pub fn category_names(&self) -> Vec<String> {
        self.categories.iter().map(|(name, _)| name.clone()).collect()
    }

    pub fn category(&self, name: &str) -> Option<&Vec<Macro>> {
        self.categories.iter().find(|(c, _)| c == name).map(|(_, macros)| macros)
    }

    pub fn add_category(&mut self, name: &str) -> Result<(), CommandError> {
        let name = clean_name(name)?;
        if self.category(&name).is_some() {
            return Err(CommandError::new("CONFLICT"));
        }
        self.categories.push((name, Vec::new()));
        Ok(())
    }

    pub fn rename_category(&mut self, from: &str, to: &str) -> Result<(), CommandError> {
        let to = clean_name(to)?;
        if from != to && self.category(&to).is_some() {
            return Err(CommandError::new("CONFLICT"));
        }
        let (name, macros) = self.categories.iter_mut().find(|(c, _)| c == from).ok_or_else(CommandError::not_found)?;
        *name = to.clone();
        for entry in macros.iter_mut() {
            entry.category = to.clone();
        }
        Ok(())
    }

    pub fn delete_category(&mut self, name: &str) -> Result<(String, Vec<Macro>), CommandError> {
        let index = self.categories.iter().position(|(c, _)| c == name).ok_or_else(CommandError::not_found)?;
        Ok(self.categories.remove(index))
    }

    pub fn add_macro(&mut self, name: &str, category: &str, entry: Macro) {
        let mut entry = entry;
        if entry.name.is_empty() {
            entry.name = name.to_string();
        }
        entry.category = category.to_string();
        if let Some(cat) = self.categories.iter_mut().find(|(c, _)| c == category) {
            cat.1.push(entry);
        } else {
            self.categories.push((category.to_string(), vec![entry]));
        }
    }

    pub fn rename_macro(&mut self, category: &str, from: &str, to: &str) -> Result<(), CommandError> {
        let to = clean_name(to)?;
        let (_, macros) = self.categories.iter_mut().find(|(c, _)| c == category).ok_or_else(CommandError::not_found)?;
        if from != to && macros.iter().any(|m| m.name == to) {
            return Err(CommandError::new("CONFLICT"));
        }
        let entry = macros.iter_mut().find(|m| m.name == from).ok_or_else(CommandError::not_found)?;
        entry.name = to;
        Ok(())
    }

    pub fn delete_macro(&mut self, category: &str, name: &str) -> Result<Macro, CommandError> {
        let (_, macros) = self.categories.iter_mut().find(|(c, _)| c == category).ok_or_else(CommandError::not_found)?;
        let index = macros.iter().position(|m| m.name == name).ok_or_else(CommandError::not_found)?;
        Ok(macros.remove(index))
    }

    pub fn get_macro(&self, name: &str) -> Option<&Macro> {
        for (_, macros) in &self.categories {
            if let Some(m) = macros.iter().find(|m| m.name == name) {
                return Some(m);
            }
        }
        None
    }

    pub fn get_in(&self, category: &str, name: &str) -> Option<&Macro> {
        self.category(category).and_then(|macros| macros.iter().find(|m| m.name == name))
    }

    pub fn import_category(&mut self, category: &str, macros: Vec<Macro>) -> Result<String, CommandError> {
        let base = clean_name(category)?;
        let mut name = base.clone();
        let mut counter = 2;
        while self.category(&name).is_some() {
            name = format!("{base} {counter}");
            counter += 1;
        }
        let macros = macros.into_iter().map(|mut m| { m.category = name.clone(); m }).collect();
        self.categories.push((name.clone(), macros));
        Ok(name)
    }

    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), CommandError> {
        let json = serde_json::to_string_pretty(self).map_err(|_| {
            CommandError::new("MACRO_LIBRARY_SERIALIZE_FAILED")
        })?;
        std::fs::write(path, json).map_err(|_| {
            CommandError::new("MACRO_LIBRARY_WRITE_FAILED")
        })?;
        Ok(())
    }

    pub fn load_from_file(path: &PathBuf) -> Result<Self, CommandError> {
        let content = std::fs::read_to_string(path).map_err(|_| {
            CommandError::new("MACRO_LIBRARY_READ_FAILED")
        })?;
        serde_json::from_str(&content).map_err(|_| {
            CommandError::new("MACRO_LIBRARY_PARSE_FAILED")
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MacroFile {
    format: String,
    version: u32,
    kind: String,
    #[serde(default)]
    category: String,
    macros: Vec<Macro>,
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

fn write_file(path: &Path, file: &MacroFile) -> Result<(), CommandError> {
    let json = serde_json::to_string_pretty(file).map_err(|_| CommandError::new("MACRO_SERIALIZE_FAILED"))?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|_| CommandError::new("IO_ERROR"))?;
    }
    std::fs::write(path, json).map_err(|_| CommandError::new("MACRO_WRITE_FAILED"))
}

fn read_file(path: &Path, kind: &str) -> Result<MacroFile, CommandError> {
    let content = std::fs::read_to_string(path).map_err(|_| CommandError::new("MACRO_READ_FAILED"))?;
    let value: serde_json::Value = serde_json::from_str(&content).map_err(|_| CommandError::new("MACRO_PARSE_FAILED"))?;
    let file = if value.get("format").is_some() {
        serde_json::from_value::<MacroFile>(value).map_err(|_| CommandError::new("MACRO_PARSE_FAILED"))?
    } else if kind == MACRO_EXTENSION {
        let single: Macro = serde_json::from_value(value).map_err(|_| CommandError::new("MACRO_PARSE_FAILED"))?;
        MacroFile { format: MACRO_FILE_FORMAT.to_string(), version: MACRO_FILE_VERSION, kind: kind.to_string(), category: single.category.clone(), macros: vec![single] }
    } else {
        return Err(CommandError::new("MACRO_PARSE_FAILED"));
    };
    if file.format != MACRO_FILE_FORMAT || file.version > MACRO_FILE_VERSION || file.kind != kind {
        return Err(CommandError::new("MACRO_PARSE_FAILED"));
    }
    for entry in &file.macros {
        check_steps(&entry.steps)?;
    }
    Ok(file)
}

pub fn export_macro(m: &Macro, path: &PathBuf) -> Result<(), CommandError> {
    if !has_extension(path, MACRO_EXTENSION) {
        return Err(CommandError::invalid_params());
    }
    check_steps(&m.steps)?;
    write_file(path, &MacroFile {
        format: MACRO_FILE_FORMAT.to_string(),
        version: MACRO_FILE_VERSION,
        kind: MACRO_EXTENSION.to_string(),
        category: m.category.clone(),
        macros: vec![m.clone()],
    })
}

pub fn import_macro(path: &PathBuf) -> Result<Macro, CommandError> {
    let file = read_file(path, MACRO_EXTENSION)?;
    let mut macros = file.macros;
    if macros.len() != 1 {
        return Err(CommandError::new("MACRO_PARSE_FAILED"));
    }
    Ok(macros.remove(0))
}

pub fn export_category(library: &MacroLibrary, category: &str, path: &PathBuf) -> Result<(), CommandError> {
    if !has_extension(path, MACRO_CATEGORY_EXTENSION) {
        return Err(CommandError::invalid_params());
    }
    let macros = library.category(category).ok_or_else(CommandError::not_found)?;
    for entry in macros {
        check_steps(&entry.steps)?;
    }
    write_file(path, &MacroFile {
        format: MACRO_FILE_FORMAT.to_string(),
        version: MACRO_FILE_VERSION,
        kind: MACRO_CATEGORY_EXTENSION.to_string(),
        category: category.to_string(),
        macros: macros.clone(),
    })
}

pub fn import_category(path: &PathBuf) -> Result<(String, Vec<Macro>), CommandError> {
    let file = read_file(path, MACRO_CATEGORY_EXTENSION)?;
    let category = clean_name(&file.category)?;
    Ok((category, file.macros))
}

pub fn import_into(library: &mut MacroLibrary, path: &PathBuf) -> Result<serde_json::Value, CommandError> {
    if has_extension(path, MACRO_CATEGORY_EXTENSION) {
        let (category, macros) = import_category(path)?;
        let count = macros.len();
        let name = library.import_category(&category, macros)?;
        Ok(serde_json::json!({ "status": "imported", "kind": MACRO_CATEGORY_EXTENSION, "category": name, "macros": count }))
    } else if has_extension(path, MACRO_EXTENSION) {
        let imported = import_macro(path)?;
        let category = if imported.category.trim().is_empty() { path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { imported.category.clone() };
        let category = clean_name(&category)?;
        let name = imported.name.clone();
        let steps = imported.steps.len();
        library.add_macro(&name, &category, imported);
        Ok(serde_json::json!({ "status": "imported", "kind": MACRO_EXTENSION, "category": category, "name": name, "steps": steps }))
    } else {
        Err(CommandError::invalid_params())
    }
}

fn check_params(params: &serde_json::Value) -> Result<(), CommandError> {
    if params.is_null() || params.is_object() {
        Ok(())
    } else {
        Err(CommandError::invalid_params())
    }
}

fn required_str<'a>(params: &'a serde_json::Value, key: &str) -> Result<&'a str, CommandError> {
    params.get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .ok_or_else(CommandError::invalid_params)
}

fn required_path(params: &serde_json::Value, key: &str) -> Result<PathBuf, CommandError> {
    required_str(params, key).map(PathBuf::from)
}

fn library_path(params: &serde_json::Value) -> PathBuf {
    params.get("library").and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(MacroLibrary::default_path)
}

fn load_library(path: &PathBuf) -> Result<MacroLibrary, CommandError> {
    if path.exists() { MacroLibrary::load_from_file(path) } else { Ok(MacroLibrary::new()) }
}

fn store_library(library: &MacroLibrary, path: &PathBuf) -> Result<(), CommandError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|_| CommandError::new("IO_ERROR"))?;
    }
    library.save_to_file(path)
}

pub const MACRO_RECORD_START: &str = "macro.record.start";
pub const MACRO_RECORD_STOP: &str = "macro.record.stop";
pub const MACRO_PLAY: &str = "macro.play";
pub const MACRO_RESET: &str = "macro.reset";
pub const MACRO_LIBRARY_ADD: &str = "macro.library.add";
pub const MACRO_EXPORT: &str = "macro.export";
pub const MACRO_IMPORT: &str = "macro.import";
pub const NAMES: [&str; 7] = [MACRO_RECORD_START, MACRO_RECORD_STOP, MACRO_PLAY, MACRO_RESET, MACRO_LIBRARY_ADD, MACRO_EXPORT, MACRO_IMPORT];

pub fn install() -> bool {
    dtool_core::commands::macro_host::install(run, can_execute)
}

pub fn recorded_steps(session: &dtool_core::Session) -> Vec<MacroStep> {
    session.macro_recorder.steps().iter().map(|step| MacroStep { command: step.command.clone(), params: step.params.clone(), enabled: true, show_name: false }).collect()
}

fn recording_reply(session: &dtool_core::Session) -> serde_json::Value {
    serde_json::json!({ "recording": session.macro_recorder.is_recording(), "steps": recorded_steps(session) })
}

fn param_steps(params: &serde_json::Value) -> Result<Option<Vec<MacroStep>>, CommandError> {
    match params.get("steps") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => {
            let items = value.as_array().ok_or_else(CommandError::invalid_params)?;
            items
                .iter()
                .map(|item| {
                    let command = item.get("command").and_then(|v| v.as_str()).filter(|v| !v.trim().is_empty()).ok_or_else(CommandError::invalid_params)?;
                    let flag = |key: &str| match item.get(key) {
                        None | Some(serde_json::Value::Null) => Ok(key == "enabled"),
                        Some(serde_json::Value::Bool(v)) => Ok(*v),
                        Some(_) => Err(CommandError::invalid_params()),
                    };
                    Ok(MacroStep { command: command.to_string(), params: item.get("params").cloned().unwrap_or(serde_json::Value::Null), enabled: flag("enabled")?, show_name: flag("show_name")? })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
    }
}

fn play_source(session: &dtool_core::Session, params: &serde_json::Value) -> Result<Vec<MacroStep>, CommandError> {
    if let Some(steps) = param_steps(params)? {
        return Ok(steps);
    }
    match params.get("name").and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
        Some(name) => {
            let library = load_library(&library_path(params))?;
            let category = params.get("category").and_then(|v| v.as_str());
            let entry = match category {
                Some(category) => library.get_in(category, name),
                None => library.get_macro(name),
            }
            .ok_or_else(CommandError::not_found)?;
            Ok(entry.steps.clone())
        }
        None if params.get("name").is_some_and(|v| !v.is_null()) => Err(CommandError::invalid_params()),
        None => Ok(recorded_steps(session)),
    }
}

pub fn can_execute(session: &dtool_core::Session, command: &dtool_core::Command) -> Option<bool> {
    let name = command.name.as_str();
    if !NAMES.contains(&name) {
        return None;
    }
    Some(match name {
        MACRO_RECORD_START | MACRO_RESET | MACRO_IMPORT => true,
        MACRO_RECORD_STOP => session.macro_recorder.is_recording(),
        MACRO_PLAY => session.has_document && !session.macro_recorder.is_recording(),
        _ => true,
    })
}

pub fn run(session: &mut dtool_core::Session, command: &dtool_core::Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    if !NAMES.contains(&name) {
        return None;
    }
    let params = command.params.clone();
    if let Err(error) = check_params(&params) {
        return Some(Err(error));
    }
    Some(match name {
        MACRO_RECORD_START => {
            session.macro_recorder.start();
            Ok(recording_reply(session))
        }
        MACRO_RECORD_STOP => {
            if !session.macro_recorder.is_recording() {
                Err(CommandError::new("MACRO_NOT_RECORDING"))
            } else {
                session.macro_recorder.stop();
                Ok(recording_reply(session))
            }
        }
        MACRO_RESET => {
            session.macro_recorder.reset();
            Ok(recording_reply(session))
        }
        MACRO_PLAY => play_command(session, &params),
        MACRO_LIBRARY_ADD => {
            let mut params = params;
            if params.get("steps").map_or(true, |v| v.is_null()) {
                params["steps"] = serde_json::to_value(recorded_steps(session)).unwrap_or_default();
            }
            handle_macro_library_add(params)
        }
        MACRO_EXPORT => handle_macro_export(params),
        _ => handle_macro_import(params),
    })
}

fn play_command(session: &mut dtool_core::Session, params: &serde_json::Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    if session.macro_recorder.is_recording() {
        return Err(CommandError::new("MACRO_RECORDING"));
    }
    let steps = play_source(session, params)?;
    if steps.iter().all(|step| !step.enabled) {
        return Err(CommandError::new("MACRO_EMPTY"));
    }
    let played = play_steps(session, &steps)?;
    Ok(serde_json::json!({ "played": played, "skipped": steps.len() - played, "doc_version": session.generation }))
}

pub fn handle_macro_library_add(params: serde_json::Value) -> CommandResult {
    check_params(&params)?;
    let name = required_str(&params, "name")?;
    let category = required_str(&params, "category")?;
    let steps: Vec<MacroStep> = match params.get("steps") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?,
    };
    check_steps(&steps)?;
    let count = steps.len();
    let path = library_path(&params);
    let mut library = load_library(&path)?;
    library.add_macro(name, category, Macro { name: name.to_string(), category: category.to_string(), steps });
    store_library(&library, &path)?;

    Ok(serde_json::json!({ "status": "added", "name": name, "category": category, "steps": count }))
}

pub fn handle_macro_export(params: serde_json::Value) -> CommandResult {
    check_params(&params)?;
    let name = required_str(&params, "name")?;
    let path = required_path(&params, "path")?;
    let library = load_library(&library_path(&params))?;
    if has_extension(&path, MACRO_CATEGORY_EXTENSION) {
        export_category(&library, name, &path)?;
    } else {
        let category = params.get("category").and_then(|v| v.as_str());
        let entry = match category {
            Some(category) => library.get_in(category, name),
            None => library.get_macro(name),
        }.ok_or_else(CommandError::not_found)?;
        export_macro(entry, &path)?;
    }

    Ok(serde_json::json!({ "status": "exported", "name": name, "path": path.to_string_lossy() }))
}

pub fn handle_macro_import(params: serde_json::Value) -> CommandResult {
    check_params(&params)?;
    let path = required_path(&params, "path")?;
    let store = library_path(&params);
    let mut library = load_library(&store)?;
    let mut result = import_into(&mut library, &path)?;
    store_library(&library, &store)?;
    if let Some(object) = result.as_object_mut() {
        object.insert("path".to_string(), serde_json::json!(path.to_string_lossy()));
    }

    Ok(result)
}
