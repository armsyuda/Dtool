use crate::colour::swatch::{detach, next_id, Swatch};
use crate::command::Session;
use crate::palette::{self, AppPalettes, Palette, PaletteScope, PaletteSort, PaletteView, PALETTE_EXISTS, PALETTE_FORMAT, PALETTE_NOT_FOUND};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const PALETTE_CREATE: &str = "palette.create";
pub const PALETTE_DELETE: &str = "palette.delete";
pub const PALETTE_RENAME: &str = "palette.rename";
pub const PALETTE_LIST: &str = "palette.list";
pub const PALETTE_IMPORT: &str = "palette.import";
pub const PALETTE_EXPORT: &str = "palette.export";
pub const PALETTE_SORT: &str = "palette.sort";
pub const PALETTE_VIEW: &str = "palette.view";
pub const PALETTE_RESTORE: &str = "palette.restore";
pub const UI_PANEL_SET: &str = "ui.panel.set";
pub const SWATCHES_PANEL: &str = "swatches";

pub fn is_swatches_set(params: &Value) -> bool {
    let panel = params.get("panel").or_else(|| params.get("id")).and_then(Value::as_str);
    panel.is_some_and(|panel| panel.eq_ignore_ascii_case(SWATCHES_PANEL)) && params.get("set").is_some_and(Value::is_object)
}

pub fn apply_view_set(set: &Value) -> Result<palette::PaletteUi, CommandError> {
    let mut ui = palette::ui_state();
    if let Some(value) = set.get("scope") {
        ui.scope = value.as_str().and_then(PaletteScope::parse).ok_or_else(CommandError::invalid_params)?;
    }
    if let Some(value) = set.get("palette") {
        ui.palette = value.as_str().ok_or_else(CommandError::invalid_params)?.to_string();
    }
    if let Some(value) = set.get("by").or_else(|| set.get("sort")) {
        ui.sort = value.as_str().and_then(PaletteSort::parse).ok_or_else(CommandError::invalid_params)?;
    }
    if let Some(value) = set.get("view") {
        ui.view = value.as_str().and_then(PaletteView::parse).ok_or_else(CommandError::invalid_params)?;
    }
    if let Some(value) = set.get("search") {
        ui.search = value.as_str().ok_or_else(CommandError::invalid_params)?.to_string();
    }
    palette::set_ui_state(ui.clone());
    Ok(ui)
}

pub fn view_order(session: &Session, ui: &palette::PaletteUi) -> Vec<u64> {
    let swatches: Vec<Swatch> = match ui.scope {
        PaletteScope::Document if session.has_document => session.document.swatches.iter().filter(|swatch| swatch.palette == ui.palette).cloned().collect(),
        PaletteScope::Application => palette::app_palettes().into_iter().find(|p| p.name == ui.palette).map(|p| p.swatches).unwrap_or_default(),
        _ => Vec::new(),
    };
    palette::ordered_ids(&swatches, ui.sort, &ui.search)
}

const NAMES: &[&str] = &[PALETTE_CREATE, PALETTE_DELETE, PALETTE_RENAME, PALETTE_LIST, PALETTE_IMPORT, PALETTE_EXPORT, PALETTE_SORT, PALETTE_VIEW];

fn text_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str)
}

fn scope_param(params: &Value) -> Result<PaletteScope, CommandError> {
    match params.get("scope") {
        None | Some(Value::Null) => Ok(PaletteScope::Document),
        Some(Value::String(value)) => PaletteScope::parse(value).ok_or_else(CommandError::invalid_params),
        _ => Err(CommandError::invalid_params()),
    }
}

fn required_name(params: &Value, key: &str) -> Result<String, CommandError> {
    text_param(params, key).map(str::trim).filter(|name| !name.is_empty()).map(str::to_string).ok_or_else(CommandError::invalid_params)
}

fn writable(scope: PaletteScope) -> Result<(), CommandError> {
    if scope == PaletteScope::System {
        return Err(CommandError::new("NOT_IMPLEMENTED"));
    }
    Ok(())
}

pub fn doc_palette_names(session: &Session) -> Vec<String> {
    std::iter::once(String::new()).chain(session.document.palettes.iter().cloned()).collect()
}

pub fn doc_has_palette(session: &Session, name: &str) -> bool {
    name.is_empty() || session.document.palettes.iter().any(|known| known == name)
}

pub fn doc_palettes(session: &Session) -> Vec<Palette> {
    doc_palette_names(session).into_iter().map(|name| Palette {
        swatches: session.document.swatches.iter().filter(|swatch| swatch.palette == name).cloned().collect(),
        name,
        scope: PaletteScope::Document,
    }).collect()
}

fn palette_json(palette: &Palette, sort: PaletteSort, search: &str) -> Value {
    json!({
        "name": palette.name,
        "scope": palette.scope,
        "swatches": palette.swatches,
        "order": palette::ordered_ids(&palette.swatches, sort, search)
    })
}

fn snapshot(session: &Session) -> Result<Value, CommandError> {
    Ok(json!({
        "palettes": session.document.palettes,
        "swatches": serde_json::to_value(&session.document.swatches).map_err(|_| CommandError::invalid_params())?,
        "next_swatch_id": session.document.next_swatch_id,
        "bindings": serde_json::to_value(&session.document.swatch_bindings).map_err(|_| CommandError::invalid_params())?
    }))
}

fn restore(session: &mut Session, value: &Value) -> Result<(), CommandError> {
    let field = |key: &str| value.get(key).cloned().ok_or_else(CommandError::invalid_params);
    session.document.palettes = serde_json::from_value(field("palettes")?).map_err(|_| CommandError::invalid_params())?;
    session.document.swatches = serde_json::from_value(field("swatches")?).map_err(|_| CommandError::invalid_params())?;
    session.document.next_swatch_id = field("next_swatch_id")?.as_u64().ok_or_else(CommandError::invalid_params)?;
    session.document.swatch_bindings = serde_json::from_value(field("bindings")?).map_err(|_| CommandError::invalid_params())?;
    session.document.version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    Ok(())
}

fn commit(session: &mut Session, name: &str, before: Value, mut reply: Value) -> CommandResult {
    let after = snapshot(session)?;
    if after != before {
        session.undo.push(crate::undo::Change {
            name: name.to_string(),
            undo: Command::with_params(PALETTE_RESTORE, before),
            redo: Command::with_params(PALETTE_RESTORE, after),
        });
        session.generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        session.document.version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        reply["changes"] = json!([{ "name": name }]);
    } else {
        reply["changes"] = json!([]);
    }
    reply["doc_version"] = json!(session.generation);
    Ok(reply)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let name = command.name.as_str();
    if name == UI_PANEL_SET && is_swatches_set(&command.params) {
        return Some(true);
    }
    if !NAMES.contains(&name) {
        return None;
    }
    let scope = scope_param(&command.params).unwrap_or_default();
    Some(scope != PaletteScope::Document || session.has_document || name == PALETTE_SORT || name == PALETTE_VIEW || name == PALETTE_LIST)
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != PALETTE_RESTORE {
        return None;
    }
    Some(restore(session, &command.params))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    if name == UI_PANEL_SET && is_swatches_set(&command.params) {
        return Some(apply_view_set(&command.params["set"]).map(|ui| json!({ "panel": SWATCHES_PANEL, "state": ui, "order": view_order(session, &ui), "changes": [] })));
    }
    if !NAMES.contains(&name) {
        return None;
    }
    Some(run_named(session, name, &command.params))
}

fn need_document(session: &Session, scope: PaletteScope) -> Result<(), CommandError> {
    if scope == PaletteScope::Document && !session.has_document {
        return Err(CommandError::no_document());
    }
    Ok(())
}

fn unique_name(taken: &[String], base: &str) -> String {
    if !taken.iter().any(|name| name == base) {
        return base.to_string();
    }
    (2..).map(|index| format!("{base} {index}")).find(|candidate| !taken.iter().any(|name| name == candidate)).unwrap_or_else(|| base.to_string())
}

fn read_entries(params: &Value, path: &PathBuf) -> Result<(Option<String>, Vec<Swatch>), CommandError> {
    let bytes = std::fs::read(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => CommandError::new("IO.NOT_FOUND"),
        _ => CommandError::new("IO.OTHER"),
    })?;
    match format_param(params, path)?.as_str() {
        "ase" => {
            let (group, entries) = palette::decode_ase(&bytes)?;
            let swatches = entries.into_iter().map(|entry| {
                let mut swatch = Swatch::new(0, &entry.name, entry.colour);
                swatch.global = entry.global;
                swatch.spot = entry.spot;
                swatch
            }).collect();
            Ok((group, swatches))
        }
        _ => {
            let palette: Palette = serde_json::from_slice(&bytes).map_err(|_| CommandError::new(PALETTE_FORMAT))?;
            Ok((Some(palette.name), palette.swatches))
        }
    }
}

fn requested_format(params: &Value, path: &PathBuf) -> String {
    match text_param(params, "format") {
        Some(format) => format.to_ascii_lowercase(),
        None => path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase(),
    }
}

fn format_param(params: &Value, path: &PathBuf) -> Result<String, CommandError> {
    let format = requested_format(params, path);
    match format.as_str() {
        "ase" => Ok("ase".to_string()),
        "json" | "afpalette" => Ok("json".to_string()),
        _ => Err(CommandError::new(PALETTE_FORMAT)),
    }
}

fn path_param(params: &Value) -> Result<PathBuf, CommandError> {
    text_param(params, "path").filter(|path| !path.is_empty()).map(PathBuf::from).ok_or_else(CommandError::invalid_params)
}

fn run_named(session: &mut Session, name: &str, params: &Value) -> CommandResult {
    match name {
        PALETTE_LIST => {
            let ui = palette::ui_state();
            let scope = match params.get("scope") {
                None | Some(Value::Null) => None,
                _ => Some(scope_param(params)?),
            };
            let mut reply = json!({});
            if scope.is_none() || scope == Some(PaletteScope::Document) {
                let list: Vec<Value> = if session.has_document { doc_palettes(session).iter().map(|p| palette_json(p, ui.sort, &ui.search)).collect() } else { Vec::new() };
                reply["document"] = Value::Array(list);
            }
            if scope.is_none() || scope == Some(PaletteScope::Application) {
                reply["application"] = Value::Array(palette::app_palettes().iter().map(|p| palette_json(p, ui.sort, &ui.search)).collect());
            }
            if scope == Some(PaletteScope::System) {
                reply["system"] = json!([]);
            }
            Ok(reply)
        }
        PALETTE_SORT | PALETTE_VIEW => {
            let ui = apply_view_set(params)?;
            Ok(json!({ "state": ui, "order": view_order(session, &ui), "changes": [] }))
        }
        PALETTE_EXPORT => {
            let scope = scope_param(params)?;
            need_document(session, scope)?;
            let path = path_param(params)?;
            if requested_format(params, &path) == "afpalette" {
                return Err(CommandError::new("UNSUPPORTED_FORMAT"));
            }
            let target = text_param(params, "palette").unwrap_or("").to_string();
            let found = match scope {
                PaletteScope::Document => doc_palettes(session).into_iter().find(|p| p.name == target),
                PaletteScope::Application => palette::app_palettes().into_iter().find(|p| p.name == target || (target.is_empty() && p.name == palette::APP_DEFAULT)),
                PaletteScope::System => None,
            }.ok_or_else(|| CommandError::new(PALETTE_NOT_FOUND))?;
            let bytes = match format_param(params, &path)?.as_str() {
                "ase" => palette::encode_ase(&found.name, &palette::entries_of(&found.swatches)),
                _ => serde_json::to_vec_pretty(&found).map_err(|_| CommandError::invalid_params())?,
            };
            std::fs::write(&path, bytes).map_err(|_| CommandError::new("IO.OTHER"))?;
            Ok(json!({ "count": found.swatches.len(), "path": path, "changes": [] }))
        }
        _ => {
            let scope = scope_param(params)?;
            writable(scope)?;
            need_document(session, scope)?;
            match scope {
                PaletteScope::Document => run_document(session, name, params),
                _ => run_application(name, params),
            }
        }
    }
}

fn conflict_mode(params: &Value) -> Result<&str, CommandError> {
    match text_param(params, "on_conflict") {
        None => Ok("rename"),
        Some(mode @ ("rename" | "skip" | "replace")) => Ok(mode),
        Some(_) => Err(CommandError::invalid_params()),
    }
}

fn run_document(session: &mut Session, name: &str, params: &Value) -> CommandResult {
    let before = snapshot(session)?;
    match name {
        PALETTE_CREATE => {
            let label = required_name(params, "name")?;
            if doc_has_palette(session, &label) {
                return Err(CommandError::new(PALETTE_EXISTS));
            }
            session.document.palettes.push(label.clone());
            commit(session, name, before, json!({ "name": label }))
        }
        PALETTE_DELETE => {
            let label = required_name(params, "name")?;
            let at = session.document.palettes.iter().position(|known| *known == label).ok_or_else(|| CommandError::new(PALETTE_NOT_FOUND))?;
            session.document.palettes.remove(at);
            let removed: Vec<u64> = session.document.swatches.iter().filter(|swatch| swatch.palette == label).map(|swatch| swatch.id).collect();
            session.document.swatches.retain(|swatch| swatch.palette != label);
            for id in &removed {
                detach(&mut session.document, *id);
            }
            commit(session, name, before, json!({ "name": label, "removed": removed }))
        }
        PALETTE_RENAME => {
            let label = required_name(params, "name")?;
            let to = required_name(params, "to")?;
            let at = session.document.palettes.iter().position(|known| *known == label).ok_or_else(|| CommandError::new(PALETTE_NOT_FOUND))?;
            if label != to && doc_has_palette(session, &to) {
                return Err(CommandError::new(PALETTE_EXISTS));
            }
            session.document.palettes[at] = to.clone();
            for swatch in session.document.swatches.iter_mut().filter(|swatch| swatch.palette == label) {
                swatch.palette = to.clone();
            }
            commit(session, name, before, json!({ "name": to }))
        }
        _ => {
            let path = path_param(params)?;
            let mode = conflict_mode(params)?;
            let (group, swatches) = read_entries(params, &path)?;
            let base = text_param(params, "palette_name").map(str::to_string).or(group).or_else(|| path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string)).unwrap_or_default();
            let exists = doc_has_palette(session, &base);
            let (target, renamed, skipped, replaced) = match (exists, mode) {
                (false, _) => (base.clone(), 0, 0, 0),
                (true, "skip") => return commit(session, name, before, json!({ "palette": base, "added": 0, "renamed": 0, "skipped": swatches.len(), "replaced": 0 })),
                (true, "replace") => {
                    let removed: Vec<u64> = session.document.swatches.iter().filter(|swatch| swatch.palette == base).map(|swatch| swatch.id).collect();
                    session.document.swatches.retain(|swatch| swatch.palette != base);
                    for id in &removed {
                        detach(&mut session.document, *id);
                    }
                    (base.clone(), 0, 0, removed.len())
                }
                (true, _) => (unique_name(&doc_palette_names(session), &base), 1, 0, 0),
            };
            if !doc_has_palette(session, &target) {
                session.document.palettes.push(target.clone());
            }
            let mut ids = Vec::new();
            for mut swatch in swatches {
                swatch.id = next_id(&session.document);
                swatch.palette = target.clone();
                session.document.next_swatch_id = swatch.id + 1;
                ids.push(swatch.id);
                session.document.swatches.push(swatch);
            }
            commit(session, name, before, json!({ "palette": target, "added": ids.len(), "ids": ids, "renamed": renamed, "skipped": skipped, "replaced": replaced }))
        }
    }
}

fn run_application(name: &str, params: &Value) -> CommandResult {
    match name {
        PALETTE_CREATE => {
            let label = required_name(params, "name")?;
            palette::with_app(true, |store| {
                if store.find(&label).is_some() {
                    return Err(CommandError::new(PALETTE_EXISTS));
                }
                store.ensure(&label);
                Ok(json!({ "name": label, "changes": [] }))
            })
        }
        PALETTE_DELETE => {
            let label = required_name(params, "name")?;
            palette::with_app(true, |store| {
                let at = store.find(&label).ok_or_else(|| CommandError::new(PALETTE_NOT_FOUND))?;
                store.palettes.remove(at);
                Ok(json!({ "name": label, "changes": [] }))
            })
        }
        PALETTE_RENAME => {
            let label = required_name(params, "name")?;
            let to = required_name(params, "to")?;
            palette::with_app(true, |store| {
                let at = store.find(&label).ok_or_else(|| CommandError::new(PALETTE_NOT_FOUND))?;
                if label != to && store.find(&to).is_some() {
                    return Err(CommandError::new(PALETTE_EXISTS));
                }
                store.palettes[at].name = to.clone();
                Ok(json!({ "name": to, "changes": [] }))
            })
        }
        _ => {
            let path = path_param(params)?;
            let mode = conflict_mode(params)?.to_string();
            let (group, swatches) = read_entries(params, &path)?;
            let base = text_param(params, "palette_name").map(str::to_string).or(group).or_else(|| path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string)).unwrap_or_else(|| palette::APP_DEFAULT.to_string());
            palette::with_app(true, |store| import_app(store, &base, &mode, swatches))
        }
    }
}

fn import_app(store: &mut AppPalettes, base: &str, mode: &str, swatches: Vec<Swatch>) -> CommandResult {
    let names: Vec<String> = store.palettes.iter().map(|p| p.name.clone()).collect();
    let exists = store.find(base).is_some();
    let (target, renamed, replaced) = match (exists, mode) {
        (false, _) => (base.to_string(), 0, 0),
        (true, "skip") => return Ok(json!({ "palette": base, "added": 0, "renamed": 0, "skipped": swatches.len(), "replaced": 0, "changes": [] })),
        (true, "replace") => {
            let at = store.ensure(base);
            let count = store.palettes[at].swatches.len();
            store.palettes[at].swatches.clear();
            (base.to_string(), 0, count)
        }
        (true, _) => (unique_name(&names, base), 1, 0),
    };
    let at = store.ensure(&target);
    let mut ids = Vec::new();
    for mut swatch in swatches {
        swatch.id = store.next_id();
        swatch.palette = target.clone();
        store.next_id = swatch.id + 1;
        ids.push(swatch.id);
        store.palettes[at].swatches.push(swatch);
    }
    Ok(json!({ "palette": target, "added": ids.len(), "ids": ids, "renamed": renamed, "skipped": 0, "replaced": replaced, "changes": [] }))
}