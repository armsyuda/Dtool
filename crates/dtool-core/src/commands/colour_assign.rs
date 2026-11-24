use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::json;

fn profile(command: &Command) -> Result<&str, CommandError> {
    command.params.get("profile").and_then(|value| value.as_str()).filter(|value| !value.is_empty()).ok_or_else(CommandError::invalid_params)
}
pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == "color.profile.assign").then(|| session.has_document && profile(command).is_ok())
}
pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != "color.profile.restore" { return None; }
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        let name = profile(command)?;
        let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        session.document.colour.profile = name.to_string();
        session.document.version = version;
        Ok(())
    })())
}
pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name != "color.profile.assign" { return None; }
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        let name = profile(command)?;
        let generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        let previous = session.document.colour.profile.clone();
        let services = crate::colour::services().read().map_err(|_| CommandError::new("INTERNAL"))?;
        crate::colour::profiles::assign_profile(&mut session.document, name, &services.profiles)?;
        drop(services);
        if previous != name {
            session.undo.push(crate::undo::Change { name: command.name.clone(), undo: Command::with_params("color.profile.restore", json!({"profile": previous})), redo: Command::with_params("color.profile.restore", json!({"profile": name})) });
            session.generation = generation;
            session.document.version = version;
        }
        Ok(json!({"doc_version": session.generation, "profile": name, "changes": if previous == name { Vec::<serde_json::Value>::new() } else { vec![json!({"name": command.name})] }}))
    })())
}
