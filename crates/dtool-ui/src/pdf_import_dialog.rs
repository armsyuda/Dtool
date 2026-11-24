use dtool_design::{color, space, icon};
use dtool_io::import_pdf::{ImportFormat, ImportOptions, PageSel, ImportAs, ImportColour};
use egui;

pub struct PdfImportDialog {
    pub path: std::path::PathBuf,
    pub options: ImportOptions,
    pub encrypted: bool,
    pub password_input: String,
    pub error_key: Option<String>,
}

impl PdfImportDialog {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            options: ImportOptions::default(),
            encrypted: false,
            password_input: String::new(),
            error_key: None,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<ImportOptions> {
        let mut result = None;
        let mut close_requested = false;

        egui::Window::new(dtool_strings::get("import.pdf.title"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                if self.encrypted {
                    ui.label(dtool_strings::get("import.pdf.password"));
                    let response = ui.add(egui::TextEdit::singleline(&mut self.password_input).password(true));
                    
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.options.password = Some(self.password_input.clone());
                    }

                    if !self.password_input.is_empty() {
                        self.options.password = Some(self.password_input.clone());
                    } else {
                        self.options.password = None;
                    }
                }

                ui.add_space(space("md"));

                let ok_enabled = !self.encrypted || !self.password_input.is_empty();

                if ui.add_enabled(ok_enabled, dtool_design::button(dtool_strings::get("action.ok"))).clicked() {
                    result = Some(self.options.clone());
                    close_requested = true;
                }

                if let Some(key) = &self.error_key {
                    ui.label(dtool_strings::get(key));
                }
            });

        if close_requested {
            return result;
        }

        None
    }
}
