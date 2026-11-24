use dtool_design::{button, panel_frame, panel_header, dropdown, number_field, text_field};
use dtool_core::caption::CaptionSettings;
use dtool_protocol::Command;

pub struct CaptionDialog {
    pub settings: CaptionSettings,
}

impl CaptionDialog {
    pub fn new() -> Self {
        Self {
            settings: CaptionSettings::default(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<Command> {
        panel_frame(ui, |ui| {
            panel_header(ui, dtool_strings::get("menu.caption.add"));

            let counter_types = vec![
                (dtool_strings::get("caption.token.figure"), "Figure"),
                ("Table", "Table"),
            ];
            
            let current_counter_idx = match self.settings.counter_type {
                dtool_core::caption::CounterType::Figure => 0,
                dtool_core::caption::CounterType::Table => 1,
                _ => 0,
            };

            ui.horizontal(|ui| {
                ui.label(dtool_strings::get("caption.token.counter"));
                let selected = dropdown(ui, &counter_types, current_counter_idx);
                if selected != current_counter_idx {
                    self.settings.counter_type = match selected {
                        0 => dtool_core::caption::CounterType::Figure,
                        1 => dtool_core::caption::CounterType::Table,
                        _ => dtool_core::caption::CounterType::Figure,
                    };
                }
            });

            dtool_design::toggle(ui, &mut self.settings.restart_numbering, dtool_strings::get("caption.restart_numbering"));

            ui.horizontal(|ui| {
                ui.label("Format");
                text_field(ui, &mut self.settings.format);
            });

            let alignments = vec![
                ("Below", dtool_core::caption::CaptionSide::Below),
                ("Above", dtool_core::caption::CaptionSide::Above),
                ("Left", dtool_core::caption::CaptionSide::Left),
                ("Right", dtool_core::caption::CaptionSide::Right),
            ];
            
            let current_align_idx = match self.settings.alignment {
                dtool_core::caption::CaptionSide::Below => 0,
                dtool_core::caption::CaptionSide::Above => 1,
                dtool_core::caption::CaptionSide::Left => 2,
                dtool_core::caption::CaptionSide::Right => 3,
            };

            ui.horizontal(|ui| {
                ui.label("Alignment");
                let selected = dropdown(ui, &alignments.iter().map(|(l, _)| *l).collect::<Vec<_>>(), current_align_idx);
                if selected != current_align_idx {
                    self.settings.alignment = alignments[selected].1.clone();
                }
            });

            ui.horizontal(|ui| {
                ui.label("H Offset");
                number_field(ui, &mut self.settings.h_offset);
            });

            ui.horizontal(|ui| {
                ui.label("V Offset");
                number_field(ui, &mut self.settings.v_offset);
            });

            let preview_text = self.settings.format.replace("{counter}", "1");
            ui.label(format!("Preview: {}", preview_text));

            if button(ui, dtool_strings::get("menu.caption.add")) {
                return Some(Command {
                    name: "caption.add".to_string(),
                    args: serde_json::json!({
                        "settings": self.settings
                    }),
                });
            }
        });
        
        None
    }
}

use egui;
