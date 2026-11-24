use crate::command::Session;
use dtool_protocol::{Command, CommandError};
use serde_json::{json, Value};
use std::sync::OnceLock;

static PROVIDER: OnceLock<fn(&str) -> Value> = OnceLock::new();

pub fn register_menu_provider(provider: fn(&str) -> Value) -> bool {
    PROVIDER.set(provider).is_ok() || PROVIDER.get().is_some()
}

pub fn provider_ready() -> bool { PROVIDER.get().is_some() }

pub type MenuRowGate = fn(&str, &Session) -> Option<bool>;

static ROW_GATE: OnceLock<MenuRowGate> = OnceLock::new();

pub fn register_menu_row_gate(gate: MenuRowGate) -> bool {
    ROW_GATE.set(gate).is_ok() || ROW_GATE.get().is_some()
}

pub fn row_gate_ready() -> bool { ROW_GATE.get().is_some() }

pub fn row_gate(id: &str, session: &Session) -> Option<bool> {
    ROW_GATE.get().and_then(|gate| gate(id, session))
}

type MenuRowState = fn(&str, &Session) -> Option<(Value, Option<bool>)>;
static ROW_STATE: OnceLock<MenuRowState> = OnceLock::new();

pub fn register_menu_row_state(resolve: MenuRowState) -> bool {
    ROW_STATE.set(resolve).is_ok() || ROW_STATE.get().is_some()
}

pub fn menu_definition(persona:&str)->Option<Value>{PROVIDER.get().map(|provider|provider(persona))}

pub fn resolve_items(items:&mut Value,session:&Session){state(items,session);}

fn state(items:&mut Value,session:&Session){
    if let Some(items)=items.as_array_mut(){
        for item in items {
            let candidate=item.get("command").and_then(Value::as_str).or_else(||item.get("id").and_then(Value::as_str)).unwrap_or("").to_string();
            let host=item["host_action"].as_bool()==Some(true);
            let known=host||session.catalog.get(&candidate).is_some_and(|spec|spec.implemented)||session.can_execute_named(&candidate);
            let needs_selection=item.get("needs_selection").and_then(Value::as_bool).unwrap_or(false);
            let id=item.get("id").and_then(Value::as_str);
            let gated=id.and_then(|id|row_gate(id,session));
            let resolved = ROW_STATE.get().and_then(|resolve| id.and_then(|id| resolve(id, session)));
            let can_execute = if let Some((params, checked)) = resolved {
                let can_execute = session.can_execute(&Command::with_params(&candidate, params.clone()));
                item["params"] = params;
                item["checked"] = json!(checked);
                can_execute
            } else {
                session.can_execute_named(&candidate)
            };
            let by_command=known&&can_execute&&!(needs_selection&&session.selection.ids.is_empty());
            let enabled = if host {
                (session.has_document || item["requires_document"].as_bool() == Some(false))
                    && !(needs_selection && session.selection.ids.is_empty())
                    && gated.unwrap_or(true)
            } else {
                match gated {
                    Some(on) => known && session.has_document && on,
                    None => by_command,
                }
            };
            if known && !host {item["command"]=Value::String(candidate);}
            if let Some(children)=item.get_mut("children"){state(children,session);}
            let parent=item.get("children").and_then(Value::as_array).is_some_and(|v|!v.is_empty());
            let separator=item.get("separator").and_then(Value::as_bool).unwrap_or(false);
            let planned=!separator&&!parent&&!known;
            item["planned"]=Value::Bool(planned);
            item["enabled"]=Value::Bool(!separator&&(parent||enabled));
            item["planned_key"]=if planned {Value::String("menu.planned.suffix".into())}else{Value::Null};
            item["error_code"]=if planned {Value::String("NOT_IMPLEMENTED".into())}else{Value::Null};
        }
    }
}

fn layer_menu() -> Value {
    json!({
        "menu": "layer",
        "items": [
            {"id": "menu.layer.group", "command": "node.group"},
            {"id": "menu.layer.ungroup", "command": "node.ungroup"},
            {"id": "menu.layer.ungroup_all", "command": "node.ungroup"},
            {"id": "menu.layer.new_group", "command": "node.group"},
            {"id": "menu.layer.new_folder", "command": "node.folder.create"},
            {"id": "menu.layer.lock", "command": "node.lock_state"},
            {"id": "menu.layer.unlock", "command": "node.lock_state"},
            {"id": "menu.layer.unlock_all", "command": "node.unlock_all"},
            {"id": "menu.layer.hide", "command": "node.update"},
            {"id": "menu.layer.show", "command": "node.update"},
            {"id": "menu.layer.show_all", "command": "node.show_all"},
            {"id": "layer.blend_options", "host_action": true, "needs_selection": true}
        ]
    })
}

fn find_menu<'a>(items: &'a Value, id: &str) -> Option<&'a Value> {
    items.as_array()?.iter().find_map(|item| {
        if item["id"].as_str() == Some(id) { Some(item) }
        else { find_menu(&item["children"], id) }
    })
}

pub fn run(session: &Session, command: &Command) -> Option<Result<Value, CommandError>> {
    if command.name != "ui.menu.query" { return None; }
    Some((|| {
        let menu = command.params.get("menu").and_then(Value::as_str);
        if menu == Some("layer") {
            let mut result = layer_menu();
            if let Some(items) = result.get_mut("items") {
                state(items, session);
            }
            return Ok(result);
        }
        let persona = match command.params.get("persona") {
            Some(value) => value.as_str().ok_or_else(CommandError::invalid_params)?,
            None => "pixel",
        };
        let provider = match PROVIDER.get() {
            Some(provider) => provider,
            None => return Err(CommandError::new("NOT_AVAILABLE")),
        };
        let mut result = provider(persona);
        state(&mut result, session);
        if menu == Some("table") {
            let table = find_menu(&result, "text.table").ok_or_else(|| CommandError::new("NOT_AVAILABLE"))?;
            return Ok(json!({"menu": "table", "items": table["children"]}));
        }
        Ok(result)
    })())
}

pub fn can_execute(_session: &Session, command: &Command) -> Option<bool> {
    if command.name != "ui.menu.query" {
        return None;
    }
    Some(true)
}
