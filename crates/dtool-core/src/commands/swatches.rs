use crate::colour::swatch::{bind, bind_use, detach, ensure_range_id, next_id, propagate_all, set_stop_colour, set_story_colour, Swatch, SwatchBinding, SwatchTarget};
use crate::command::Session;
use crate::object::ObjectId;
use crate::paint::{Colour, Paint};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const SWATCH_ADD: &str = "swatch.add";
pub const SWATCH_SPOT_ADD: &str = "swatch.spot.add";
pub const SWATCH_RENAME: &str = "swatch.rename";
pub const SWATCH_EDIT: &str = "swatch.edit";
pub const SWATCH_UPDATE: &str = "swatch.update";
pub const SWATCH_MAKE_GLOBAL: &str = "swatch.make_global";
pub const SWATCH_DELETE: &str = "swatch.delete";
pub const SWATCH_APPLY: &str = "swatch.apply";
pub const SWATCH_APPLY_TO_SELECTION: &str = "swatch.apply_to_selection";
pub const SWATCH_LIST: &str = "swatch.list";
pub const SWATCH_ADD_FROM_SELECTION: &str = "swatch.add_from_selection";
pub const SWATCH_RESTORE: &str = "swatch.restore";
pub const SWATCH_NOT_FOUND: &str = "SWATCH_NOT_FOUND";

const MUTATING: &[&str] = &[SWATCH_ADD, SWATCH_SPOT_ADD, SWATCH_RENAME, SWATCH_EDIT, SWATCH_UPDATE, SWATCH_MAKE_GLOBAL, SWATCH_DELETE, SWATCH_APPLY, SWATCH_APPLY_TO_SELECTION, SWATCH_ADD_FROM_SELECTION];

pub fn parse_colour(value: &Value) -> Result<Colour, CommandError> {
    match value {
        Value::String(text) => match crate::paint::normalize_legacy_paint_alias(Some(text)) {
            Ok(Paint::Solid { colour }) => Ok(colour),
            _ => Err(CommandError::invalid_params()),
        },
        Value::Object(_) => serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params()),
        _ => Err(CommandError::invalid_params()),
    }
}

fn colour_param(params: &Value) -> Option<&Value> {
    params.get("colour").or_else(|| params.get("color"))
}

fn id_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim_start_matches("swatch:").parse().ok(),
        _ => None,
    }
}

fn swatch_id(params: &Value) -> Result<u64, CommandError> {
    params.get("id").or_else(|| params.get("swatch")).and_then(id_value).ok_or_else(CommandError::invalid_params)
}

fn index(session: &Session, id: u64) -> Result<usize, CommandError> {
    session.document.swatches.iter().position(|swatch| swatch.id == id).ok_or_else(|| CommandError::new(SWATCH_NOT_FOUND))
}

fn flag(params: &Value, key: &str) -> Result<Option<bool>, CommandError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_bool().map(Some).ok_or_else(CommandError::invalid_params),
    }
}

fn name_param(params: &Value) -> Result<Option<String>, CommandError> {
    match params.get("name") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) if !name.trim().is_empty() => Ok(Some(name.clone())),
        _ => Err(CommandError::invalid_params()),
    }
}

fn hex(colour: &Colour) -> String {
    if colour.a < 255 {
        format!("#{:02X}{:02X}{:02X}{:02X}", colour.r, colour.g, colour.b, colour.a)
    } else {
        format!("#{:02X}{:02X}{:02X}", colour.r, colour.g, colour.b)
    }
}

fn target_ids(session: &Session, params: &Value) -> Result<Vec<ObjectId>, CommandError> {
    let listed = params.get("ids").or_else(|| params.get("target")).or_else(|| params.get("targets"));
    let ids = match listed {
        Some(Value::Array(items)) => items.iter().map(|item| match item {
            Value::Number(number) => number.as_u64().map(ObjectId),
            Value::String(text) => text.trim_start_matches("node:").parse().ok().map(ObjectId),
            _ => None,
        }).collect::<Option<Vec<_>>>().ok_or_else(CommandError::invalid_params)?,
        Some(_) => return Err(CommandError::invalid_params()),
        None => session.selection.ids.clone(),
    };
    if ids.is_empty() {
        return Err(CommandError::invalid_params());
    }
    for id in &ids {
        if !session.document.objects.iter().any(|object| object.id == *id) {
            return Err(CommandError::not_found());
        }
    }
    Ok(ids)
}

fn index_param(params: &Value, key: &str) -> Result<Option<usize>, CommandError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(|index| Some(index as usize)).ok_or_else(CommandError::invalid_params),
    }
}

fn target_param(params: &Value) -> Result<SwatchTarget, CommandError> {
    match params.get("to").and_then(Value::as_str) {
        None | Some("fill") => Ok(SwatchTarget::Fill),
        Some("stroke") => Ok(SwatchTarget::Stroke),
        Some("text") => Ok(SwatchTarget::Text),
        Some(_) => Err(CommandError::invalid_params()),
    }
}

fn snapshot(session: &Session) -> Result<Value, CommandError> {
    Ok(json!({
        "swatches": serde_json::to_value(&session.document.swatches).map_err(|_| CommandError::invalid_params())?,
        "next_swatch_id": session.document.next_swatch_id,
        "bindings": serde_json::to_value(&session.document.swatch_bindings).map_err(|_| CommandError::invalid_params())?,
        "objects": serde_json::to_value(&session.document.objects).map_err(|_| CommandError::invalid_params())?,
        "stories": serde_json::to_value(&session.document.stories).map_err(|_| CommandError::invalid_params())?
    }))
}

fn restore(session: &mut Session, value: &Value) -> Result<(), CommandError> {
    let swatches = serde_json::from_value(value.get("swatches").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())?;
    let next = value.get("next_swatch_id").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?;
    if let Some(objects) = value.get("objects") {
        session.document.objects = serde_json::from_value(objects.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(stories) = value.get("stories") {
        session.document.stories = serde_json::from_value(stories.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(bindings) = value.get("bindings") {
        session.document.swatch_bindings = serde_json::from_value(bindings.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    session.document.swatches = swatches;
    session.document.next_swatch_id = next;
    session.document.version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    Ok(())
}

fn commit(session: &mut Session, name: &str, before: Value, extra: Value) -> CommandResult {
    let after = snapshot(session)?;
    session.undo.push(crate::undo::Change {
        name: name.to_string(),
        undo: Command::with_params(SWATCH_RESTORE, before),
        redo: Command::with_params(SWATCH_RESTORE, after),
    });
    session.generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    session.document.version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let mut reply = json!({ "doc_version": session.generation, "changes": [{ "name": name }] });
    if let (Some(map), Value::Object(more)) = (reply.as_object_mut(), extra) {
        map.extend(more);
    }
    Ok(reply)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let name = command.name.as_str();
    (name == SWATCH_LIST || MUTATING.contains(&name)).then_some(session.has_document)
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != SWATCH_RESTORE {
        return None;
    }
    Some(restore(session, &command.params))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    if name == SWATCH_LIST {
        return Some(if session.has_document {
            serde_json::to_value(&session.document.swatches).map(|swatches| json!({ "swatches": swatches })).map_err(|_| CommandError::invalid_params())
        } else {
            Err(CommandError::no_document())
        });
    }
    if !MUTATING.contains(&name) {
        return None;
    }
    if (name == SWATCH_ADD || name == SWATCH_SPOT_ADD) && command.params.get("scope").and_then(Value::as_str).and_then(crate::palette::PaletteScope::parse) == Some(crate::palette::PaletteScope::Application) {
        return Some(add_application(name, &command.params));
    }
    if !session.has_document {
        return Some(Err(CommandError::no_document()));
    }
    Some(run_mutating(session, name, &command.params))
}

fn build_swatch(name: &str, params: &Value, id: u64) -> Result<Swatch, CommandError> {
    let colour = parse_colour(colour_param(params).ok_or_else(CommandError::invalid_params)?)?;
    let label = name_param(params)?;
    if name == SWATCH_SPOT_ADD && label.is_none() {
        return Err(CommandError::invalid_params());
    }
    let mut swatch = Swatch::new(id, &label.unwrap_or_else(|| hex(&colour)), colour);
    swatch.spot = name == SWATCH_SPOT_ADD || flag(params, "spot")?.unwrap_or(false);
    swatch.global = flag(params, "global")?.unwrap_or(swatch.spot);
    swatch.overprint = flag(params, "overprint")?.unwrap_or(false);
    swatch.registration = flag(params, "registration")?.unwrap_or(false);
    Ok(swatch)
}

fn add_application(name: &str, params: &Value) -> CommandResult {
    let palette_name = match params.get("palette") {
        None | Some(Value::Null) => crate::palette::APP_DEFAULT.to_string(),
        Some(Value::String(text)) if !text.trim().is_empty() => text.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    crate::palette::with_app(true, |store| {
        let id = store.next_id();
        let mut swatch = build_swatch(name, params, id)?;
        swatch.palette = palette_name.clone();
        let at = store.ensure(&palette_name);
        store.palettes[at].swatches.push(swatch);
        store.next_id = id + 1;
        Ok(json!({ "id": id, "scope": "application", "palette": palette_name, "changes": [] }))
    })
}

fn document_palette(session: &Session, params: &Value) -> Result<String, CommandError> {
    let palette = match params.get("palette") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(name)) => name.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    if !crate::commands::palettes::doc_has_palette(session, &palette) {
        return Err(CommandError::new(crate::palette::PALETTE_NOT_FOUND));
    }
    Ok(palette)
}

fn run_mutating(session: &mut Session, name: &str, params: &Value) -> CommandResult {
    let before = snapshot(session)?;
    match name {
        SWATCH_ADD | SWATCH_SPOT_ADD => {
            match params.get("scope").and_then(Value::as_str) {
                None | Some("document") => {}
                Some("application") | Some("system") => return Err(CommandError::new("NOT_IMPLEMENTED")),
                Some(_) => return Err(CommandError::invalid_params()),
            }
            let id = next_id(&session.document);
            let mut swatch = build_swatch(name, params, id)?;
            swatch.palette = document_palette(session, params)?;
            session.document.swatches.push(swatch);
            session.document.next_swatch_id = id + 1;
            commit(session, name, before, json!({ "id": id }))
        }
        SWATCH_RENAME => {
            let id = swatch_id(params)?;
            let at = index(session, id)?;
            let label = name_param(params)?.ok_or_else(CommandError::invalid_params)?;
            session.document.swatches[at].name = label;
            commit(session, name, before, json!({ "id": id }))
        }
        SWATCH_EDIT | SWATCH_UPDATE => {
            let id = swatch_id(params)?;
            let at = index(session, id)?;
            let label = name_param(params)?;
            let colour = colour_param(params).map(parse_colour).transpose()?;
            let flags = [flag(params, "global")?, flag(params, "spot")?, flag(params, "overprint")?, flag(params, "registration")?];
            if label.is_none() && colour.is_none() && flags.iter().all(Option::is_none) {
                return Err(CommandError::invalid_params());
            }
            let swatch = &mut session.document.swatches[at];
            if let Some(label) = label { swatch.name = label; }
            let old = swatch.colour;
            if let Some(colour) = colour { swatch.colour = colour; }
            if let Some(value) = flags[0] { swatch.global = value; }
            if let Some(value) = flags[1] { swatch.spot = value; }
            if let Some(value) = flags[2] { swatch.overprint = value; }
            if let Some(value) = flags[3] { swatch.registration = value; }
            let (global, new) = (swatch.global, swatch.colour);
            let (updated, stories) = if global && new != old { propagate_all(&mut session.document, id, old, new) } else { (Vec::new(), Vec::new()) };
            if !global { detach(&mut session.document, id); }
            commit(session, name, before, json!({ "id": id, "updated": updated, "updated_stories": stories }))
        }
        SWATCH_MAKE_GLOBAL => {
            let id = swatch_id(params)?;
            let at = index(session, id)?;
            let global = flag(params, "global")?.unwrap_or(true);
            session.document.swatches[at].global = global;
            if !global { detach(&mut session.document, id); }
            commit(session, name, before, json!({ "id": id }))
        }
        SWATCH_DELETE => {
            let id = swatch_id(params)?;
            let at = index(session, id)?;
            session.document.swatches.remove(at);
            detach(&mut session.document, id);
            commit(session, name, before, json!({ "id": id }))
        }
        SWATCH_ADD_FROM_SELECTION => {
            let target = target_param(params)?;
            let ids = target_ids(session, params)?;
            let object = session.document.objects.iter().find(|object| object.id == ids[0]).ok_or_else(CommandError::not_found)?;
            let colour = match crate::colour::swatch::target_paint(object, target) {
                Paint::Solid { colour } => *colour,
                _ => return Err(CommandError::invalid_params()),
            };
            let id = next_id(&session.document);
            let label = name_param(params)?.unwrap_or_else(|| hex(&colour));
            let mut swatch = Swatch::new(id, &label, colour);
            swatch.palette = document_palette(session, params)?;
            swatch.global = flag(params, "global")?.unwrap_or(false);
            let global = swatch.global;
            session.document.swatches.push(swatch);
            session.document.next_swatch_id = id + 1;
            if global {
                bind(&mut session.document, ids[0], target, Some(id));
            }
            commit(session, name, before, json!({ "id": id }))
        }
        _ => {
            let id = swatch_id(params)?;
            let (colour, global) = if params.get("scope").and_then(Value::as_str)
                .and_then(crate::palette::PaletteScope::parse) == Some(crate::palette::PaletteScope::Application)
            {
                let name = match params.get("palette") {
                    None | Some(Value::Null) => crate::palette::APP_DEFAULT,
                    Some(Value::String(name)) => name,
                    _ => return Err(CommandError::invalid_params()),
                };
                let palettes = crate::palette::app_palettes();
                let palette = palettes.iter().find(|palette| palette.name == name)
                    .ok_or_else(|| CommandError::new(crate::palette::PALETTE_NOT_FOUND))?;
                let swatch = palette.swatches.iter().find(|swatch| swatch.id == id)
                    .ok_or_else(CommandError::not_found)?;
                (swatch.colour, false)
            } else {
                let at = index(session, id)?;
                let swatch = &session.document.swatches[at];
                (swatch.colour, swatch.global)
            };
            let target = target_param(params)?;
            let stop = index_param(params, "stop")?;
            if target == SwatchTarget::Text {
                let story = params.get("story").and_then(id_value).map(crate::document::StoryId).ok_or_else(CommandError::invalid_params)?;
                let range = index_param(params, "range")?;
                set_story_colour(&mut session.document, story, range, colour).ok_or_else(CommandError::not_found)?;
                let range_id = match (global, range) {
                    (true, Some(at)) => ensure_range_id(&mut session.document, story, at),
                    _ => None,
                };
                let key = SwatchBinding { story: Some(story), range, range_id, ..SwatchBinding::paint(ObjectId(0), SwatchTarget::Text, id) };
                bind_use(&mut session.document, key, global.then_some(id));
                return commit(session, name, before, json!({ "id": id, "story": story.0, "range_id": range_id }));
            }
            let ids = target_ids(session, params)?;
            if let Some(index) = stop {
                let fits = session.document.objects.iter().filter(|object| ids.contains(&object.id)).all(|object| {
                    let paint = if target == SwatchTarget::Stroke { &object.stroke.paint } else { &object.fill };
                    matches!(paint, Paint::Gradient { stops, .. } if index < stops.len())
                });
                if !fits {
                    return Err(CommandError::invalid_params());
                }
            }
            for object in session.document.objects.iter_mut().filter(|object| ids.contains(&object.id)) {
                match stop {
                    Some(index) => {
                        if !set_stop_colour(object, target, index, colour) {
                            return Err(CommandError::invalid_params());
                        }
                    }
                    None => crate::colour::swatch::set_target_paint(object, target, Paint::Solid { colour }),
                }
            }
            for object in &ids {
                match stop {
                    Some(index) => bind_use(&mut session.document, SwatchBinding { stop: Some(index), ..SwatchBinding::paint(*object, target, id) }, global.then_some(id)),
                    None => bind(&mut session.document, *object, target, global.then_some(id)),
                }
            }
            let applied: Vec<u64> = ids.iter().map(|id| id.0).collect();
            commit(session, name, before, json!({ "id": id, "applied": applied }))
        }
    }
}
