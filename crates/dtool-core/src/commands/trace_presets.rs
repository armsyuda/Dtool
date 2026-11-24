use crate::command::Session;
use crate::trace::{builtin_presets, TracePreset};
use dtool_protocol::{CommandError, CommandResult};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn presets_path() -> PathBuf {
    let mut path = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    path.push("DTool");
    path.push("trace_presets.json");
    path
}

fn load_user_presets() -> Result<Vec<TracePreset>,CommandError> {
    let content=match fs::read_to_string(presets_path()) {Ok(v)=>v,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(Vec::new()),Err(_)=>return Err(CommandError::validation("error.trace_preset_io"))};
    serde_json::from_str(&content).map_err(|_|CommandError::validation("error.trace_preset_format"))
}

fn save_user_presets(presets: &[TracePreset]) -> Result<(), CommandError> {
    let path = presets_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| CommandError::validation("error.trace_preset_io"))?;
    }
    match serde_json::to_string_pretty(presets) {
        Ok(json) => {
            crate::commands::package::write_document(&path,json.as_bytes()).map_err(|_|CommandError::validation("error.trace_preset_io"))?;
            Ok(())
        }
        Err(_) => Err(CommandError::validation("error.trace_preset_io")),
    }
}

pub fn can_execute(name: &str) -> bool {
    matches!(name, "trace.presets.list" | "trace.presets.save" | "trace.presets.rename" | "trace.presets.delete")
}

pub fn presets() -> Result<Vec<TracePreset>, CommandError> {
    let mut presets = builtin_presets();
    presets.extend(load_user_presets()?);
    Ok(presets)
}

pub fn run(_session: &mut Session, name: &str, args: Value) -> Result<CommandResult, CommandError> {
    match name {
        "trace.presets.list" => {
            let names: Vec<String> = presets()?.into_iter().map(|p| p.name).collect();
            Ok(Ok(serde_json::json!({ "presets": names })))
        }
        "trace.presets.save" => {
            let name_str = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| CommandError::invalid_params())?;
            if name_str.trim().is_empty() {return Err(CommandError::invalid_params());}
            if builtin_presets().iter().any(|p| p.name == name_str) {
                return Err(CommandError::invalid_params());
            }
            let settings: crate::trace::TraceSettings = serde_json::from_value(args.get("settings").cloned().unwrap_or_default())
                .map_err(|_| CommandError::validation("error.trace_preset_io"))?;
            if !crate::trace::validate_settings(&settings) { return Err(CommandError::invalid_params()); }
            
            let mut presets = load_user_presets()?;
            if let Some(existing) = presets.iter_mut().find(|p| p.name == name_str) {
                existing.settings = settings;
            } else {
                presets.push(TracePreset { name: name_str.to_string(), builtin: false, settings });
            }
            save_user_presets(&presets)?;
            Ok(Ok(serde_json::json!({})))
        }
        "trace.presets.rename" => {
            let old_name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| CommandError::invalid_params())?;
            let new_name = args.get("new_name").and_then(|v| v.as_str()).ok_or_else(|| CommandError::invalid_params())?;
            
            if new_name.trim().is_empty() {return Err(CommandError::invalid_params());}
            if builtin_presets().iter().any(|p| p.name == old_name) {
                return Err(CommandError::invalid_params());
            }
            
            let mut presets = load_user_presets()?;
            if !presets.iter().any(|p| p.name == old_name) {
                return Err(CommandError::not_found());
            }
            if presets.iter().any(|p| p.name == new_name) || builtin_presets().iter().any(|p| p.name == new_name) {
                return Err(CommandError::invalid_params());
            }
            
            for preset in &mut presets {
                if preset.name == old_name {
                    preset.name = new_name.to_string();
                    break;
                }
            }
            save_user_presets(&presets)?;
            Ok(Ok(serde_json::json!({})))
        }
        "trace.presets.delete" => {
            let name_str = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| CommandError::invalid_params())?;
            
            if name_str.trim().is_empty() {return Err(CommandError::invalid_params());}
            if builtin_presets().iter().any(|p| p.name == name_str) {
                return Err(CommandError::invalid_params());
            }
            
            let mut presets = load_user_presets()?;
            let initial_len = presets.len();
            presets.retain(|p| p.name != name_str);
            
            if presets.len() == initial_len {
                return Err(CommandError::not_found());
            }
            
            save_user_presets(&presets)?;
            Ok(Ok(serde_json::json!({})))
        }
        _ => Err(CommandError::not_implemented()),
    }
}

pub fn apply(session: &mut Session, name: &str, args: Value) -> Result<CommandResult, CommandError> {
    run(session, name, args)
}
