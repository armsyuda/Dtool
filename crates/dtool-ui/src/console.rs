use dtool_core::{Scope, Workspace as CoreWorkspace};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::Value;

pub struct ConsolePanel {
    pub command_name: String,
    pub params_json: String,
    pub result_json: Option<String>,
    pub error_message: Option<String>,
}

impl Default for ConsolePanel {
    fn default() -> Self {
        Self {
            command_name: "app.about".to_string(),
            params_json: "{}".to_string(),
            result_json: None,
            error_message: None,
        }
    }
}

impl ConsolePanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        let mut run_requested = false;

        ui.horizontal(|ui| {
            ui.label(text("panel.console.title"));
        });

        ui.add_space(dtool_design::space("sm"));

        ui.horizontal(|ui| {
            ui.label(text("panel.console.command_name"));
            ui.text_edit_singleline(&mut self.command_name);
        });

        ui.add_space(dtool_design::space("sm"));

        ui.horizontal(|ui| {
            ui.label(text("panel.console.params"));
            dtool_design::text_input_multiline(ui, &mut self.params_json);
        });

        ui.add_space(dtool_design::space("md"));

        if dtool_design::button(ui, text("panel.console.run")).clicked() {
            run_requested = true;
        }

        ui.add_space(dtool_design::space("md"));

        if run_requested {
            self.execute(workspace);
        }

        if let Some(ref result) = self.result_json {
            ui.label(text("panel.console.result"));
            ui.monospace(result);
        }

        if let Some(ref error) = self.error_message {
            ui.label(text("panel.console.error"));
            ui.monospace(error);
        }
    }

    fn execute(&mut self, workspace: &mut CoreWorkspace) {
        let params: Value = match serde_json::from_str(&self.params_json) {
            Ok(v) => v,
            Err(e) => {
                self.result_json = None;
                self.error_message = Some(format!("{}: {}", text("panel.console.invalid_params"), e));
                return;
            }
        };

        let command = Command {
            name: self.command_name.clone(),
            params,
        };

        let (_, result) = workspace.run(Scope::active(), command);
        match result {
            Ok(outcome) => {
                self.error_message = None;
                match serde_json::to_string_pretty(&outcome) {
                    Ok(json) => self.result_json = Some(json),
                    Err(e) => self.error_message = Some(format!("{}: {}", text("error.internal_error"), e)),
                }
            }
            Err(err) => {
                self.result_json = None;
                let localized = dtool_strings::text(&err.message_key);
                self.error_message = Some(format!("{}: {}", err.code, localized));
                if let Ok(json) = serde_json::to_string_pretty(&err) {
                    self.result_json = Some(json);
                }
            }
        }
    }
}
