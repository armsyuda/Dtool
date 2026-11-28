mod setup;
mod workspace;

use dtool_core::ai_image::{APPLY, CANCEL, DEPTH_MAP, EDIT, GENERATE, OUTPAINT, REGENERATE_DETAIL, REMOVE_BACKGROUND, SELECT_SUBJECT, STATUS, UPSCALE, detail_output_size, flux3, outpaint, qwen, reference_alignment, upscale};
use dtool_core::Session;
use dtool_design::{ControlState, NumericSpec, button_with, panel_columns};
use dtool_strings::text;
use serde_json::{Value, json};
use std::sync::mpsc;

type ModelResult = Result<Vec<String>, dtool_core::CommandError>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageAction {
    Generate,
    Edit,
    RemoveBackground,
    SelectSubject,
    Upscale,
    DepthMap,
    Outpaint,
    RegenerateDetail,
}

impl ImageAction {
    const TOOLS: [(Self, &'static str); 8] = [
        (Self::Generate, "ai.image.generate"),
        (Self::Edit, "ai.image.edit"),
        (Self::RemoveBackground, "ai.image.remove_background"),
        (Self::SelectSubject, SELECT_SUBJECT),
        (Self::Upscale, "ai.image.upscale"),
        (Self::RegenerateDetail, "ai.image.regenerate_detail"),
        (Self::Outpaint, "ai.image.outpaint"),
        (Self::DepthMap, "ai.image.depth_map"),
    ];

    fn key(self) -> &'static str {
        Self::TOOLS.iter().find(|(action, _)| *action == self).unwrap().1
    }

    fn uses_prompt(self) -> bool {
        self == Self::RegenerateDetail || self.has_candidate_count()
    }

    fn has_candidate_count(self) -> bool {
        matches!(self, Self::Generate | Self::Edit | Self::Outpaint)
    }
}

struct RenderPreset {
    model: String,
    steps: f64,
}

impl RenderPreset {
    fn new(steps: f64) -> Self {
        Self { model: String::new(), steps }
    }
}

pub struct AiImagePanel {
    port: f64,
    detail: bool,
    generation_presets: [RenderPreset; 2],
    editing_presets: [RenderPreset; 2],
    outpaint_presets: [RenderPreset; 2],
    detail_presets: [RenderPreset; 2],
    checkpoints: Vec<String>,
    models_port: u16,
    model_query: Option<(u16, mpsc::Receiver<ModelResult>)>,
    prompt: String,
    negative: String,
    size: [f64; 2],
    count: f64,
    seed: String,
    strength: f64,
    action: ImageAction,
    upscale_scale: u32,
    upscale_model: upscale::Model,
    depth_color: bool,
    outpaint_borders: [f64; 4],
    outpaint_percent: bool,
    outpaint_to_canvas: bool,
    detail_scale: u32,
    detail_strength: f64,
    protect_detail_selection: bool,
    use_selection: bool,
    index: usize,
    texture: Option<(dtool_core::workspace::ContextId, u64, usize, egui::TextureHandle)>,
    feedback: Option<String>,
    setup: setup::SetupPanel,
    workspace_view: workspace::WorkspaceView,
    icons: crate::icon_cache::IconCache,
}

impl Default for AiImagePanel {
    fn default() -> Self {
        Self {
            port: 8188.0, detail: false,
            generation_presets: [RenderPreset::new(12.0), RenderPreset::new(30.0)],
            editing_presets: [RenderPreset::new(12.0), RenderPreset::new(30.0)],
            outpaint_presets: [RenderPreset::new(16.0), RenderPreset::new(30.0)],
            detail_presets: [RenderPreset::new(16.0), RenderPreset::new(28.0)],
            checkpoints: Vec::new(), models_port: 0, model_query: None,
            prompt: String::new(), negative: String::new(),
            size: [512.0, 512.0], count: 1.0, seed: "0".into(),
            strength: 55.0, action: ImageAction::Generate, use_selection: false, index: 0, texture: None, feedback: None,
            upscale_scale: 2, upscale_model: upscale::Model::PreserveOriginal,
            depth_color: false,
            outpaint_borders: [0.0, 0.0, 96.0, 0.0], outpaint_percent: false, outpaint_to_canvas: false,
            detail_scale: 2, detail_strength: 20.0, protect_detail_selection: true,
            setup: Default::default(), workspace_view: Default::default(), icons: Default::default(),
        }
    }
}

impl AiImagePanel {
    fn set_action(&mut self, action: ImageAction) {
        if self.action != action {
            self.action = action;
            self.feedback = None;
        }
    }

    fn presets(&mut self) -> &mut [RenderPreset; 2] {
        match self.action {
            ImageAction::Edit => &mut self.editing_presets,
            ImageAction::Outpaint => &mut self.outpaint_presets,
            ImageAction::RegenerateDetail => &mut self.detail_presets,
            _ => &mut self.generation_presets,
        }
    }

    fn active_preset(&self) -> &RenderPreset {
        let presets = match self.action {
            ImageAction::Edit => &self.editing_presets,
            ImageAction::Outpaint => &self.outpaint_presets,
            ImageAction::RegenerateDetail => &self.detail_presets,
            _ => &self.generation_presets,
        };
        &presets[usize::from(self.detail)]
    }

    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        self.setup.poll(ui.ctx(), &mut self.port);
        self.poll_models();
        let status = session.run_named(STATUS, json!({})).unwrap_or(Value::Null);
        let running = status["state"] == "running";
        egui::ScrollArea::vertical().id_salt("ai.image.options").show(ui, |ui| {
            ui.add_enabled_ui(!running, |ui| self.options(ui, session, false));
            if running {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(text("ai.image.running"));
                    if let Some(progress) = status["progress"].as_u64() {
                        ui.label(format!("{progress}%"));
                    }
                });
                if dtool_design::button(ui, text("button.cancel")).clicked() {
                    self.feedback = session.run_named(CANCEL, json!({})).err().map(|error| error.message_key);
                }
            } else if status["state"] == "done" && status["operation"] == self.action.key() {
                self.candidates(ui, session, &status);
            } else if status["state"] == "failed" && status["operation"] == self.action.key() {
                if let Some(key) = status["error"]["message_key"].as_str() {
                    ui.label(text(key));
                }
            } else if status["state"] == "cancelled" && status["operation"] == self.action.key() {
                ui.label(text("error.cancelled"));
            }
            if let Some(key) = &self.feedback {
                ui.label(text(key));
            }
        });
    }

    fn options(&mut self, ui: &mut egui::Ui, session: &mut Session, docked: bool) {
        if !docked { panel_columns(ui, ImageAction::TOOLS.len(), 90.0, |ui, column| {
            let (action, label) = ImageAction::TOOLS[column];
            if dtool_design::tab(ui, self.action == action, text(label)).clicked() {
                self.set_action(action);
            }
        });
            if self.action.uses_prompt() {
                ui.horizontal(|ui| quality_tabs(ui, &mut self.detail));
            }
        }
        let editing = self.action == ImageAction::Edit;
        let outpainting = self.action == ImageAction::Outpaint;
        let regenerating = self.action == ImageAction::RegenerateDetail;
        let selected = match session.selection.ids.as_slice() {
            [id] => session.document.get_object(*id),
            _ => None,
        };
        let source = selected.filter(|object| object.raster_size.is_some() || session.document.pixel_layers.get(object.id).is_some());
        if self.action != ImageAction::Generate {
            option_section(ui, "원본 레이어", false);
            if let Some(source) = source {
                ui.label(&source.name);
            } else {
                ui.label(text("error.ai_image_source"));
            }
        }
        if matches!(self.action, ImageAction::RemoveBackground | ImageAction::SelectSubject) {
            option_section(ui, "처리", true);
            if self.action == ImageAction::SelectSubject {
                ui.label(text("ai.image.subject_hint"));
            }
            ui.label(text("ai.image.background_setup_hint"));
            if !docked {
                ui.collapsing(text("ai.image.connection"), |ui| {
                    number(ui, "ai.image.port", &mut self.port, 1.0..=65535.0, "");
                });
                self.render_control(ui, session);
            }
            return;
        }
        if self.action == ImageAction::DepthMap {
            option_section(ui, "미리보기 형식", true);
            ui.horizontal_wrapped(|ui| {
                for (color, key) in [(false, "ai.image.depth_gray"), (true, "ai.image.depth_color")] {
                    if dtool_design::tab(ui, self.depth_color == color, text(key)).clicked() {
                        self.depth_color = color;
                    }
                }
            });
            ui.label(text(if self.depth_color { "ai.image.depth_color_hint" } else { "ai.image.depth_gray_hint" }));
            ui.label(text("ai.image.depth_setup_hint"));
            if !docked {
                ui.collapsing(text("ai.image.connection"), |ui| {
                    number(ui, "ai.image.port", &mut self.port, 1.0..=65535.0, "");
                });
                self.render_control(ui, session);
            }
            return;
        }
        if self.action == ImageAction::Upscale {
            option_section(ui, text("ai.image.upscale_model"), true);
            ui.horizontal_wrapped(|ui| {
                let modes = [
                    (upscale::Model::PreserveOriginal, "ai.image.upscale_preserve"),
                    (upscale::Model::Standard, "ai.image.upscale_standard"),
                    (upscale::Model::Illustration, "ai.image.upscale_illustration"),
                ];
                for (mode, key) in modes {
                    if dtool_design::tab(ui, self.upscale_model == mode, text(key)).clicked() {
                        self.upscale_model = mode;
                    }
                }
            });
            option_section(ui, text("ai.image.upscale_scale"), true);
            ui.horizontal_wrapped(|ui| {
                for scale in 2..=4 {
                    if dtool_design::tab(ui, self.upscale_scale == scale, format!("{scale}×")).clicked() {
                        self.upscale_scale = scale;
                    }
                }
            });
            if let Some(source) = source {
                if let Some([w, h]) = self.upscale_size(session, source.id) {
                    ui.label(format!("{}  {w} × {h} px", text("ai.image.upscale_output")));
                } else {
                    ui.label(text("error.ai_image_upscale_size"));
                }
            }
            if self.upscale_model == upscale::Model::PreserveOriginal {
                ui.label(text("ai.image.upscale_preserve_hint"));
            } else {
                self.setup.upscale_install(ui);
            }
            if !docked {
                self.render_control(ui, session);
            }
            return;
        }
        if outpainting {
            option_section(ui, "확장 범위", true);
            self.outpaint_options(ui, session, source.map(|object| object.id));
        }
        if regenerating {
            option_section(ui, "해상도 및 원본 보호", true);
            ui.label(text("ai.image.detail_hint"));
            dtool_design::toggle(ui, &mut self.protect_detail_selection, text("ai.image.detail_protect"));
            if self.protect_detail_selection {
                ui.label(text("ai.image.detail_protect_hint"));
            }
            ui.horizontal_wrapped(|ui| {
                for (scale, key) in [(1, "ai.image.detail_same_size"), (2, "ai.image.detail_double_size")] {
                    if dtool_design::tab(ui, self.detail_scale == scale, text(key)).clicked() {
                        self.detail_scale = scale;
                    }
                }
            });
            if let Some(source) = source {
                if let Some([w, h]) = self.detail_size(session, source.id) {
                    ui.label(format!("{}  {w} × {h} px", text("ai.image.upscale_output")));
                } else {
                    ui.label(text("error.ai_image_detail_size"));
                }
            }
            number(ui, "ai.image.detail_strength", &mut self.detail_strength, 8.0..=60.0, "%");
        }
        let caption = option_section(ui, text("ai.image.checkpoint"), self.action != ImageAction::Generate);
        let index = usize::from(self.detail);
        let presets = match self.action {
            ImageAction::Edit => &mut self.editing_presets,
            ImageAction::Outpaint => &mut self.outpaint_presets,
            ImageAction::RegenerateDetail => &mut self.detail_presets,
            _ => &mut self.generation_presets,
        };
        let model = &mut presets[index].model;
        if self.models_port == self.port as u16 && !self.checkpoints.is_empty() {
            let label = if model.is_empty() { text("ai.image.select_checkpoint") } else { model_filename(model) };
            egui::ComboBox::from_id_salt("ai.image.checkpoints").width(ui.available_width()).truncate()
                .selected_text(label).show_ui(ui, |ui| {
                    for checkpoint in self.checkpoints.iter().filter(|checkpoint| !(outpainting || regenerating) || reference_alignment(checkpoint).is_none()) {
                        ui.selectable_value(model, checkpoint.clone(), model_filename(checkpoint))
                            .on_hover_text(checkpoint);
                    }
                }).response.labelled_by(caption.id).on_hover_text(model.as_str());
        } else {
            ui.add(egui::TextEdit::singleline(model).desired_width(ui.available_width()))
                .labelled_by(caption.id);
        }
        let qwen = qwen::is_model(model);
        let flux_steps = flux3::steps(model);
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
        let caption = option_section(ui, text(if regenerating { "ai.image.detail_prompt" } else { "ai.image.prompt" }), true);
        ui.add(egui::TextEdit::multiline(&mut self.prompt).desired_rows(3).desired_width(ui.available_width()))
            .labelled_by(caption.id);
        let caption = ui.label(text("ai.image.negative"));
        ui.add(egui::TextEdit::singleline(&mut self.negative).desired_width(ui.available_width()))
            .labelled_by(caption.id);

        if editing {
            option_section(ui, "편집 범위", true);
            if !qwen && flux_steps.is_none() {
                number(ui, "ai.image.strength", &mut self.strength, 0.0..=100.0, "%");
            }
            dtool_design::toggle(ui, &mut self.use_selection, text("ai.image.use_selection"));
        } else if !outpainting && !regenerating {
            option_section(ui, "출력 크기", true);
            panel_columns(ui, 2, 110.0, |ui, axis| {
                let key = ["ai.image.width", "ai.image.height"][axis];
                number(ui, key, &mut self.size[axis], 64.0..=2048.0, " px");
            });
        }
        option_section(ui, "렌더 설정", true);
        let show_count = !docked && self.action.has_candidate_count();
        panel_columns(ui, if show_count { 3 } else { 2 }, 105.0, |ui, column| match column {
            0 if qwen => {
                ui.label(format!("{} {}", text("ai.image.qwen_fixed_steps"), qwen::STEPS));
            }
            0 if flux_steps.is_some() => {
                ui.label(format!("{} {}", text("ai.image.flux3_fixed_steps"), flux_steps.unwrap_or_default()));
            }
            0 => number(ui, "ai.image.steps", &mut self.presets()[index].steps, 1.0..=100.0, ""),
            1 if show_count => number(ui, "ai.image.count", &mut self.count, 1.0..=4.0, ""),
            _ => {
                let caption = ui.label(text("ai.image.seed"));
                ui.add(egui::TextEdit::singleline(&mut self.seed).desired_width(60.0).char_limit(20))
                    .labelled_by(caption.id);
            }
        });
        if !docked { ui.collapsing(text("ai.image.connection"), |ui| {
            number(ui, "ai.image.port", &mut self.port, 1.0..=65535.0, "");
        });
        self.render_control(ui, session);
        }
    }

    fn render_control(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        let source = match session.selection.ids.as_slice() {
            [id] if session.document.get_object(*id).is_some_and(|object| {
                object.raster_size.is_some() || session.document.pixel_layers.get(*id).is_some()
            }) => Some(*id),
            _ => None,
        };
        if self.action == ImageAction::SelectSubject {
            let ready = source.and_then(|id| session.document.get_object(id))
                .is_some_and(|object| object.kind == dtool_core::ObjectKind::Image);
            if render_button(ui, text("ai.image.subject_run"), ready).clicked() {
                self.submit(session, SELECT_SUBJECT, json!({ "port": self.port as u16, "source": source }));
            }
            return;
        }
        if self.action == ImageAction::RemoveBackground {
            if render_button(ui, text("ai.image.remove_background"), source.is_some()).clicked() {
                self.submit(session, REMOVE_BACKGROUND, json!({ "port": self.port as u16, "source": source }));
            }
            return;
        }
        if self.action == ImageAction::DepthMap {
            if render_button(ui, text("ai.image.depth_run"), source.is_some()).clicked() {
                self.submit(session, DEPTH_MAP, json!({
                    "port": self.port as u16, "source": source, "depth_color": self.depth_color,
                }));
            }
            return;
        }
        if self.action == ImageAction::Upscale {
            let ready = (self.upscale_model == upscale::Model::PreserveOriginal || self.setup.upscale_ready)
                && source.and_then(|id| self.upscale_size(session, id)).is_some();
            if render_button(ui, text("ai.image.upscale_run"), ready).clicked() {
                self.submit(session, UPSCALE, json!({
                    "source": source, "scale": self.upscale_scale,
                    "upscale_model": self.upscale_model.key(),
                }));
            }
            return;
        }
        let editing = self.action == ImageAction::Edit;
        let outpainting = self.action == ImageAction::Outpaint;
        let regenerating = self.action == ImageAction::RegenerateDetail;
        let seed = self.seed.parse::<u64>().ok();
        let preset = self.active_preset();
        let borders = outpainting.then(|| self.expansion(session, source)).flatten();
        if regenerating {
            let ready = source.and_then(|id| self.detail_size(session, id)).is_some()
                && !preset.model.trim().is_empty() && reference_alignment(&preset.model).is_none() && seed.is_some()
                && (!self.protect_detail_selection || session.document.pixel_selection.is_some());
            if render_button(ui, text("ai.image.detail_run"), ready).clicked() {
                self.submit(session, REGENERATE_DETAIL, json!({
                    "source": source, "port": self.port as u16, "model": preset.model.trim(),
                    "prompt": self.prompt.trim(), "negative": self.negative.trim(), "steps": preset.steps as u32,
                    "scale": self.detail_scale, "strength": self.detail_strength / 100.0, "seed": seed.unwrap_or_default(),
                    "protect_selection": self.protect_detail_selection,
                }));
            }
            return;
        }
        let ready = !preset.model.trim().is_empty() && !self.prompt.trim().is_empty()
            && seed.is_some() && (!editing || source.is_some())
            && (!outpainting || (borders.is_some() && reference_alignment(&preset.model).is_none()));
        if render_button(ui, text("ai.image.run"), ready).clicked() {
            let name = match self.action { ImageAction::Edit => EDIT, ImageAction::Outpaint => OUTPAINT, _ => GENERATE };
            let qwen = qwen::is_model(&preset.model);
            let alignment = reference_alignment(&preset.model).unwrap_or(8);
            let size = self.size.map(|side| ((side / f64::from(alignment)).round() as u32) * alignment);
            let params = json!({
                "port": self.port as u16, "model": preset.model.trim(),
                "prompt": self.prompt.trim(), "negative": self.negative.trim(),
                "width": size[0], "height": size[1], "steps": if qwen { qwen::STEPS } else { flux3::steps(&preset.model).unwrap_or(preset.steps as u32) },
                "count": self.count as u32, "seed": seed.unwrap_or_default(),
                "strength": self.strength / 100.0, "source": if editing || outpainting { source } else { None },
                "use_selection": editing && self.use_selection,
                "borders": borders.map_or([0; 4], |(_, borders)| borders), "to_canvas": outpainting && self.outpaint_to_canvas,
            });
            self.submit(session, name, params);
        }
    }

    fn outpaint_options(&mut self, ui: &mut egui::Ui, session: &Session, source: Option<dtool_core::ObjectId>) {
        ui.label(text("ai.image.outpaint_hint"));
        dtool_design::toggle(ui, &mut self.outpaint_to_canvas, text("ai.image.outpaint_canvas"));
        if !self.outpaint_to_canvas {
            ui.horizontal_wrapped(|ui| {
                for (percent, label) in [(false, "px"), (true, "%")] {
                    if dtool_design::tab(ui, self.outpaint_percent == percent, label).clicked() && self.outpaint_percent != percent {
                        if let Some([w, h]) = source.and_then(|id| outpaint::image_size(&session.document, id)) {
                            for (side, dimension) in self.outpaint_borders.iter_mut().zip([w, h, w, h]) {
                                let factor = f64::from(dimension) / 100.0;
                                *side = if percent { *side / factor } else { *side * factor };
                            }
                        }
                        self.outpaint_percent = percent;
                    }
                }
            });
            let suffix = if self.outpaint_percent { "%" } else { " px" };
            panel_columns(ui, 2, 110.0, |ui, column| {
                let keys = [["ai.image.outpaint_left", "ai.image.outpaint_right"], ["ai.image.outpaint_top", "ai.image.outpaint_bottom"]];
                for (row, keys) in keys.into_iter().enumerate() {
                    let key = keys[column];
                    let caption = ui.label(text(key));
                    let spec = NumericSpec::new("").range(0.0..=2048.0).suffix(suffix)
                        .decimals(if self.outpaint_percent { 2 } else { 0 }).width(64.0);
                    dtool_design::numeric_value(ui, ui.make_persistent_id(key), &mut self.outpaint_borders[row + column * 2], &spec)
                        .labelled_by(caption.id);
                }
            });
        }
        if let Some(([w, h], _)) = self.expansion(session, source) {
            ui.label(format!("{}  {w} × {h} px", text("ai.image.upscale_output")));
        } else if source.is_some() {
            ui.label(text(if self.outpaint_to_canvas { "error.ai_image_outpaint_canvas" } else { "error.ai_image_outpaint_size" }));
        }
    }

    fn expansion(&self, session: &Session, source: Option<dtool_core::ObjectId>) -> Option<([u32; 2], outpaint::Borders)> {
        let id = source?;
        let [w, h] = outpaint::image_size(&session.document, id)?;
        let borders = if self.outpaint_to_canvas {
            outpaint::canvas_borders(&session.document, id)?
        } else {
            let mut borders = self.outpaint_borders;
            if self.outpaint_percent {
                for (value, dimension) in borders.iter_mut().zip([w, h, w, h]) {
                    *value *= f64::from(dimension) / 100.0;
                }
            }
            borders.map(|value| value.round() as u32)
        };
        Some((outpaint::output_size([w, h], borders)?, borders))
    }

    fn upscale_size(&self, session: &Session, id: dtool_core::ObjectId) -> Option<[u32; 2]> {
        upscale::output_size(Self::raster_size(session, id)?, self.upscale_scale)
    }

    fn detail_size(&self, session: &Session, id: dtool_core::ObjectId) -> Option<[u32; 2]> {
        detail_output_size(Self::raster_size(session, id)?, self.detail_scale)
    }

    fn raster_size(session: &Session, id: dtool_core::ObjectId) -> Option<[u32; 2]> {
        let size = session.document.pixel_layers.get(id).map(|layer| (layer.width, layer.height))
            .or_else(|| session.document.get_object(id)?.raster_size)?;
        Some([size.0, size.1])
    }

    fn submit(&mut self, session: &mut Session, name: &str, params: Value) {
        self.feedback = session.run_named(name, params).err().map(|error| error.message_key);
        if self.feedback.is_none() {
            self.index = 0;
            self.texture = None;
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
        let subject = self.action == ImageAction::SelectSubject;
        let label = if subject { "ai.image.subject_apply" } else { "ai.image.apply" };
        if dtool_design::button(ui, text(label)).clicked() {
            self.feedback = Some(match session.run_named(APPLY, json!({"job_id": job, "index": self.index})) {
                Ok(_) => if subject { "ai.image.subject_applied" } else { "ai.image.applied" }.into(),
                Err(error) => error.message_key,
            });
        }
    }
}

fn model_filename(model: &str) -> &str {
    model.rsplit(['/', '\\']).next().unwrap_or(model)
}

fn option_section(ui: &mut egui::Ui, title: &str, separated: bool) -> egui::Response {
    if separated {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
    }
    ui.label(egui::RichText::new(title).strong().color(workspace::ACCENT))
}

fn render_button(ui: &mut egui::Ui, title: &str, ready: bool) -> egui::Response {
    ui.scope(|ui| {
        dtool_design::button_visuals(ui);
        let mut label = egui::RichText::new(title).strong();
        let mut fill = None;
        if ready && ui.is_enabled() {
            label = label.color(egui::Color32::from_rgb(16, 37, 35));
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.weak_bg_fill = workspace::ACCENT;
            widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(167, 236, 224);
            widgets.active.weak_bg_fill = egui::Color32::from_rgb(111, 197, 184);
        } else {
            fill = Some(egui::Color32::from_rgb(54, 61, 62));
        }
        let mut button = egui::Button::new(label)
            .corner_radius(6.0)
            .min_size(egui::vec2(94.0, 30.0));
        if let Some(fill) = fill {
            button = button.fill(fill);
        }
        ui.add_enabled(ready, button)
    }).inner
}

fn quality_tabs(ui: &mut egui::Ui, detail: &mut bool) {
    let mut choices = [(false, "빠르게"), (true, "정밀하게")];
    if ui.layout().main_dir == egui::Direction::RightToLeft {
        choices.reverse();
    }
    for (quality, label) in choices {
        if dtool_design::tab(ui, *detail == quality, label).clicked() {
            *detail = quality;
        }
    }
}

fn number(ui: &mut egui::Ui, key: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, unit: &str) {
    let caption = ui.label(text(key));
    let spec = NumericSpec::new("").range(range).suffix(unit).decimals(0).width(64.0);
    dtool_design::numeric_value(ui, ui.make_persistent_id(key), value, &spec).labelled_by(caption.id);
}
