use dtool_design::{panel_frame, panel_header, button};
use dtool_io::AfInspectReport;
use dtool_strings::t;

pub struct AfReportDialog {
    pub report: Option<AfInspectReport>,
}

impl AfReportDialog {
    pub fn new() -> Self {
        Self { report: None }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if let Some(report) = &self.report {
            panel_frame(ctx, |ui| {
                panel_header(ui, t("af.report.title"));

                ui.label(format!("Version: {}", report.version));

                if !report.supported.is_empty() {
                    ui.label(t("af.report.supported"));
                    for item in &report.supported {
                        ui.label(item);
                    }
                }

                if !report.unsupported.is_empty() {
                    ui.label(t("af.report.unsupported"));
                    for item in &report.unsupported {
                        ui.label(item);
                    }
                }

                if !report.unknown.is_empty() {
                    ui.label(t("af.report.unknown"));
                    for item in &report.unknown {
                        ui.label(item);
                    }
                }

                if report.verdict == "NO_GO" {
                    ui.label(t("af.report.nogo"));
                } else {
                    button(ui, t("af.report.open"), || {});
                }
            });
        }
    }
}
