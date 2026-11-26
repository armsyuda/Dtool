use dtool_core::ai_image::{APPLY, CANCEL, EDIT, GENERATE, STATUS};
use dtool_core::Session;
use dtool_design::{ControlState, NumericSpec, button_with, panel_columns};
use dtool_strings::text;
use serde_json::{Value, json};
use std::sync::mpsc;

type ModelResult = Result<Vec<String>, dtool_core::CommandError>;

pub struct AiImagePanel {
    port: f64,
    model: String,
    checkpoints: Vec<String>,
    models_port: u16,
    model_query: Option<(u16, mpsc::Receiver<ModelResult>)>,
    prompt: String,
    negative: String,
    size: [f64; 2],
    steps: f64,
    count: f64,
    seed: String,
    strength: f64,
    editing: bool,
    use_selection: bool,
    index: usize,
    texture: Option<(dtool_core::workspace::ContextId, u64, usize, egui::TextureHandle)>,
    feedback: Option<String>,
}

impl Default for AiImagePanel {
    fn default() -> Self {
        Self {
            port: 8188.0, model: String::new(), checkpoints: Vec::new(), models_port: 0, model_query: None,
            prompt: String::new(), negative: String::new(),
            size: [512.0, 512.0], steps: 20.0, count: 1.0, seed: "0".into(),
            strength: 55.0, editing: false, use_selection: false, index: 0, texture: None, feedback: None,
        }
    }
}

impl AiImagePanel {
    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        self.poll_models();
        let status = session.run_named(STATUS, json!({})).unwrap_or(Value::Null);
        let running = status["state"] == "running";
        egui::ScrollArea::vertical().id_salt("ai.image.options").show(ui, |ui| {
            ui.add_enabled_ui(!running, |ui| self.options(ui, session));
            if running {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(text("ai.image.running"));
                });
                if dtool_design::button(ui, text("button.cancel")).clicked() {
                    self.feedback = session.run_named(CANCEL, json!({})).err().map(|error| error.message_key);
                }
            } else if status["state"] == "done" {
                self.candidates(ui, session, &status);
            } else if status["state"] == "failed" {
                if let Some(key) = status["error"]["message_key"].as_str() {
                    ui.label(text(key));
                }
            } else if status["state"] == "cancelled" {
                ui.label(text("error.cancelled"));
            }
            if let Some(key) = &self.feedback {
                ui.label(text(key));
            }
        });
    }

    fn options(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        panel_columns(ui, 2, 90.0, |ui, column| {
            let edit = column == 1;
            let label = if edit { "ai.image.edit" } else { "ai.image.generate" };
            if dtool_design::tab(ui, self.editing == edit, text(label)).clicked() {
                self.editing = edit;
            }
        });
        let selected = match session.selection.ids.as_slice() {
            [id] => session.document.get_object(*id),
            _ => None,
        };
        let source = selected.filter(|object| object.raster_size.is_some() || session.document.pixel_layers.get(object.id).is_some());
        if self.editing {
            if let Some(source) = source {
                ui.label(&source.name);
            } else {
                ui.label(text("error.ai_image_source"));
            }
        }
        let source = source.map(|object| object.id);
        let caption = ui.label(text("ai.image.checkpoint"));
        if self.models_port == self.port as u16 && !self.checkpoints.is_empty() {
            let label = if self.model.is_empty() { text("ai.image.select_checkpoint") } else { &self.model };
            egui::ComboBox::from_id_salt("ai.image.checkpoints").width(ui.available_width()).truncate()
                .selected_text(label).show_ui(ui, |ui| {
                    for model in &self.checkpoints {
                        ui.selectable_value(&mut self.model, model.clone(), model);
                    }
                }).response.labelled_by(caption.id);
        } else {
            ui.add(egui::TextEdit::singleline(&mut self.model).desired_width(ui.available_width()))
                .labelled_by(caption.id);
        }
        let enabled = self.model_query.is_none();
        if button_with(ui, text("ai.image.refresh_models"), &ControlState::enabled(enabled)).clicked() {
            let port = self.port as u16;
            let (send, receive) = mpsc::channel();
            let repaint = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = send.send(dtool_core::ai_image::models(port));
                repaint.request_repaint();
            });
            self.model_query = Some((port, receive));
        }
        let caption = ui.label(text("ai.image.prompt"));
        ui.add(egui::TextEdit::multiline(&mut self.prompt).desired_rows(3).desired_width(ui.available_width()))
            .labelled_by(caption.id);
        let caption = ui.label(text("ai.image.negative"));
        ui.add(egui::TextEdit::singleline(&mut self.negative).desired_width(ui.available_width()))
            .labelled_by(caption.id);

        if self.editing {
            number(ui, "ai.image.strength", &mut self.strength, 0.0..=100.0, "%");
            dtool_design::toggle(ui, &mut self.use_selection, text("ai.image.use_selection"));
        } else {
            panel_columns(ui, 2, 110.0, |ui, axis| {
                let key = ["ai.image.width", "ai.image.height"][axis];
                number(ui, key, &mut self.size[axis], 64.0..=2048.0, " px");
            });
        }
        panel_columns(ui, 3, 105.0, |ui, column| match column {
            0 => number(ui, "ai.image.steps", &mut self.steps, 1.0..=100.0, ""),
            1 => number(ui, "ai.image.count", &mut self.count, 1.0..=4.0, ""),
            _ => {
                let caption = ui.label(text("ai.image.seed"));
                ui.add(egui::TextEdit::singleline(&mut self.seed).desired_width(60.0).char_limit(20))
                    .labelled_by(caption.id);
            }
        });
        ui.collapsing(text("ai.image.connection"), |ui| {
            number(ui, "ai.image.port", &mut self.port, 1.0..=65535.0, "");
        });
        let seed = self.seed.parse::<u64>().ok();
        let ready = !self.model.trim().is_empty() && !self.prompt.trim().is_empty()
            && seed.is_some() && (!self.editing || source.is_some());
        if button_with(ui, text("ai.image.run"), &ControlState::enabled(ready)).clicked() {
            let name = if self.editing { EDIT } else { GENERATE };
            let size = self.size.map(|side| ((side / 8.0).round() as u32) * 8);
            let params = json!({
                "port": self.port as u16, "model": self.model.trim(),
                "prompt": self.prompt.trim(), "negative": self.negative.trim(),
                "width": size[0], "height": size[1], "steps": self.steps as u32,
                "count": self.count as u32, "seed": seed.unwrap_or_default(),
                "strength": self.strength / 100.0, "source": if self.editing { source } else { None },
                "use_selection": self.editing && self.use_selection,
            });
            self.index = 0;
            self.texture = None;
            self.feedback = session.run_named(name, params).err().map(|error| error.message_key);
        }
    }

    fn poll_models(&mut self) {
        let Some((port, receive)) = &self.model_query else { return };
        let port = *port;
        match receive.try_recv() {
            Ok(Ok(models)) => {
                self.checkpoints = models;
                self.models_port = port;
                self.feedback = self.checkpoints.is_empty().then(|| "ai.image.no_checkpoints".into());
            }
            Ok(Err(error)) => self.feedback = Some(error.message_key),
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => self.feedback = Some("error.ai_image_response".into()),
        }
        self.model_query = None;
    }

    fn candidates(&mut self, ui: &mut egui::Ui, session: &mut Session, status: &Value) {
        let Some(job) = status["job_id"].as_u64() else { return };
        let count = status["count"].as_u64().unwrap_or(0) as usize;
        if count == 0 { return; }
        self.index = self.index.min(count - 1);
        ui.horizontal_wrapped(|ui| {
            for index in 0..count {
                if dtool_design::tab(ui, self.index == index, (index + 1).to_string()).clicked() {
                    self.index = index;
                }
            }
        });
        let Some(context) = session.context else { return };
        if !self.texture.as_ref().is_some_and(|(owner, id, index, _)| (*owner, *id, *index) == (context, job, self.index))
            && let Some(image) = session.ai_image.preview_image(self.index)
        {
            let rgba = egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba);
            let texture = ui.ctx().load_texture("ai.image.preview", rgba, egui::TextureOptions::LINEAR);
            self.texture = Some((context, job, self.index, texture));
        }
        if let Some((_, _, _, texture)) = &self.texture {
            let size = texture.size_vec2();
            let scale = (ui.available_width().max(1.0) / size.x).min(280.0 / size.y).min(1.0);
            ui.add(egui::Image::new((texture.id(), size * scale)));
        }
        if dtool_design::button(ui, text("ai.image.apply")).clicked() {
            self.feedback = Some(match session.run_named(APPLY, json!({"job_id": job, "index": self.index})) {
                Ok(_) => "ai.image.applied".into(),
                Err(error) => error.message_key,
            });
        }
    }
}

fn number(ui: &mut egui::Ui, key: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, unit: &str) {
    let caption = ui.label(text(key));
    let spec = NumericSpec::new("").range(range).suffix(unit).decimals(0).width(64.0);
    dtool_design::numeric_value(ui, ui.make_persistent_id(key), value, &spec).labelled_by(caption.id);
}
