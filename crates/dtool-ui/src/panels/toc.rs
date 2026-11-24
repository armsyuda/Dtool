use dtool_core::longdoc::toc::{build_toc, para_style_names, TocSpec, MAX_LEVEL};
use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_design::{button, dropdown, number_field, panel_frame, panel_header, toggle};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;

#[derive(Default)]
pub struct TocPanel {
    pub add_index: usize,
}

impl TocPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        let mut pending: Vec<Command> = Vec::new();
        {
            let Some(doc) = workspace.session().and_then(|s| s.document()) else { return };
            panel_frame(ui, |ui| {
                panel_header(ui, text("panel.toc.title"));
                let current = doc.toc.spec.clone();
                let mut spec: TocSpec = current.clone();
                let mut remove = None;
                for (i, (name, level)) in spec.styles.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(name.as_str());
                        let mut value = *level as f64;
                        if number_field(ui, &mut value, "").changed() {
                            *level = (value.round() as u8).clamp(1, MAX_LEVEL);
                        }
                        if button(ui, text("button.delete")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    spec.styles.remove(i);
                }
                let names: Vec<String> = para_style_names(doc).into_iter().filter(|n| !spec.styles.iter().any(|(s, _)| s == n)).collect();
                if !names.is_empty() {
                    ui.horizontal(|ui| {
                        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
                        if self.add_index >= labels.len() {
                            self.add_index = 0;
                        }
                        dropdown(ui, "toc_add_style", &mut self.add_index, &labels);
                        if button(ui, "+").clicked() {
                            let level = (spec.styles.len() as u8 + 1).min(MAX_LEVEL);
                            spec.styles.push((names[self.add_index].clone(), level));
                        }
                    });
                }
                toggle(ui, &mut spec.page_numbers, text("toc.page_numbers"));
                ui.horizontal(|ui| {
                    ui.label(text("toc.leader"));
                    ui.add(egui::TextEdit::singleline(&mut spec.leader).desired_width(60.0));
                });
                if spec != current {
                    pending.push(Command::with_params("toc.spec.set", json!({ "spec": spec })));
                }
                ui.horizontal_wrapped(|ui| {
                    if dtool_design::button_with(ui, text("toc.insert"), &dtool_design::ControlState::enabled(!current.styles.is_empty())).clicked() {
                        pending.push(Command::with_params("toc.insert", json!({})));
                    }
                    if dtool_design::button_with(ui, text("toc.update"), &dtool_design::ControlState::enabled(!doc.toc.tocs.is_empty())).clicked() {
                        pending.push(Command::with_params("toc.update", json!({})));
                    }
                    if dtool_design::button_with(ui, text("toc.update_all"), &dtool_design::ControlState::enabled(!doc.toc.tocs.is_empty())).clicked() {
                        pending.push(Command::with_params("toc.update_all", json!({})));
                    }
                });
                ui.separator();
                for entry in build_toc(doc, &current) {
                    ui.label(format!("{}{}  {}", "  ".repeat(entry.level.saturating_sub(1) as usize), entry.text, entry.page_label));
                }
            });
        }
        for command in pending {
            let _ = workspace.run(Scope::active(), command);
        }
    }
}
