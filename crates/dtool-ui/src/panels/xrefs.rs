use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_design::{panel_frame, panel_header};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;

const MISSING: &str = "[Missing]";

#[derive(Default)]
pub struct XrefsPanel;

impl XrefsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        let mut update = false;
        {
            let Some(doc) = workspace.session().and_then(|s| s.document()) else { return };
            panel_frame(ui, |ui| {
                panel_header(ui, text("panel.xrefs.title"));
                let xrefs = doc.cross_refs();
                if xrefs.is_empty() {
                    ui.label(text("xref.none_found"));
                }
                for xref in xrefs {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}: {}", xref.anchor, xref.cached));
                        if xref.cached == MISSING {
                            ui.colored_label(egui::Color32::RED, text("xref.missing"));
                        }
                    });
                }
                if dtool_design::button_with(ui, text("xref.update_all"), &dtool_design::ControlState::enabled(!xrefs.is_empty())).clicked() {
                    update = true;
                }
            });
        }
        if update {
            let _ = workspace.run(Scope::active(), Command::with_params("xref.update_all", json!({})));
        }
    }
}
