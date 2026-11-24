use crate::dialogs::locked_delete::LockedDeleteDialog;
use dtool_core::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use dtool_strings::text;
use serde_json::{json, Value};

pub const LAYER_LOCK: &str = "layer.lock";
pub const LAYER_UNLOCK: &str = "layer.unlock";
pub const LAYER_UNLOCK_ALL: &str = "layer.unlock_all";
pub const LAYER_HIDE: &str = "layer.hide";
pub const LAYER_SHOW: &str = "layer.show";
pub const LAYER_SHOW_ALL: &str = "layer.show.all";
pub const EDIT_DELETE: &str = "edit.delete";

pub const LAYER_LOCK_ITEMS: &[(&str, &str)] = &[
    (LAYER_LOCK, "menu.layer.lock"),
    (LAYER_UNLOCK, "menu.layer.unlock"),
    (LAYER_UNLOCK_ALL, "menu.layer.unlock_all"),
];

pub const LAYER_VISIBILITY_ITEMS: &[(&str, &str)] = &[
    (LAYER_HIDE, "menu.layer.hide"),
    (LAYER_SHOW, "menu.layer.show"),
    (LAYER_SHOW_ALL, "menu.layer.show_all"),
];

fn node_list(nodes: &[u64]) -> Value {
    Value::Array(nodes.iter().map(|id| json!(id)).collect())
}

pub fn selected_nodes(session: &Session) -> Vec<u64> {
    session.selection.ids.iter().map(|id| id.0).collect()
}

pub fn command_for(action: &str, nodes: &[u64]) -> Option<Command> {
    let command = match action {
        LAYER_LOCK => Command::with_params("node.lock_state", json!({ "nodes": node_list(nodes), "state": "locked" })),
        LAYER_UNLOCK => Command::with_params("node.lock_state", json!({ "nodes": node_list(nodes), "state": "unlocked" })),
        LAYER_UNLOCK_ALL => Command::with_params("node.unlock_all", json!({})),
        LAYER_HIDE => Command::with_params("node.update", json!({ "nodes": node_list(nodes), "set": { "visible": false } })),
        LAYER_SHOW => Command::with_params("node.update", json!({ "nodes": node_list(nodes), "set": { "visible": true } })),
        LAYER_SHOW_ALL => Command::with_params("node.show_all", json!({})),
        EDIT_DELETE => LockedDeleteDialog::delete_command(nodes, false),
        _ => return None,
    };
    Some(command)
}

pub fn for_bridge(action: &str) -> Option<&'static str> {
    match action {
        "lock" => Some(LAYER_LOCK),
        "unlock" => Some(LAYER_UNLOCK),
        "unlock_all" => Some(LAYER_UNLOCK_ALL),
        "hide" => Some(LAYER_HIDE),
        "show" => Some(LAYER_SHOW),
        "show_all" => Some(LAYER_SHOW_ALL),
        _ => None,
    }
}

pub fn needs_selection(action: &str) -> bool {
    matches!(action, LAYER_LOCK | LAYER_UNLOCK | LAYER_HIDE | LAYER_SHOW | EDIT_DELETE)
}

pub fn is_enabled(session: &Session, action: &str) -> bool {
    let objects = session.document.objects();
    match action {
        LAYER_LOCK => session.selection.ids.iter().any(|id| objects.iter().any(|obj| obj.id == *id && !obj.locked)),
        LAYER_UNLOCK => session.selection.ids.iter().any(|id| objects.iter().any(|obj| obj.id == *id && obj.locked)),
        LAYER_UNLOCK_ALL => objects.iter().any(|obj| obj.locked),
        LAYER_HIDE => !session.selection.ids.is_empty(),
        LAYER_SHOW => session.selection.ids.iter().any(|id| objects.iter().any(|obj| obj.id == *id && !obj.pickable)),
        LAYER_SHOW_ALL => objects.iter().any(|obj| !obj.pickable),
        EDIT_DELETE => !session.selection.ids.is_empty(),
        _ => false,
    }
}

pub fn invoke(session: &mut Session, dialog: &mut LockedDeleteDialog, action: &str) -> CommandResult {
    let nodes = selected_nodes(session);
    if needs_selection(action) && nodes.is_empty() {
        return Err(CommandError::invalid_params());
    }
    if action == EDIT_DELETE {
        return dialog.request_delete(session, nodes);
    }
    let command = command_for(action, &nodes).ok_or_else(CommandError::not_implemented)?;
    session.run(&command)
}

pub fn menu_rows() -> Vec<(&'static str, String)> {
    LAYER_LOCK_ITEMS
        .iter()
        .chain(LAYER_VISIBILITY_ITEMS.iter())
        .map(|(action, key)| (*action, text(key).to_string()))
        .collect()
}

fn render_items(
    ui: &mut egui::Ui,
    session: &mut Session,
    dialog: &mut LockedDeleteDialog,
    items: &[(&str, &str)],
) -> Option<CommandResult> {
    let mut outcome = None;
    for (action, key) in items {
        let enabled = is_enabled(session, action);
        let clicked = ui
            .add_enabled_ui(enabled, |ui| dtool_design::menu_item(ui, text(key)))
            .inner
            .clicked();
        if clicked {
            outcome = Some(invoke(session, dialog, action));
            ui.close();
        }
    }
    outcome
}

pub fn render_layer_lock_menu(
    ui: &mut egui::Ui,
    session: &mut Session,
    dialog: &mut LockedDeleteDialog,
) -> Option<CommandResult> {
    let locked = render_items(ui, session, dialog, LAYER_LOCK_ITEMS);
    ui.separator();
    let shown = render_items(ui, session, dialog, LAYER_VISIBILITY_ITEMS);
    let confirmed = dialog.show(ui, session);
    confirmed.or(shown).or(locked)
}
