use crate::command::Session;
use crate::defaults::ObjectDefaults;
use dtool_protocol::{Command, CommandError, CommandResult};
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

pub fn set_paints(fill: Option<crate::paint::Paint>, stroke: Option<crate::paint::Paint>) {
    let mut defaults = store();
    if let Some(fill) = fill { defaults.fill = fill; }
    if let Some(stroke) = stroke { defaults.stroke.paint = stroke; }
}

fn ours(command: &Command) -> bool {
    matches!(command.name.as_str(), "defaults.sync" | "defaults.save" | "defaults.reset" | "defaults.revert" | "defaults.query" | "defaults.apply")
}

fn previous_param(command: &Command) -> Result<ObjectDefaults, CommandError> {
    let value = command.params.get("previous").cloned().ok_or_else(CommandError::invalid_params)?;
    serde_json::from_value::<ObjectDefaults>(value).map(ObjectDefaults::with_pt_unit).map_err(|_| CommandError::invalid_params())
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !ours(command) {
        return None;
    }
    Some(match command.name.as_str() {
        "defaults.sync" | "defaults.apply" => !session.selection.ids.is_empty(),
        "defaults.revert" => previous_param(command).is_ok(),
        _ => true,
    })
}

fn apply_to_selection(session: &mut Session, defaults: &ObjectDefaults) -> Result<Vec<Command>, CommandError> {
    let mut undo = Vec::new();
    let stroke = defaults.stroke_for_document(&session.document);
    for id in session.selection.ids.clone() {
        let obj = session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        undo.push(Command::with_params("object.update", serde_json::json!({ "id": id.0, "fill": obj.fill, "stroke": obj.stroke })));
        obj.fill = defaults.fill.clone();
        obj.stroke = stroke.clone();
    }
    Ok(undo)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !ours(command) {
        return None;
    }
    let mut defaults = store();
    let previous = defaults.clone();
    let result = match command.name.as_str() {
        "defaults.sync" => {
            defaults.sync_from_selection(&session.document, &session.selection);
            Ok(())
        }
        "defaults.save" => defaults.save_app().map_err(|_| CommandError::new("IO_ERROR")),
        "defaults.reset" => {
            defaults.reset();
            Ok(())
        }
        "defaults.revert" => previous_param(command).map(|prev| defaults.revert(&prev)),
        "defaults.apply" => apply_to_selection(session, &defaults).map(|undo| {
            if !undo.is_empty() {
                let undo = if undo.len() == 1 {
                    undo.into_iter().next().unwrap_or_else(Command::no_op)
                } else {
                    Command::with_params("edit.batch", serde_json::json!({ "commands": undo }))
                };
                session.undo.push(crate::undo::Change { name: command.name.clone(), redo: command.clone(), undo });
                session.generation += 1;
                session.document.version += 1;
            }
        }),
        _ => Ok(()),
    };
    Some(result.map(|_| serde_json::json!({ "previous": previous, "defaults": *defaults, "is_default": defaults.is_default() })))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if !ours(command) {
        return None;
    }
    if command.name == "defaults.apply" {
        let defaults = current();
        return Some(apply_to_selection(session, &defaults).map(|_| ()));
    }
    Some(run(session, command).unwrap_or_else(|| Err(CommandError::not_implemented())).map(|_| ()))
}

pub fn undo(_session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if !ours(command) {
        return None;
    }
    if command.name == "defaults.save" || command.name == "defaults.query" || command.name == "defaults.apply" {
        return Some(Ok(()));
    }
    Some(previous_param(command).map(|prev| store().revert(&prev)))
}
