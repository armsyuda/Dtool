use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_core::Document;
use dtool_design::{button, panel_frame, panel_header};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;
use std::cell::RefCell;

pub struct AnchorsPanelState {
    pub open: bool,
    pub selected_anchor: Option<String>,
    pub rename_to: String,
}

impl Default for AnchorsPanelState {
    fn default() -> Self {
        Self {
            open: true,
            selected_anchor: None,
            rename_to: String::new(),
        }
    }
}

pub fn show_anchors_panel(
    ui: &mut egui::Ui,
    doc: &Document,
    state: &mut AnchorsPanelState,
    on_go_to: &dyn Fn(usize),
    on_rename: &dyn Fn(String, String),
    on_delete: &dyn Fn(String),
) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("panel.anchors.title"));
        let anchors_by_page = dtool_core::longdoc::anchors::list_anchors_by_page(doc);
        for (page_idx, anchors) in &anchors_by_page {
            ui.label(format!("{} {}", text("hyperlink.page"), dtool_core::longdoc::sections::section_page_label(doc, *page_idx)));
            for anchor in anchors {
                let is_selected = state.selected_anchor.as_ref() == Some(&anchor.name);
                ui.horizontal(|ui| {
                    if ui.selectable_label(is_selected, &anchor.name).clicked() {
                        state.selected_anchor = Some(anchor.name.clone());
                        state.rename_to = anchor.name.clone();
                        on_go_to(anchor.story_pos);
                    }
                    if is_selected {
                        ui.add(egui::TextEdit::singleline(&mut state.rename_to).desired_width(120.0));
                        if button(ui, text("button.rename")).clicked() && !state.rename_to.trim().is_empty() {
                            on_rename(anchor.name.clone(), state.rename_to.trim().to_string());
                        }
                        if button(ui, text("button.delete")).clicked() {
                            on_delete(anchor.name.clone());
                            state.selected_anchor = None;
                        }
                    }
                });
            }
        }
    });
}

#[derive(Default)]
pub struct AnchorsPanel {
    pub state: AnchorsPanelState,
}

impl AnchorsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        let pending: RefCell<Vec<Command>> = RefCell::new(Vec::new());
        {
            let Some(doc) = workspace.session().and_then(|s| s.document()) else { return; };
            show_anchors_panel(
                ui,
                doc,
                &mut self.state,
                &|_pos| {},
                &|name, new_name| pending.borrow_mut().push(Command::with_params("anchor.rename", json!({ "name": name, "new_name": new_name }))),
                &|name| pending.borrow_mut().push(Command::with_params("anchor.delete", json!({ "name": name }))),
            );
        }
        for command in pending.into_inner() {
            let _ = workspace.run(Scope::active(), command);
        }
    }
}
