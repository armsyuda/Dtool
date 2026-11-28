use crate::command::Session;
use crate::defaults::{EffectDefaults, ObjectDefaults};
use crate::layer_style::LayerEffect;
use crate::object::Object;
use crate::paint::Paint;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::json;
use std::sync::{Mutex, MutexGuard, OnceLock};

static STORE: OnceLock<Mutex<ObjectDefaults>> = OnceLock::new();

fn store() -> MutexGuard<'static, ObjectDefaults> {
    let lock = STORE.get_or_init(|| Mutex::new(ObjectDefaults::load_app().unwrap_or_default()));
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn current() -> ObjectDefaults {
    store().clone()
}

pub fn for_document(document: &crate::Document) -> ObjectDefaults {
    document.object_defaults.clone().unwrap_or_else(current)
}

pub fn for_session(session: Option<&Session>) -> ObjectDefaults {
    session.map_or_else(current, |session| for_document(&session.document))
}

pub fn set_paints(session: Option<&mut Session>, fill: Option<Paint>, stroke: Option<Paint>) {
    let previous = for_session(session.as_deref());
    let mut next = previous.clone();
    if let Some(fill) = fill { next.fill = fill; }
    if let Some(stroke) = stroke { next.stroke.paint = stroke; }

    if let Some(session) = session {
        commit_defaults(session, "defaults.paint", &previous, &next);
    } else {
        *store() = next;
    }
}

pub(crate) fn effects_for_creation(
    document: &crate::Document, size: [f32; 2], params: &serde_json::Value,
) -> Result<Vec<LayerEffect>, CommandError> {
    if let Some(value) = params.get("effects") {
        let effects: Vec<LayerEffect> = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        return effects.iter().all(LayerEffect::is_valid).then_some(effects).ok_or_else(CommandError::invalid_params);
    }
    let effects = match params.get("effect_defaults") {
        Some(value) => serde_json::from_value::<EffectDefaults>(value.clone()).map_err(|_| CommandError::invalid_params())?,
        None => for_document(document).effects,
    };
    Ok(effects.for_object(crate::units::units_per_point(document.units, document.dpi), size))
}

fn ours(command: &Command) -> bool {
    matches!(command.name.as_str(), "defaults.sync" | "defaults.save" | "defaults.reset" | "defaults.revert" | "defaults.query" | "defaults.apply")
}

fn previous_param(command: &Command) -> Result<ObjectDefaults, CommandError> {
    let value = command.params.get("previous").cloned().ok_or_else(CommandError::invalid_params)?;
    serde_json::from_value::<ObjectDefaults>(value).map(ObjectDefaults::with_pt_unit).map_err(|_| CommandError::invalid_params())
}

fn selection_is_editable(session: &Session) -> bool {
    !session.selection.ids.is_empty() && session.selection.ids.iter().all(|id| {
        session.document.get_object(*id).is_some_and(|object| !session.document.is_object_locked(object))
    })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !ours(command) {
        return None;
    }
    Some(match command.name.as_str() {
        "defaults.sync" => session.selection.ids.first().is_some_and(|id| session.document.get_object(*id).is_some()),
        "defaults.apply" => selection_is_editable(session),
        "defaults.revert" => command.params.get("previous").is_none() || previous_param(command).is_ok(),
        _ => true,
    })
}

fn apply_to_selection(session: &mut Session, defaults: &ObjectDefaults) -> Result<Vec<Object>, CommandError> {
    if !selection_is_editable(session) {
        return Err(CommandError::new("CANNOT_EXECUTE"));
    }
    let stroke = defaults.stroke_for_document(&session.document);
    let units_per_point = crate::units::units_per_point(session.document.units, session.document.dpi);
    let mut previous = Vec::new();
    for id in session.selection.ids.clone() {
        let object = session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        let effects = defaults.effects.for_object(units_per_point, object.size);
        if object.fill != defaults.fill || object.stroke != stroke || object.effects != effects {
            previous.push(object.clone());
            object.fill = defaults.fill.clone();
            object.stroke = stroke.clone();
            object.effects = effects;
        }
    }
    Ok(previous)
}

fn next_defaults(session: &Session, command: &Command, mut defaults: ObjectDefaults) -> Result<ObjectDefaults, CommandError> {
    match command.name.as_str() {
        "defaults.sync" => {
            if can_execute(session, command) != Some(true) {
                return Err(CommandError::new("CANNOT_EXECUTE"));
            }
            defaults.sync_from_selection(&session.document, &session.selection);
            Ok(defaults)
        }
        "defaults.reset" => Ok(ObjectDefaults::default()),
        "defaults.revert" if command.params.get("previous").is_some() => previous_param(command),
        "defaults.revert" => ObjectDefaults::load_app().map_err(|_| CommandError::new("IO_ERROR")),
        _ => Err(CommandError::not_implemented()),
    }
}

fn commit_defaults(session: &mut Session, name: &str, previous: &ObjectDefaults, next: &ObjectDefaults) -> bool {
    if previous == next {
        return false;
    }
    let restore = |value: &ObjectDefaults| Command::with_params("defaults.revert", json!({"previous": value}));
    session.undo.push(crate::undo::Change { name: name.to_owned(), redo: restore(next), undo: restore(previous) });
    session.document.object_defaults = Some(next.clone());
    session.generation += 1;
    session.document.version += 1;
    true
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !ours(command) {
        return None;
    }
    Some((|| {
        let previous = for_document(&session.document);
        let mut defaults = previous.clone();
        let changed = match command.name.as_str() {
            "defaults.query" => false,
            "defaults.save" => {
                defaults.save_app().map_err(|_| CommandError::new("IO_ERROR"))?;
                *store() = defaults.clone();
                false
            }
            "defaults.apply" => {
                let old = apply_to_selection(session, &defaults)?;
                let changed = !old.is_empty();
                if changed {
                    let redo_objects = old.iter().filter_map(|object| session.document.get_object(object.id)).collect::<Vec<_>>();
                    let redo = Command::with_params("object.restore", json!({"objects": redo_objects}));
                    session.push_restore_undo(&command.name, redo, old);
                }
                changed
            }
            _ => {
                defaults = next_defaults(session, command, defaults)?;
                commit_defaults(session, &command.name, &previous, &defaults)
            }
        };
        Ok(json!({
            "previous": previous, "defaults": defaults, "is_default": defaults.is_default(),
            "doc_version": session.generation, "changed": changed
        }))
    })())
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if !ours(command) {
        return None;
    }
    Some((|| {
        let defaults = for_document(&session.document);
        match command.name.as_str() {
            "defaults.query" => {}
            "defaults.save" => return Err(CommandError::new("CANNOT_EXECUTE")),
            "defaults.apply" => { apply_to_selection(session, &defaults)?; }
            _ => {
                let next = next_defaults(session, command, defaults)?;
                session.document.object_defaults = Some(next);
                session.document.version += 1;
            }
        }
        Ok(())
    })())
}
