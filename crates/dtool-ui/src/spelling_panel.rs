use dtool_strings::text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellPanelAction {
    Select(usize),
    Replace(String),
    Ignore,
    AddWord,
    Recheck,
}

pub struct SpellPanelView<'a> {
    pub rows: &'a [String],
    pub selected: Option<usize>,
    pub suggestions: &'a [String],
    pub error: Option<&'a str>,
}

pub fn error_key(code: &str) -> String {
    format!("panel.spelling.error.{}", code.to_ascii_lowercase())
}

pub fn show_spelling_panel(ui: &mut egui::Ui, view: &SpellPanelView, choice: &mut String) -> Option<SpellPanelAction> {
    let mut action = None;
    if let Some(code) = view.error {
        ui.colored_label(ui.visuals().error_fg_color, text(&error_key(code)));
    }
    let ready = !matches!(view.error, Some("SPELL_DICTIONARY_UNAVAILABLE"));
    if view.rows.is_empty() && view.error.is_none() {
        ui.label(text("panel.spelling.no_issues"));
    }
    egui::ScrollArea::vertical().id_salt("spelling_issues").max_height(160.0).show(ui, |ui| {
        for (i, word) in view.rows.iter().enumerate() {
            if ui.selectable_label(view.selected == Some(i), word.as_str()).clicked() && view.selected != Some(i) {
                action = Some(SpellPanelAction::Select(i));
            }
        }
    });
    ui.separator();
    ui.label(text("panel.spelling.suggestions"));
    for suggestion in view.suggestions {
        let picked = choice == suggestion;
        let response = ui.selectable_label(picked, suggestion.as_str());
        if response.clicked() {
            *choice = suggestion.clone();
        }
        if response.double_clicked() {
            action = Some(SpellPanelAction::Replace(suggestion.clone()));
        }
    }
    ui.text_edit_singleline(choice);
    ui.horizontal(|ui| {
        let has_issue = ready && view.selected.is_some();
        if ui.add_enabled(has_issue && !choice.is_empty(), egui::Button::new(text("panel.spelling.replace"))).clicked() {
            action = Some(SpellPanelAction::Replace(choice.clone()));
        }
        if ui.add_enabled(has_issue, egui::Button::new(text("panel.spelling.ignore"))).clicked() {
            action = Some(SpellPanelAction::Ignore);
        }
        if ui.add_enabled(has_issue, egui::Button::new(text("panel.spelling.add_word"))).clicked() {
            action = Some(SpellPanelAction::AddWord);
        }
        if ui.add_enabled(ready, egui::Button::new(text("panel.spelling.recheck"))).clicked() {
            action = Some(SpellPanelAction::Recheck);
        }
    });
    action
}
