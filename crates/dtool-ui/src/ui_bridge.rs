use crate::dialogs::locked_delete::{LockedDeleteAnswer, LockedDeleteDialog};
use crate::menus::vector_warp;
use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_core::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::json;

pub const CONTEXT_NOT_ACTIVE: &str = "CONTEXT_NOT_ACTIVE";

#[derive(Debug, Clone, Copy, Default)]
pub struct UiBridge;

fn not_active() -> CommandError {
    CommandError::new(CONTEXT_NOT_ACTIVE)
}

fn active_session(ws: &mut CoreWorkspace) -> Result<&mut Session, CommandError> {
    ws.session_mut().ok_or_else(not_active)
}

fn run(ws: &mut CoreWorkspace, command: Command) -> CommandResult {
    if ws.active().is_none() {
        return Err(not_active());
    }
    ws.run(Scope::active(), command).1
}

impl UiBridge {
    pub fn needs_selection(action: &str) -> bool {
        matches!(action, "group" | "ungroup" | "lock" | "unlock" | "hide" | "show" | "delete"
            | "flip_horizontal" | "flip_vertical" | "rotate_left" | "rotate_right")
    }

    pub fn invoke_action(ws: &mut CoreWorkspace, dialog: &mut LockedDeleteDialog, action: &str) -> CommandResult {
        let session = active_session(ws)?;
        let nodes = LockedDeleteDialog::selected_nodes(session);
        if Self::needs_selection(action) && nodes.is_empty() {
            return Err(CommandError::invalid_params());
        }
        let command = match action {
            "group" => Command::with_params("node.group", json!({ "nodes": nodes })),
            "new_layer_folder" => Command::with_params("node.folder.create", json!({ "name": dtool_strings::text("layers.folder") })),
            "new_pixel_layer" | "new_fill_layer" => {
                let (name, params) = crate::menu_tree::bridge_command(action).ok_or_else(CommandError::not_implemented)?;
                Command::with_params(name, params)
            }
            "ungroup" => Command::with_params("node.ungroup", json!({ "nodes": nodes })),
            "ungroup_all" => Command::with_params("node.ungroup", json!({ "all": true })),
            "lock" => Command::with_params("node.lock_state", json!({ "nodes": nodes, "state": "locked" })),
            "unlock" => Command::with_params("node.lock_state", json!({ "nodes": nodes, "state": "unlocked" })),
            "unlock_all" => Command::with_params("node.unlock_all", json!({})),
            "hide" => Command::with_params("node.update", json!({ "nodes": nodes, "set": { "visible": false } })),
            "show" => Command::with_params("node.update", json!({ "nodes": nodes, "set": { "visible": true } })),
            "show_all" => Command::with_params("node.show_all", json!({})),
            "arrange_to_front" => Command::with_params("node.arrange", json!({ "nodes": nodes, "step": "to_front" })),
            "arrange_forward" => Command::with_params("node.arrange", json!({ "nodes": nodes, "step": "forward" })),
            "arrange_backward" => Command::with_params("node.arrange", json!({ "nodes": nodes, "step": "backward" })),
            "arrange_to_back" => Command::with_params("node.arrange", json!({ "nodes": nodes, "step": "to_back" })),
            "flip_horizontal" | "flip_vertical" | "rotate_left" | "rotate_right" => {
                let (name, mut params) = crate::menu_tree::bridge_command(action).ok_or_else(CommandError::not_implemented)?;
                params["ids"] = json!(nodes);
                Command::with_params(name, params)
            }
            "new_warp_group" => vector_warp::new_warp_group_command(session),
            "delete" => return dialog.request_delete(session, nodes),
            "delete_confirm_yes" => {
                return dialog
                    .resolve(session, LockedDeleteAnswer::Yes)
                    .unwrap_or_else(|| Err(CommandError::invalid_params()))
            }
            "delete_confirm_no" => {
                dialog.resolve(session, LockedDeleteAnswer::No);
                return Ok(json!({ "kept": true }));
            }
            _ => return Err(CommandError::not_implemented()),
        };
        run(ws, command)
    }

    pub fn invoke_menu_item(ws: &mut CoreWorkspace, id: &str) -> CommandResult {
        let command = vector_warp::menu_command(active_session(ws)?, id).ok_or_else(CommandError::not_implemented)?;
        run(ws, command)
    }

    pub fn choose_warp_preset(ws: &mut CoreWorkspace, preset: &str, value: Option<f64>) -> CommandResult {
        let command = vector_warp::preset_choice_command(preset, value, None)?;
        run(ws, command)
    }

    pub fn set_warp_value(ws: &mut CoreWorkspace, value: f64) -> CommandResult {
        let command = vector_warp::value_command(value, None)?;
        run(ws, command)
    }

    pub fn reset_warp_mesh(ws: &mut CoreWorkspace) -> CommandResult {
        run(ws, vector_warp::reset_command(None))
    }

    pub fn query_warp_group(ws: &mut CoreWorkspace) -> CommandResult {
        run(ws, vector_warp::group_query_command())
    }

    pub fn query_warp_mesh(ws: &mut CoreWorkspace) -> CommandResult {
        run(ws, vector_warp::mesh_query_command())
    }

    pub fn query_menu(menu: &str) -> Vec<String> {
        match menu {
            "layer" => vec![
                "menu.layer.group".to_string(),
                "menu.layer.ungroup".to_string(),
                "menu.layer.ungroup_all".to_string(),
                "menu.layer.new_group".to_string(),
                "menu.layer.lock".to_string(),
                "menu.layer.unlock".to_string(),
                "menu.layer.unlock_all".to_string(),
                "menu.layer.hide".to_string(),
                "menu.layer.show".to_string(),
                "menu.layer.show_all".to_string(),
            ],
            "arrange" => vec![
                "menu.arrange.to_front".to_string(),
                "menu.arrange.forward".to_string(),
                "menu.arrange.backward".to_string(),
                "menu.arrange.to_back".to_string(),
            ],
            "vector.new_warp_group" => vector_warp::menu_keys(),
            "warp.preset" => vector_warp::PRESET_CHOICES.iter().map(|(_, key)| key.to_string()).collect(),
            _ => vec![],
        }
    }

    pub fn run_warp_command(ws: &mut CoreWorkspace, preset: &str) -> CommandResult {
        let session = active_session(ws)?;
        let nodes = LockedDeleteDialog::selected_nodes(session);
        if nodes.is_empty() {
            return Err(CommandError::invalid_params());
        }
        let preset = dtool_core::commands::warp::parse_preset(preset).ok_or_else(|| CommandError::new(dtool_core::commands::warp::INVALID_PRESET))?;
        let command = vector_warp::create_command(session, &preset, None)?;
        run(ws, command)
    }
}
