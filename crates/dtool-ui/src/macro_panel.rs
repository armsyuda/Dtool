use dtool_core::Session;
use dtool_strings::text;
use egui::Ui;

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleMode {
    NoScale,
    Stretch,
    MinFit,
    MaxFit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Alignment {
    Centre,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone)]
pub struct MacroStepUI {
    pub command: String,
    pub params: serde_json::Value,
    pub enabled: bool,
    pub show_name: bool,
    pub scale_mode: ScaleMode,
    pub alignment: Alignment,
}

#[derive(Default)]
pub struct MacroPanel {
    pub open: bool,
    pub recording: bool,
    pub steps: Vec<MacroStepUI>,
    pub selected_step: Option<usize>,
    pub library_name: String,
    pub library_category: String,
    pub export_path: String,
    pub import_path: String,
}

impl MacroPanel {
    fn bus(session: &mut Session, name: &str, params: serde_json::Value) -> Result<serde_json::Value, dtool_core::CommandError> {
        let command = dtool_core::Command::with_params(name, params);
        dtool_export::macros::run(session, &command).unwrap_or_else(|| Err(dtool_core::CommandError::not_implemented()))
    }

    fn load_steps(&mut self, reply: &serde_json::Value) {
        let steps: Vec<dtool_export::macros::MacroStep> = reply.get("steps").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        self.steps = steps
            .into_iter()
            .map(|s| MacroStepUI { command: s.command, params: s.params, enabled: s.enabled, show_name: s.show_name, scale_mode: ScaleMode::NoScale, alignment: Alignment::Centre })
            .collect();
        self.selected_step = None;
    }

    fn step_values(&self) -> Vec<dtool_export::macros::MacroStep> {
        self.steps.iter().map(|s| dtool_export::macros::MacroStep { command: s.command.clone(), params: s.params.clone(), enabled: s.enabled, show_name: s.show_name }).collect()
    }

    pub fn start_recording(&mut self, session: &mut Session) -> Result<(), dtool_core::CommandError> {
        let reply = Self::bus(session, dtool_export::macros::MACRO_RECORD_START, serde_json::json!({}))?;
        self.recording = true;
        self.load_steps(&reply);
        Ok(())
    }

    pub fn stop_recording(&mut self, session: &mut Session) -> Result<usize, dtool_core::CommandError> {
        let reply = Self::bus(session, dtool_export::macros::MACRO_RECORD_STOP, serde_json::json!({}))?;
        self.recording = false;
        self.load_steps(&reply);
        Ok(self.steps.len())
    }

    pub fn reset(&mut self, session: &mut Session) -> Result<(), dtool_core::CommandError> {
        Self::bus(session, dtool_export::macros::MACRO_RESET, serde_json::json!({}))?;
        self.recording = false;
        self.steps.clear();
        self.selected_step = None;
        Ok(())
    }

    pub fn play(&self, session: &mut Session) -> Result<usize, dtool_core::CommandError> {
        let steps = serde_json::to_value(self.step_values()).unwrap_or_default();
        let reply = Self::bus(session, dtool_export::macros::MACRO_PLAY, serde_json::json!({ "steps": steps }))?;
        Ok(reply.get("played").and_then(|v| v.as_u64()).unwrap_or(0) as usize)
    }

    pub fn add_to_library(&self, session: &mut Session) -> Result<serde_json::Value, dtool_core::CommandError> {
        let steps = serde_json::to_value(self.step_values()).unwrap_or_default();
        Self::bus(session, dtool_export::macros::MACRO_LIBRARY_ADD, serde_json::json!({ "name": self.library_name, "category": self.library_category, "steps": steps }))
    }

    pub fn show(&mut self, ui: &mut Ui, session: &mut Session) {
        if !self.open {
            return;
        }

        dtool_design::panel_frame(ui, |ui| {
            dtool_design::panel_header(ui, text("macro.panel.title"));

            ui.horizontal(|ui| {
                if dtool_design::button(ui, text("macro.record")).clicked() {
                    let _ = self.start_recording(session);
                }
                if dtool_design::button(ui, text("macro.stop")).clicked() {
                    let _ = self.stop_recording(session);
                }
                if dtool_design::button(ui, text("macro.play")).clicked() {
                    let _ = self.play(session);
                }
                if dtool_design::button(ui, text("macro.reset")).clicked() {
                    let _ = self.reset(session);
                }
            });

            ui.horizontal(|ui| {
                if dtool_design::button(ui, text("macro.add_library")).clicked() {
                    let _ = self.add_to_library(session);
                }
                ui.text_edit_singleline(&mut self.library_name);
                ui.text_edit_singleline(&mut self.library_category);
            });

            ui.horizontal(|ui| {
                if dtool_design::button(ui, text("macro.export")).clicked() {
                    let _ = Self::bus(session, dtool_export::macros::MACRO_EXPORT, serde_json::json!({ "name": self.library_name, "path": self.export_path }));
                }
                ui.text_edit_singleline(&mut self.export_path);
            });

            ui.horizontal(|ui| {
                if dtool_design::button(ui, text("macro.import")).clicked() {
                    let _ = Self::bus(session, dtool_export::macros::MACRO_IMPORT, serde_json::json!({ "path": self.import_path }));
                }
                ui.text_edit_singleline(&mut self.import_path);
            });

            ui.separator();

            if !self.steps.is_empty() {
                ui.label(text("macro.steps.list"));
                for (i, step) in self.steps.iter_mut().enumerate() {
                    let is_selected = self.selected_step == Some(i);
                    ui.horizontal(|ui| {
                        let mut enabled = step.enabled;
                        if dtool_design::toggle(ui, &mut enabled, "").changed() {
                            step.enabled = enabled;
                        }
                        
                        let label = if step.show_name {
                            format!("{}: {}", i + 1, step.command)
                        } else {
                            format!("{}", i + 1)
                        };
                        
                        if ui.selectable_label(is_selected, label).clicked() {
                            self.selected_step = Some(i);
                        }
                    });
                }

                if let Some(idx) = self.selected_step {
                    if idx < self.steps.len() {
                        ui.separator();
                        ui.label(text("macro.step.properties"));
                        
                        let step = &mut self.steps[idx];
                        
                        ui.horizontal(|ui| {
                            ui.label(text("macro.step.show_name"));
                            let mut show = step.show_name;
                            if dtool_design::toggle(ui, &mut show, "").changed() {
                                step.show_name = show;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label(text("macro.step.scale_mode"));
                            let options: Vec<(ScaleMode, String)> = [ScaleMode::NoScale, ScaleMode::Stretch, ScaleMode::MinFit, ScaleMode::MaxFit].into_iter().map(|mode| { let label = format!("{:?}", mode); (mode, label) }).collect();
                            let options: Vec<(ScaleMode, &str)> = options.iter().map(|(mode, label)| (mode.clone(), label.as_str())).collect();
                            dtool_design::dropdown_value(ui, "scale_mode", &mut step.scale_mode, &options, &dtool_design::ControlState::default());
                        });

                        ui.horizontal(|ui| {
                            ui.label(text("macro.step.alignment"));
                            let options: Vec<(Alignment, String)> = [Alignment::Centre, Alignment::TopLeft, Alignment::TopRight, Alignment::BottomLeft, Alignment::BottomRight].into_iter().map(|align| { let label = format!("{:?}", align); (align, label) }).collect();
                            let options: Vec<(Alignment, &str)> = options.iter().map(|(align, label)| (align.clone(), label.as_str())).collect();
                            dtool_design::dropdown_value(ui, "alignment", &mut step.alignment, &options, &dtool_design::ControlState::default());
                        });

                        ui.label(text("macro.step.params"));
                        let mut params_str = serde_json::to_string_pretty(&step.params).unwrap_or_default();
                        if dtool_design::text_input_multiline(ui, &mut params_str).changed() {
                            if let Ok(new_params) = serde_json::from_str::<serde_json::Value>(&params_str) {
                                step.params = new_params;
                            }
                        }
                    }
                }
            } else {
                ui.label(text("macro.steps.empty"));
            }
        });
    }
}
