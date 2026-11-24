use dtool_core::command::Session;
use dtool_core::commands::frame_text::{CONVERT_TO_ART, CONVERT_TO_FRAME};
use dtool_protocol::{CommandError, CommandResult};
use dtool_strings::text;
use serde_json::json;

pub const LAYER_CONVERT_ITEMS: &[(&str, &str)] = &[
    (CONVERT_TO_FRAME, "menu.layer.convert_frame_text"),
    (CONVERT_TO_ART, "menu.layer.convert_art_text"),
];

pub fn is_convert_action(action: &str) -> bool {
    LAYER_CONVERT_ITEMS.iter().any(|(id, _)| *id == action)
}

pub fn is_enabled(session: &Session, action: &str) -> bool {
    is_convert_action(action) && session.can_execute_named(action)
}

pub fn invoke(session: &mut Session, action: &str) -> CommandResult {
    if !is_convert_action(action) {
        return Err(CommandError::not_implemented());
    }
    session.run_named(action, json!({}))
}

pub fn menu_rows() -> Vec<(&'static str, String)> {
    LAYER_CONVERT_ITEMS.iter().map(|(action, key)| (*action, text(key).to_string())).collect()
}

pub fn render_layer_convert_menu(ui: &mut egui::Ui, session: &mut Session) -> Option<CommandResult> {
    let mut outcome = None;
    for (action, key) in LAYER_CONVERT_ITEMS {
        let enabled = is_enabled(session, action);
        let clicked = ui.add_enabled_ui(enabled, |ui| dtool_design::menu_item(ui, text(key))).inner.clicked();
        if clicked {
            outcome = Some(invoke(session, action));
            ui.close();
        }
    }
    outcome
}
