use dtool_design::{button, panel_frame, panel_header, number_field};
use dtool_text::autocorrect::AutoCorrect;
use egui::Ui;

pub fn render_autocorrect_settings(ui: &mut Ui, config: &mut AutoCorrect) {
    panel_frame(ui, |ui| {
        panel_header(ui, "settings.autocorrect");

        dtool_design::toggle(ui, &mut config.smart_quotes, dtool_strings::text("pref.autocorrect.typographic_quotes"));
        dtool_design::toggle(ui, &mut config.cap_first, dtool_strings::text("pref.autocorrect.capitalise_sentences"));

        ui.add_space(8.0);

        for (i, pair) in config.pairs.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}: ", i + 1));
                ui.text_edit_singleline(&mut pair.0);
                ui.label("→");
                ui.text_edit_singleline(&mut pair.1);

                if button(ui, "Remove") {
                    config.pairs.remove(i);
                }
            });
        }

        if button(ui, "Add Pair") {
            config.pairs.push((String::new(), String::new()));
        }
    });
}
