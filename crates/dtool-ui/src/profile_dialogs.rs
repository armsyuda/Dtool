use dtool_core::Session;
use dtool_strings::text;

#[derive(Default)]
pub struct AssignIccDialog {
    pub open: bool,
    pub profiles: Vec<String>,
    pub selected: usize,
    pub error_key: Option<String>,
}
impl AssignIccDialog {
    pub fn open(&mut self, session: &Session) {
        self.error_key = None;
        match dtool_core::colour::services().read() {
            Ok(services) => self.profiles = services.profiles.list(session.document.colour.format.model).into_iter().map(str::to_string).collect(),
            Err(_) => { self.profiles.clear(); self.error_key = Some("error.internal".into()); }
        }
        self.selected = self.profiles.iter().position(|profile| *profile == session.document.colour.profile).unwrap_or(0);
        self.open = true;
    }
    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) {
        if !self.open { return; }
        crate::dialog_chrome::show_floating(ctx, "document.assign_icc", text("dialog.assign_icc.title"), |ui| {
            ui.label(text("dialog.profile.profile"));
            let labels: Vec<&str> = self.profiles.iter().map(String::as_str).collect();
            if !labels.is_empty() { dtool_design::dropdown(ui, "assign_icc.profile", &mut self.selected, &labels); }
            if let Some(key) = &self.error_key { ui.label(text(key)); }
            ui.horizontal(|ui| {
                if ui.add_enabled_ui(self.profiles.get(self.selected).is_some(), |ui| dtool_design::button(ui, text("button.ok"))).inner.clicked() {
                    if let Some(profile) = self.profiles.get(self.selected) {
                        match session.run_named("color.profile.assign", serde_json::json!({"profile": profile})) {
                            Ok(_) => self.open = false,
                            Err(error) => self.error_key = Some(error.message_key),
                        }
                    }
                }
                if dtool_design::button(ui, text("button.cancel")).clicked() { self.open = false; }
            });
        });
    }
}

#[derive(Default)]
pub struct ConvertIccDialog {
    pub open: bool,
    pub formats: Vec<String>,
    pub selected_format: usize,
    pub profiles: Vec<String>,
    pub selected_profile: usize,
    pub intents: Vec<String>,
    pub selected_intent: usize,
    pub bpc: bool,
    pub error_key: Option<String>,
}

impl ConvertIccDialog {
    pub fn open(&mut self, session: &Session) {
        self.error_key = None;
        self.formats = vec![
            "RGBA/8".to_string(),
            "CMYK/8".to_string(),
            "LAB/16".to_string(),
            "Grey/8".to_string(),
        ];
        self.selected_format = 0;

        match dtool_core::colour::services().read() {
            Ok(services) => {
                let model = match self.formats[self.selected_format].as_str() {
                    "RGBA/8" => dtool_core::colour::ColourModel::Rgb,
                    "CMYK/8" => dtool_core::colour::ColourModel::Cmyk,
                    "LAB/16" => dtool_core::colour::ColourModel::Lab,
                    _ => dtool_core::colour::ColourModel::Grey,
                };
                self.profiles = services.profiles.list(model).into_iter().map(str::to_string).collect();
            },
            Err(_) => { self.profiles.clear(); self.error_key = Some("error.internal".into()); }
        }

        self.selected_profile = 0;
        self.intents = vec![
            "Perceptual".to_string(),
            "RelativeColorimetric".to_string(),
            "Saturation".to_string(),
            "AbsoluteColorimetric".to_string(),
        ];
        self.selected_intent = 1;
        self.bpc = true;
        self.open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) {
        if !self.open { return; }
        crate::dialog_chrome::show_floating(ctx, "document.convert_icc", text("menu.doc.convert_icc"), |ui| {

            ui.label(text("dialog.profile.format"));
            let format_labels: Vec<&str> = self.formats.iter().map(String::as_str).collect();
            if !format_labels.is_empty() {
                if dtool_design::dropdown(ui, "convert_icc.format", &mut self.selected_format, &format_labels).changed() {
                    match dtool_core::colour::services().read() {
                        Ok(services) => {
                            let model = match self.formats[self.selected_format].as_str() {
                                "RGBA/8" => dtool_core::colour::ColourModel::Rgb,
                                "CMYK/8" => dtool_core::colour::ColourModel::Cmyk,
                                "LAB/16" => dtool_core::colour::ColourModel::Lab,
                                _ => dtool_core::colour::ColourModel::Grey,
                            };
                            self.profiles = services.profiles.list(model).into_iter().map(str::to_string).collect();
                            self.selected_profile = 0;
                        },
                        Err(_) => { self.profiles.clear(); }
                    }
                }
            }

            ui.label(text("dialog.profile.profile"));
            let profile_labels: Vec<&str> = self.profiles.iter().map(String::as_str).collect();
            if !profile_labels.is_empty() { dtool_design::dropdown(ui, "convert_icc.profile", &mut self.selected_profile, &profile_labels); }

            ui.label(text("dialog.profile.intent"));
            let intent_labels: Vec<&str> = self.intents.iter().map(String::as_str).collect();
            if !intent_labels.is_empty() { dtool_design::dropdown(ui, "convert_icc.intent", &mut self.selected_intent, &intent_labels); }

            dtool_design::toggle(ui, &mut self.bpc, text("dialog.profile.bpc"));

            if let Some(key) = &self.error_key { ui.label(text(key)); }

            ui.horizontal(|ui| {
                let can_run = !self.profiles.is_empty() && self.profiles.get(self.selected_profile).is_some();
                if ui.add_enabled_ui(can_run, |ui| dtool_design::button(ui, text("button.ok"))).inner.clicked() {
                    if let Some(profile) = self.profiles.get(self.selected_profile) {
                        let format_str = &self.formats[self.selected_format];
                        let model = match format_str.as_str() {
                            "RGBA/8" => dtool_core::colour::ColourModel::Rgb,
                            "CMYK/8" => dtool_core::colour::ColourModel::Cmyk,
                            "LAB/16" => dtool_core::colour::ColourModel::Lab,
                            _ => dtool_core::colour::ColourModel::Grey,
                        };

                        let intent = match self.intents[self.selected_intent].as_str() {
                            "Perceptual" => dtool_core::colour::RenderingIntent::Perceptual,
                            "RelativeColorimetric" => dtool_core::colour::RenderingIntent::RelativeColorimetric,
                            "Saturation" => dtool_core::colour::RenderingIntent::Saturation,
                            _ => dtool_core::colour::RenderingIntent::AbsoluteColorimetric,
                        };

                        match session.run_named("color.profile.convert", serde_json::json!({
                            "format": format_str,
                            "profile": profile,
                            "intent": intent,
                            "bpc": self.bpc
                        })) {
                            Ok(_) => self.open = false,
                            Err(error) => self.error_key = Some(error.message_key),
                        }
                    }
                }
                if dtool_design::button(ui, text("button.cancel")).clicked() { self.open = false; }
            });
        });
    }
}
