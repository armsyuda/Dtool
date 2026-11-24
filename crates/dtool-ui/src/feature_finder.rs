use dtool_core::{CommandError, Session};
use dtool_strings::text;
use serde_json::{json, Value};

pub const MENU_ID: &str = "help.feature_finder";

#[derive(Debug, Clone, Default)]
pub struct FeatureFinder {
    pub open: bool,
    pub query: String,
    pub hits: Vec<String>,
    pub error_key: Option<String>,
}

pub fn search(session: &mut Session, query: &str) -> Result<Vec<String>, CommandError> {
    let value = session.run_named(MENU_ID, json!({ "q": query }))?;
    Ok(value.get("hits").and_then(Value::as_array).map(|hits| hits.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default())
}

pub fn hit_label(name: &str) -> String {
    let key = format!("command.{name}");
    if dtool_strings::has(&key) { text(&key).to_string() } else { name.to_string() }
}

impl FeatureFinder {
    pub fn open(&mut self) {
        self.open = true;
        self.error_key = None;
    }

    pub fn refresh(&mut self, session: &mut Session) {
        if self.query.trim().is_empty() {
            self.hits.clear();
            return;
        }
        match search(session, self.query.trim()) {
            Ok(hits) => {
                self.hits = hits;
                self.error_key = None;
            }
            Err(error) => self.error_key = Some(error.message_key),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let mut changed = false;
        let mut picked: Option<String> = None;
        egui::Window::new(text("dialog.feature_finder.title")).open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(text("dialog.feature_finder.query"));
                changed = ui.text_edit_singleline(&mut self.query).changed();
            });
            if self.hits.is_empty() && !self.query.trim().is_empty() {
                ui.label(text("dialog.feature_finder.none"));
            }
            egui::ScrollArea::vertical().max_height(dtool_design::number("dialog.max_body_height")).show(ui, |ui| {
                for hit in &self.hits {
                    if dtool_design::menu_item(ui, hit_label(hit)).clicked() {
                        picked = Some(hit.clone());
                    }
                }
            });
            if let Some(key) = &self.error_key {
                ui.colored_label(ui.visuals().error_fg_color, text(key));
            }
        });
        self.open = open;
        if changed {
            self.refresh(session);
        }
        if let Some(name) = picked {
            match session.run_named(&name, json!({})) {
                Ok(_) => self.open = false,
                Err(error) => self.error_key = Some(error.message_key),
            }
        }
    }
}
