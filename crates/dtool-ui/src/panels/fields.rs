use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_design::{button, panel_frame, panel_header};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;

#[derive(Default)]
pub struct FieldsPanel {}

impl FieldsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        panel_frame(ui, |ui| {
            panel_header(ui, text("menu.text.insert.fields"));
            for (kind, key) in [
                ("PageNumber", "field.page_number"),
                ("NextFramePage", "field.next_frame_page"),
                ("PrevFramePage", "field.prev_frame_page"),
                ("TotalPages", "field.total_pages"),
                ("LastPageInSection", "field.last_page_in_section"),
                ("SectionName", "field.section_name"),
                ("ArtboardName", "field.artboard_name"),
                ("RunningHeader", "field.running_header"),
                ("DateTime", "field.date_time"),
                ("Created", "field.created"),
                ("Saved", "field.saved"),
                ("Printed", "field.printed"),
                ("Filename", "field.filename"),
                ("Path", "field.path"),
            ] {
                if button(ui, text(key)).clicked() {
                    let command = Command::with_params(
                        "text.field.insert",
                        json!({ "kind": kind }),
                    );
                    let _ = workspace.run(Scope::active(), command);
                }
            }
            ui.label(text("field.meta"));
            for name in dtool_core::longdoc::fields::META_NAMES {
                if button(ui, name).clicked() {
                    let command = Command::with_params(
                        "text.field.insert",
                        json!({ "kind": { "Meta": name } }),
                    );
                    let _ = workspace.run(Scope::active(), command);
                }
            }
        });
    }
}
