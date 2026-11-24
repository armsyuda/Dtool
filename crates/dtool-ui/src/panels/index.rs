use dtool_core::longdoc::index::{build_index, topic_paths};
use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_design::{button, panel_frame, panel_header, toggle};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;

pub enum IndexRequest {
    InsertMark,
    AddTopic,
}

#[derive(Default)]
pub struct IndexPanel;

impl IndexPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) -> Option<IndexRequest> {
        let mut pending: Vec<Command> = Vec::new();
        let mut request = None;
        {
            let session = workspace.session()?;
            let has_caret = session.text_edit.is_some();
            let doc = session.document()?;
            panel_frame(ui, |ui| {
                panel_header(ui, text("panel.index.title"));
                ui.horizontal_wrapped(|ui| {
                    if dtool_design::button_with(ui, text("index.insert_mark"), &dtool_design::ControlState::enabled(has_caret)).clicked() {
                        request = Some(IndexRequest::InsertMark);
                    }
                    if button(ui, text("index.add_topic")).clicked() {
                        request = Some(IndexRequest::AddTopic);
                    }
                    let any = !doc.index.topics.is_empty() || !doc.index.marks.is_empty();
                    if dtool_design::button_with(ui, text("index.insert"), &dtool_design::ControlState::enabled(any)).clicked() {
                        pending.push(Command::with_params("index.insert", json!({})));
                    }
                    if dtool_design::button_with(ui, text("index.update"), &dtool_design::ControlState::enabled(!doc.index.indexes.is_empty())).clicked() {
                        pending.push(Command::with_params("index.update", json!({})));
                    }
                });
                let mut show_marks = doc.index.show_marks;
                if toggle(ui, &mut show_marks, text("menu.text.index.show_index_marks")).changed() {
                    pending.push(Command::with_params("index.marks.toggle", json!({})));
                }
                ui.separator();
                for path in topic_paths(&doc.index.topics) {
                    ui.horizontal(|ui| {
                        ui.label(path.as_str());
                        if button(ui, text("button.delete")).clicked() {
                            pending.push(Command::with_params("index.topic.remove", json!({ "path": path })));
                        }
                    });
                }
                ui.separator();
                for line in build_index(doc) {
                    let indent = "  ".repeat(line.level.saturating_sub(1) as usize);
                    if line.header {
                        ui.strong(line.text);
                    } else {
                        ui.label(format!("{}{}", indent, line.text));
                    }
                }
            });
        }
        for command in pending {
            let _ = workspace.run(Scope::active(), command);
        }
        request
    }
}
