use dtool_core::metadata::{Accessibility, DocMetadata};
use dtool_design::{color, space};
use egui::{self, RichText, Ui};

pub struct DocMetadataDialog {
    pub open: bool,
    pub draft: DocMetadata,
}

impl Default for DocMetadataDialog {
    fn default() -> Self {
        Self {
            open: false,
            draft: DocMetadata::default(),
        }
    }
}

impl DocMetadataDialog {
    pub fn open_with(&mut self, meta: &DocMetadata) {
        self.draft = meta.clone();
        self.open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<DocMetadata> {
        if !self.open {
            return None;
        }

        let mut result = None;

        egui::Window::new("dialog.doc_metadata.title")
            .frame(dtool_design::dialog_frame())
            .open(&mut self.open)
            .show(ctx, |ui| {
                ui.add_space(space("md"));

                text_field(ui, "meta.title", &mut self.draft.title);
                text_field(ui, "meta.author", &mut self.draft.author);
                text_field(ui, "meta.subject", &mut self.draft.subject);
                text_field(ui, "meta.copyright", &mut self.draft.copyright);

                ui.label(RichText::new("meta.keywords").strong());
                let kw_str = self.draft.keywords.join(", ");
                let mut kw_edit = kw_str.clone();
                if ui.text_edit_singleline(&mut kw_edit).changed() {
                    self.draft.keywords = kw_edit.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }

                text_field(ui, "meta.description", &mut self.draft.description);

                ui.add_space(space("md"));

                ui.label(RichText::new("meta.accessibility").strong());
                ui.add_space(space("sm"));

                let acc = &mut self.draft.accessibility;

                ui.label(RichText::new("meta.access_modes").strong());
                let am_str = acc.access_modes.join(", ");
                let mut am_edit = am_str.clone();
                if ui.text_edit_singleline(&mut am_edit).changed() {
                    acc.access_modes = am_edit.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
                ui.add_space(space("sm"));

                ui.label(RichText::new("meta.access_modes_sufficient").strong());
                let ams_str = acc.access_modes_sufficient.join(", ");
                let mut ams_edit = ams_str.clone();
                if ui.text_edit_singleline(&mut ams_edit).changed() {
                    acc.access_modes_sufficient = ams_edit.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
                ui.add_space(space("sm"));

                ui.label(RichText::new("meta.features").strong());
                let feat_str = acc.features.join(", ");
                let mut feat_edit = feat_str.clone();
                if ui.text_edit_singleline(&mut feat_edit).changed() {
                    acc.features = feat_edit.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
                ui.add_space(space("sm"));

                ui.label(RichText::new("meta.hazards").strong());
                let haz_str = acc.hazards.join(", ");
                let mut haz_edit = haz_str.clone();
                if ui.text_edit_singleline(&mut haz_edit).changed() {
                    acc.hazards = haz_edit.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
                ui.add_space(space("sm"));

                text_field(ui, "meta.summary", &mut acc.summary);

                ui.label(RichText::new("meta.conformance").strong());
                let conf_str = acc.conformance.clone().unwrap_or_default();
                let mut conf_edit = conf_str.clone();
                if ui.text_edit_singleline(&mut conf_edit).changed() {
                    acc.conformance = if conf_edit.is_empty() { None } else { Some(conf_edit) };
                }
                ui.add_space(space("sm"));

                text_field(ui, "meta.certified_by", &mut acc.certified_by);
                text_field(ui, "meta.certifier_credentials", &mut acc.certifier_credentials);
                text_field(ui, "meta.certifier_report", &mut acc.certifier_report);

                ui.add_space(space("md"));

                if dtool_design::button(ui, "dialog.ok").clicked() {
                    result = Some(self.draft.clone());
                    self.open = false;
                }
            });

        result
    }
}

fn text_field(ui: &mut Ui, label_key: &str, value: &mut String) {
    ui.label(RichText::new(label_key).strong());
    ui.text_edit_singleline(value);
    ui.add_space(space("sm"));
}
