use dtool_design::{button, panel_frame, panel_header, dropdown, toggle, number_field, colour_well};
use dtool_io::{CadImportOptions, CadSelection};
use dtool_strings::t;

pub struct CadImportDialog {
    pub options: CadImportOptions,
}

impl CadImportDialog {
    pub fn new() -> Self {
        Self {
            options: CadImportOptions::default(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<CadImportOptions> {
        let mut confirmed = false;

        panel_frame(ui, |ui| {
            panel_header(ui, t("import.cad.title"));

            ui.horizontal(|ui| {
                ui.label(t("import.cad.selection"));
                let selection_text = match &self.options.selection {
                    CadSelection::AllPages => t("import.cad.selection.all_pages"),
                    CadSelection::SinglePage(_) => t("import.cad.selection.single_page"),
                    CadSelection::Model => t("import.cad.selection.model"),
                };
                dropdown(ui, selection_text);
            });

            ui.horizontal(|ui| {
                ui.label(t("import.cad.units"));
                number_field(ui, &mut self.options.units);
            });

            ui.horizontal(|ui| {
                ui.label(t("import.cad.dpi"));
                number_field(ui, &mut self.options.dpi);
            });

            ui.horizontal(|ui| {
                ui.label(t("import.cad.background"));
                colour_well(ui, &mut self.options.background);
            });

            ui.horizontal(|ui| {
                ui.label(t("import.cad.colour_override"));
                colour_well(ui, &mut self.options.colour_override);
            });

            toggle(ui, t("import.cad.remove_hidden"), &mut self.options.remove_hidden);
            toggle(ui, t("import.cad.entity_handles"), &mut self.options.entity_handles);
            toggle(ui, t("import.cad.override_line_weights"), &mut self.options.override_line_weights);
            toggle(ui, t("import.cad.sanitize"), &mut self.options.sanitize);

            ui.horizontal(|ui| {
                if button(ui, t("common.cancel")) {
                    return None;
                }
                if button(ui, t("common.import")) {
                    confirmed = true;
                }
            });
        });

        if confirmed {
            Some(self.options.clone())
        } else {
            None
        }
    }
}
