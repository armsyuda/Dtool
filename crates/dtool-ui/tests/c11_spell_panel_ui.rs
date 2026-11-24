use dtool_ui::spelling_panel::{error_key, show_spelling_panel, SpellPanelAction, SpellPanelView};

fn render(view: &SpellPanelView, choice: &mut String) -> Option<SpellPanelAction> {
    let ctx = egui::Context::default();
    let mut out = None;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        out = show_spelling_panel(ui, view, choice);
    });
    output.textures_delta.clear();
    out
}

#[test]
fn renders_rows_suggestions_and_error_without_action() {
    let rows = vec!["recieved".to_string()];
    let suggestions = vec!["received".to_string(), "relieved".to_string()];
    let mut choice = "received".to_string();
    let view = SpellPanelView { rows: &rows, selected: Some(0), suggestions: &suggestions, error: None };
    assert_eq!(render(&view, &mut choice), None);
    let view = SpellPanelView { rows: &[], selected: None, suggestions: &[], error: Some("SPELL_DICTIONARY_UNAVAILABLE") };
    assert_eq!(render(&view, &mut choice), None);
    assert_eq!(choice, "received");
}

#[test]
fn error_keys_are_stable() {
    assert_eq!(error_key("SPELL_STALE"), "panel.spelling.error.spell_stale");
    assert_eq!(error_key("SPELL_DICTIONARY_UNAVAILABLE"), "panel.spelling.error.spell_dictionary_unavailable");
}
