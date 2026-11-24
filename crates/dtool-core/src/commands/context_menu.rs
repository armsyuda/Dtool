use crate::{Command,CommandError,CommandResult,Session};
use serde_json::Value;
use std::sync::OnceLock;
static PROVIDER:OnceLock<fn(&str,&str)->Value>=OnceLock::new();
pub fn register_provider(provider:fn(&str,&str)->Value)->bool{PROVIDER.set(provider).is_ok()||PROVIDER.get().is_some()}
pub fn can_execute(_session:&Session,command:&Command)->Option<bool>{(command.name=="ui.context_menu.query").then_some(PROVIDER.get().is_some())}
pub fn run(session:&mut Session,command:&Command)->Option<CommandResult>{
    if command.name!="ui.context_menu.query"{return None;}
    Some((||{
        let target=command.params.get("target").and_then(Value::as_str).ok_or_else(CommandError::invalid_params)?;
        if !["Pasteboard","Object","Text","Group","TextEditing"].iter().any(|kind|kind.eq_ignore_ascii_case(target)){return Err(CommandError::invalid_params());}
        let persona=command.params.get("persona").and_then(Value::as_str).unwrap_or("pixel");
        let provider=PROVIDER.get().ok_or_else(||CommandError::new("NOT_AVAILABLE"))?;
        let mut rows=provider(persona,target);super::menus::resolve_items(&mut rows,session);Ok(rows)
    })())
}
