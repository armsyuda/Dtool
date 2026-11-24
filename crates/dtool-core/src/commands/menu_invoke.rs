use crate::{Command,CommandError,CommandResult,Session};
use serde_json::Value;

pub fn can_execute(_session:&Session,command:&Command)->Option<bool>{(command.name=="ui.menu.invoke").then_some(super::menus::provider_ready())}

fn find<'a>(items:&'a Value,id:&str)->Option<&'a Value>{
    for item in items.as_array()? {
        if item.get("id").and_then(Value::as_str)==Some(id){return Some(item);}
        if let Some(value)=item.get("children").and_then(|children|find(children,id)){return Some(value);}
    }
    None
}

pub fn run(session:&mut Session,command:&Command)->Option<CommandResult>{
    if command.name!="ui.menu.invoke"{return None;}
    Some((||{
        let id=command.params.get("id").and_then(Value::as_str).ok_or_else(CommandError::invalid_params)?;
        let persona=command.params.get("persona").and_then(Value::as_str).unwrap_or("pixel");
        let mut tree=super::menus::menu_definition(persona).ok_or_else(||CommandError::new("NOT_AVAILABLE"))?;
        super::menus::resolve_items(&mut tree,session);
        let item=find(&tree,id).ok_or_else(CommandError::not_found)?;
        if item.get("planned").and_then(Value::as_bool).unwrap_or(true){return Err(CommandError::not_implemented());}
        if !item.get("enabled").and_then(Value::as_bool).unwrap_or(false){return Err(CommandError::new("CANNOT_EXECUTE"));}
        let name=item.get("command").and_then(Value::as_str).ok_or_else(CommandError::not_implemented)?;
        if name=="ui.menu.invoke"{return Err(CommandError::invalid_params());}
        let mut params=item.get("params").cloned().filter(Value::is_object).unwrap_or_else(||serde_json::json!({}));
        if let (Some(base),Some(extra))=(params.as_object_mut(),command.params.get("params").and_then(Value::as_object)){
            for (key,value) in extra {base.insert(key.clone(),value.clone());}
        }
        session.run_named(name,params)
    })())
}
