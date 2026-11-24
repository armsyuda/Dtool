use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_design::{panel_frame, panel_header};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::{json, Value};

pub const LAYER_STATES_PANEL_KEY: &str = "layer_states";

#[derive(Default)]
pub struct LayerStatesPanel {
    pub name: String,
    pub rename: Option<(String, String)>,
    pub last_error: Option<String>,
}

pub fn state_names(value: &Value) -> Vec<String> {
    value
        .get("states")
        .and_then(Value::as_array)
        .map(|states| states.iter().filter_map(|state| state.get("name").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default()
}

impl LayerStatesPanel {
    pub fn run(&mut self, workspace: &mut CoreWorkspace, name: &str, params: Value) -> Option<Value> {
        let (_, result) = workspace.run(Scope::active(), Command::with_params(name, params));
        match result {
            Ok(value) => {
                self.last_error = None;
                Some(value)
            }
            Err(error) => {
                self.last_error = Some(error.message_key);
                None
            }
        }
    }

    pub fn names(&mut self, workspace: &mut CoreWorkspace) -> Vec<String> {
        if workspace.session().is_none() {
            return Vec::new();
        }
        let (_, result) = workspace.run(Scope::active(), Command::with_params("layer.state.list", json!({})));
        result.map(|value| state_names(&value)).unwrap_or_default()
    }

    pub fn save(&mut self, workspace: &mut CoreWorkspace) {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return;
        }
        if self.run(workspace, "layer.state.save", json!({"name": name})).is_some() {
            self.name.clear();
        }
    }

    pub fn restore(&mut self, workspace: &mut CoreWorkspace, name: &str) {
        let _ = self.run(workspace, "layer.state.restore", json!({"name": name}));
    }

    pub fn delete(&mut self, workspace: &mut CoreWorkspace, name: &str) {
        let _ = self.run(workspace, "layer.state.delete", json!({"name": name}));
    }

    pub fn rename_to(&mut self, workspace: &mut CoreWorkspace, name: &str, new_name: &str) {
        let new_name = new_name.trim();
        if new_name.is_empty() || new_name == name {
            return;
        }
        let _ = self.run(workspace, "layer.state.rename", json!({"name": name, "new_name": new_name}));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        panel_frame(ui, |ui| {
            panel_header(ui, text("panel.layer_states"));
            if workspace.session().is_none() {
                ui.label(text("layer_states.empty"));
                return;
            }
            let mut save = false;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.name).hint_text(text("layer_states.name")));
                save = dtool_design::button(ui, text("layer_states.save")).clicked();
            });
            if save {
                self.save(workspace);
            }
            let names = self.names(workspace);
            if names.is_empty() {
                ui.label(text("layer_states.empty"));
            }
            let mut action: Option<(&'static str, String)> = None;
            let mut commit_rename = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for name in &names {
                    ui.horizontal(|ui| {
                        let editing = matches!(&self.rename, Some((old, _)) if old == name);
                        if editing {
                            if let Some((old, value)) = self.rename.as_mut() {
                                let response = ui.text_edit_singleline(value);
                                if response.lost_focus() || dtool_design::button(ui, text("layer_states.rename")).clicked() {
                                    commit_rename = Some((old.clone(), value.clone()));
                                }
                            }
                        } else if ui.selectable_label(false, name).double_clicked() {
                            self.rename = Some((name.clone(), name.clone()));
                        }
                        if dtool_design::button(ui, text("layer_states.restore")).clicked() {
                            action = Some(("restore", name.clone()));
                        }
                        if dtool_design::button(ui, text("layer_states.delete")).clicked() {
                            action = Some(("delete", name.clone()));
                        }
                    });
                }
            });
            if let Some((old, value)) = commit_rename {
                self.rename = None;
                self.rename_to(workspace, &old, &value);
            }
            match action {
                Some(("restore", name)) => self.restore(workspace, &name),
                Some((_, name)) => self.delete(workspace, &name),
                None => {}
            }
            if let Some(error) = &self.last_error {
                ui.label(text(error));
            }
        });
    }
}
