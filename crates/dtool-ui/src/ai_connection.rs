use dtool_core::ai_connection::AiConnection;
use dtool_core::prefs::Prefs;

pub fn sync(prefs: &Prefs, connection: &AiConnection) -> bool {
    if connection.enabled() != prefs.mcp_enabled {
        connection.set_from_gui(prefs.mcp_enabled);
        true
    } else {
        false
    }
}

pub fn show(ui: &mut egui::Ui, prefs: &mut Prefs, connection: &AiConnection) -> std::io::Result<bool> {
    let mut enabled = prefs.mcp_enabled;
    dtool_design::toggle(ui, &mut enabled, dtool_strings::text("pref.mcp.enable"));
    let changed = enabled != prefs.mcp_enabled;
    if changed {
        let mut next = prefs.clone();
        next.mcp_enabled = enabled;
        next.save(&Prefs::default_path())?;
        *prefs = next;
    }
    sync(prefs, connection);
    Ok(changed)
}
