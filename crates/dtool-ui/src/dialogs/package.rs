use dtool_core::Command;
use dtool_protocol::CommandResult;
use dtool_strings::text;
use serde_json::{json, Value};

pub const PACKAGE_SAVE: &str = "doc.package.save";

pub struct PackageDialogState {
    pub open: bool,
    pub folder: String,
    pub include_images: bool,
    pub include_fonts: bool,
    pub zip: bool,
    pub report: Option<Value>,
    pub error_key: Option<String>,
}

impl Default for PackageDialogState {
    fn default() -> Self {
        Self { open: false, folder: String::new(), include_images: true, include_fonts: false, zip: false, report: None, error_key: None }
    }
}

impl PackageDialogState {
    pub fn command(&self) -> Option<Command> {
        let folder = self.folder.trim();
        if folder.is_empty() {
            return None;
        }
        Some(Command::with_params(PACKAGE_SAVE, json!({"folder": folder, "images": self.include_images, "fonts": self.include_fonts, "zip": self.zip})))
    }

    pub fn apply_result(&mut self, result: CommandResult) {
        match result {
            Ok(report) => {
                self.report = Some(report);
                self.error_key = None;
            }
            Err(error) => {
                self.report = None;
                self.error_key = Some(error.message_key);
            }
        }
    }

    pub fn missing(&self) -> Vec<String> {
        self.report.as_ref().and_then(|r| r.get("missing")).and_then(Value::as_array).map(|items| items.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default()
    }

    pub fn fonts_missing(&self) -> Vec<String> {
        self.report.as_ref().and_then(|r| r.get("fonts_missing")).and_then(Value::as_array).map(|items| items.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default()
    }

    pub fn zip_path(&self) -> Option<String> {
        self.report.as_ref().and_then(|r| r.get("zip")).and_then(Value::as_str).map(String::from)
    }

    pub fn packaged_count(&self) -> usize {
        let count = |key: &str| self.report.as_ref().and_then(|r| r.get(key)).and_then(Value::as_array).map_or(0, Vec::len);
        count("copied") + count("kept")
    }
}

pub fn show_package_dialog(ui: &mut egui::Ui, state: &mut PackageDialogState) -> Option<Command> {
    if !state.open {
        return None;
    }
    let mut pending = None;
    dtool_design::panel_frame(ui, |ui| {
        dtool_design::panel_header(ui, text("dialog.package.title"));
        ui.label(text("dialog.file.path"));
        ui.text_edit_singleline(&mut state.folder);
        dtool_design::toggle(ui, &mut state.include_images, text("package.include_images"));
        if dtool_strings::has("package.include_fonts") {
            dtool_design::toggle(ui, &mut state.include_fonts, text("package.include_fonts"));
        }
        if dtool_strings::has("package.zip") {
            dtool_design::toggle(ui, &mut state.zip, text("package.zip"));
        }
        if state.report.is_some() {
            ui.label(format!("{}: {}", text("package.present"), state.packaged_count()));
            let missing = state.missing();
            if !missing.is_empty() {
                ui.label(format!("{}: {}", text("rm.missing"), missing.join(", ")));
            }
            let fonts_missing = state.fonts_missing();
            if !fonts_missing.is_empty() && dtool_strings::has("package.fonts_missing") {
                ui.label(format!("{}: {}", text("package.fonts_missing"), fonts_missing.join(", ")));
            }
            if let (Some(path), true) = (state.zip_path(), dtool_strings::has("package.zip_written")) {
                ui.label(format!("{}: {}", text("package.zip_written"), path));
            }
        }
        if let Some(key) = &state.error_key {
            ui.label(text(key));
        }
        ui.horizontal(|ui| {
            if dtool_design::button(ui, text("package.package")).clicked() {
                pending = state.command();
            }
            if dtool_design::button(ui, text("button.close")).clicked() {
                state.open = false;
                state.report = None;
                state.error_key = None;
            }
        });
    });
    pending
}
