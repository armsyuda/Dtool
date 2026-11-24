use dtool_core::prefs::Prefs;
use dtool_design::{button, dropdown};
use dtool_strings::text;
use egui::Ui;

pub const FILLER_LANGUAGES: [(&str, &str); 2] = [("en_gb", "pref.lang.en_gb"), ("en_us", "pref.lang.en_us")];

pub fn language_index(prefs: &Prefs) -> usize {
    FILLER_LANGUAGES.iter().position(|(code, _)| *code == prefs.filler_language).unwrap_or(0)
}

pub fn save_default(prefs: &mut Prefs) {
    prefs.filler_default = prefs.filler_text.clone();
}

pub fn revert(prefs: &mut Prefs) {
    prefs.filler_text = prefs.filler_default.clone();
}

pub fn factory_reset(prefs: &mut Prefs) {
    prefs.filler_text = Prefs::factory_filler();
    prefs.filler_default = Prefs::factory_filler();
}

pub fn show(ui: &mut Ui, prefs: &mut Prefs) {
    ui.label(text("settings.filler"));
    ui.horizontal(|ui| {
        ui.label(text("settings.filler.language"));
        let labels: Vec<&str> = FILLER_LANGUAGES.iter().map(|(_, key)| text(key)).collect();
        let mut index = language_index(prefs);
        dropdown(ui, "settings.filler.language", &mut index, &labels);
        if let Some((code, _)) = FILLER_LANGUAGES.get(index) {
            prefs.filler_language = code.to_string();
        }
    });
    let response = ui.add(egui::TextEdit::multiline(&mut prefs.filler_text).hint_text(text("pref.filler.hint")));
    dtool_design::mark_multiline(ui, &response);
    ui.horizontal(|ui| {
        if button(ui, text("settings.filler.save_default")).clicked() {
            save_default(prefs);
        }
        if button(ui, text("settings.filler.revert")).clicked() {
            revert(prefs);
        }
        if button(ui, text("settings.filler.factory")).clicked() {
            factory_reset(prefs);
        }
    });
}
