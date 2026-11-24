use dtool_core::metadata::DocMetadata;
use dtool_design::{color, icon, space};
use egui::{self, RichText, Ui};

pub struct MetadataPanel {
    pub open: bool,
}

impl Default for MetadataPanel {
    fn default() -> Self {
        Self { open: true }
    }
}

impl MetadataPanel {
    pub fn show(&mut self, ctx: &egui::Context, meta: &DocMetadata) {
        if !self.open {
            return;
        }

        egui::SidePanel::right("metadata_panel")
            .frame(dtool_design::panel_frame())
            .show(ctx, |ui| {
                dtool_design::panel_header(ui, "panel.metadata.title");

                ui.add_space(space("md"));

                if meta.is_empty() {
                    ui.label(RichText::new("meta.empty").color(color("text.secondary")));
                    return;
                }

                show_section(ui, "meta.title", &meta.title);
                show_section(ui, "meta.author", &meta.author);
                show_section(ui, "meta.subject", &meta.subject);
                show_section(ui, "meta.copyright", &meta.copyright);
                show_section(ui, "meta.description", &meta.description);

                if !meta.keywords.is_empty() {
                    ui.label(RichText::new("meta.keywords").strong());
                    for kw in &meta.keywords {
                        ui.label(kw);
                    }
                    ui.add_space(space("sm"));
                }

                if !meta.xmp_extra.is_empty() {
                    ui.label(RichText::new("meta.xmp_extra").strong());
                    for (k, v) in &meta.xmp_extra {
                        ui.label(format!("{}: {}", k, v));
                    }
                }
            });
    }
}

fn show_section(ui: &mut Ui, key: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    ui.label(RichText::new(key).strong());
    ui.label(value);
    ui.add_space(space("sm"));
}
