use crate::menu_tree::MenuId;
use dtool_core::Session;
use dtool_protocol::CommandError;
use serde_json::{json, Value};

pub const INVOKE: &str = "ui.menu.invoke";

#[derive(Debug, Clone, PartialEq)]
pub struct PanelAction {
    pub id: MenuId,
    pub key: &'static str,
    pub command: Option<&'static str>,
}

impl PanelAction {
    pub const fn new(id: &'static str, key: &'static str, command: Option<&'static str>) -> Self {
        Self { id: MenuId(id), key, command }
    }
}

pub const PANEL_ACTIONS: &[PanelAction] = &[
    PanelAction::new("panel.menu.close", "panel.menu.close", None),
];

pub fn panel_action_ids() -> Vec<&'static str> {
    PANEL_ACTIONS.iter().map(|action| action.id.0).collect()
}

pub fn find_panel_action(id: &str) -> Option<&'static PanelAction> {
    PANEL_ACTIONS.iter().find(|action| action.id.0 == id)
}

pub fn get_close_action() -> PanelAction {
    PANEL_ACTIONS[0].clone()
}

pub fn invoke_panel_action(session: &mut Session, action_id: &str) -> Result<Value, CommandError> {
    let action = find_panel_action(action_id);
    match action.and_then(|action| action.command) {
        Some(command) => session.run_named(command, json!({})),
        None if action.is_some() => Err(CommandError::not_implemented()),
        None => session.run_named(INVOKE, json!({ "id": action_id })),
    }
}
