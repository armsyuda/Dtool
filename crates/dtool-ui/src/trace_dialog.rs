use std::sync::{Arc, mpsc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dtool_core::{ObjectId, Session};
use dtool_core::raster_view::RasterView;
use dtool_core::trace::{PaletteKind, TraceMethod, TraceMode, TracePreset, TraceSettings, TracedPath};
use dtool_strings::text;
use egui::{Color32, Pos2, Stroke, TextureHandle, Ui, vec2};

use crate::dialog_chrome::{ClosePolicy, DialogMode, DialogResult};

const PREVIEW_DELAY: Duration = Duration::from_millis(150);

struct Source {
    size: [usize; 2],
    rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PreviewView {
    #[default]
    Result,
    Source,
    Outline,
    OutlineWithSource,
    ResultWithOutline,
}

impl PreviewView {
    const CHOICES: [(Self, &'static str); 5] = [
        (Self::Result, "trace.view.result"),
        (Self::ResultWithOutline, "trace.view.result_outline"),
        (Self::Outline, "trace.view.outline_only"),
        (Self::OutlineWithSource, "trace.view.outline"),
        (Self::Source, "trace.view.source"),
    ];

    fn label(self) -> &'static str {
        Self::CHOICES.into_iter().find(|(view, _)| *view == self).unwrap().1
    }
}

struct Preview {
    paths: Vec<TracedPath>,
    size: [u32; 2],
    contours: Vec<Vec<[f32; 2]>>,
    anchors: usize,
    colors: usize,
    image: egui::ColorImage,
}

enum TraceEvent {
    Progress(u32, &'static str),
    Finished(Option<Preview>),
}

struct Worker {
    settings: TraceSettings,
    limit: usize,
    cancel: Arc<AtomicBool>,
    events: mpsc::Receiver<TraceEvent>,
}

impl Drop for Worker {
    fn drop(&mut self) { self.cancel.store(true, Ordering::Relaxed); }
}

#[derive(Default)]
pub struct TraceDialog {
    node: Option<ObjectId>,
    source: Option<Arc<Source>>,
    texture: Option<TextureHandle>,
    chrome: crate::dialog_chrome::DialogChrome,
    settings: TraceSettings,
    preview: Option<Preview>,
    result_texture: Option<TextureHandle>,
    pending: Option<Instant>,
    worker: Option<Worker>,
    owner: Option<(dtool_core::document::DocInstance, u64)>,
    progress: Option<(u32, &'static str)>,
    view: PreviewView,
    limit: usize,
    apply_when_ready: bool,
    live_preview: bool,
    compare_source: bool,
    presets: Vec<TracePreset>,
    selected_preset: Option<String>,
    preset_name: String,
    error: Option<String>,
}

impl TraceDialog {
    pub fn open(&mut self, session: &Session, node: ObjectId) -> dtool_protocol::CommandResult {
        let object = session.document.get_object(node).ok_or_else(dtool_protocol::CommandError::not_found)?;
        let (size, rgba) = dtool_core::raster_source::rgba(&session.document, object)
            .ok_or_else(|| dtool_protocol::CommandError::new("NO_PIXEL_LAYER"))?;
        let presets = dtool_core::commands::trace_presets::presets()?;
        *self = Self {
            node: Some(node),
            owner: Some((session.document.instance, session.generation)),
            limit: 1600,
            live_preview: true,
            selected_preset: Some("Default".into()),
            settings: presets.iter().find(|preset| preset.name == "Default")
                .map(|preset| preset.settings.clone()).unwrap_or_default(),
            source: Some(Arc::new(Source { size, rgba })),
            pending: Some(Instant::now()),
            presets,
            ..Self::default()
        };
        Ok(serde_json::json!({ "node": node.0 }))
    }

    pub fn is_open(&self) -> bool {
        self.node.is_some()
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    pub fn show(&mut self, ui: &Ui, session: &mut Session) -> bool {
        let Some(node) = self.node else { return false };
        if self.owner != Some((session.document.instance, session.generation)) || session.document.get_object(node).is_none() {
            self.close();
            return false;
        }

        let ctx = ui.ctx();
        if self.texture.is_none() && let Some(source) = &self.source {
            self.texture = Some(ctx.load_texture("trace.source", thumbnail(source), egui::TextureOptions::LINEAR));
        }
        let old_settings = self.settings.clone();
        let old_limit = self.limit;
        let old_live_preview = self.live_preview;
        self.update_preview(ctx);
        let mut chrome = std::mem::take(&mut self.chrome);
        let (result, (apply, refresh, cancel)) = chrome.show(ctx, "dtool.dialog.image_trace", text("trace.title"),
            DialogMode::Modal, ClosePolicy::CancelOnly, false, |ui| {
                let screen = ui.ctx().content_rect().size();
                ui.set_width((screen.x - 42.0).clamp(160.0, 760.0));
                let height = (screen.y - 138.0).clamp(65.0, 500.0);
                crate::dialog_chrome::options_scroll(ui, "image_trace.options", height, |ui| {
                    if ui.available_width() >= 580.0 {
                        ui.columns(2, |columns| {
                            self.show_preview(&mut columns[0]);
                            columns[1].add_enabled_ui(!self.apply_when_ready, |ui| self.show_settings(ui, session));
                        });
                    } else {
                        self.show_preview(ui);
                        ui.separator();
                        ui.add_enabled_ui(!self.apply_when_ready, |ui| self.show_settings(ui, session));
                    }
                });
                if let Some((percent, stage)) = self.progress {
                    ui.label(format!("{} · {percent}%", text(stage)));
                    ui.add(egui::ProgressBar::new(percent as f32 / 100.0)
                        .fill(egui::Color32::from_rgb(220, 107, 89))
                        .desired_height(5.0));
                }
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, text(error));
                }
                ui.separator();
                if let Some(preview) = &self.preview {
                    ui.label(dtool_strings::format("trace.preview.stats", &[("paths", &preview.paths.len().to_string()),
                        ("anchors", &preview.anchors.to_string()), ("colors", &preview.colors.to_string())]));
                }
                ui.horizontal_wrapped(|ui| {
                    dtool_design::toggle(ui, &mut self.live_preview, text("trace.live_preview"));
                    let enabled = !self.apply_when_ready && (self.settings.fills || self.settings.strokes);
                    let refresh = ui.add_enabled_ui(enabled && self.preview.is_none(), |ui| {
                        dtool_design::button(ui, text("trace.trace")).clicked()
                    }).inner;
                    let apply = ui.add_enabled_ui(enabled, |ui| dtool_design::button(ui, text("trace.expand")).clicked()).inner;
                    let cancel = dtool_design::button(ui, text("dialog.cancel")).clicked();
                    (apply, refresh, cancel)
                }).inner
            });
        self.chrome = chrome;

        if cancel || result == DialogResult::Cancel {
            self.close();
            return false;
        }
        if self.settings != old_settings && self.selected_preset.as_ref().is_some_and(|name| {
            !self.presets.iter().any(|preset| &preset.name == name && preset.settings == self.settings)
        }) {
            self.selected_preset = None;
        }
        if self.settings != old_settings || self.limit != old_limit || self.live_preview != old_live_preview {
            if let Some(worker) = &self.worker { worker.cancel.store(true, Ordering::Relaxed); }
            self.preview = None;
            self.result_texture = None;
            self.error = None;
            self.progress = self.live_preview.then_some((0, "trace.progress.prepare"));
            self.pending = self.live_preview.then(|| Instant::now() + PREVIEW_DELAY);
            ctx.request_repaint_after(PREVIEW_DELAY);
        }
        if refresh {
            self.pending = Some(Instant::now());
            ctx.request_repaint();
        }
        if apply {
            self.apply_when_ready = true;
            if self.preview.is_none() && self.worker.is_none() { self.pending = Some(Instant::now()); }
            ctx.request_repaint();
        }
        if self.apply_when_ready && let Some(preview) = self.preview.take() {
            self.apply_when_ready = false;
            if preview.paths.is_empty() {
                self.error = Some("trace.no_paths".into());
                return false;
            }
            match dtool_core::commands::image_trace::apply_paths(session, node, preview.size, preview.paths, true, self.settings.auto_group) {
                Ok(_) => { self.close(); return true; }
                Err(error) => self.error = Some(error.message_key),
            }
        }
        false
    }

    fn show_preview(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            let Some(texture) = &self.texture else { return };
            let image_size = texture.size_vec2();
            let scale = (ui.available_width().min(350.0) / image_size.x).min(270.0 / image_size.y);
            let (rect, _) = ui.allocate_exact_size(image_size * scale, egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_gray(215));
            let view = if self.compare_source { PreviewView::Source } else { self.view };
            if view != PreviewView::Outline {
                let shown = if matches!(view, PreviewView::Result | PreviewView::ResultWithOutline) {
                    self.result_texture.as_ref().unwrap_or(texture)
                } else { texture };
                painter.image(shown.id(), rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            if let Some(preview) = &self.preview {
                for contour in preview.contours.iter().filter(|_| !matches!(view, PreviewView::Result | PreviewView::Source)) {
                    let points = contour.iter().map(|[x, y]| rect.min + vec2(x * rect.width(), y * rect.height())).collect::<Vec<_>>();
                    painter.add(egui::Shape::line(points.clone(), Stroke::new(2.5, Color32::from_black_alpha(190))));
                    painter.add(egui::Shape::line(points, Stroke::new(1.0, Color32::from_rgb(255, 188, 71))));
                }
                ui.weak(format!("{} × {} px", preview.size[0], preview.size[1]));
            } else if self.pending.is_some() || self.worker.is_some() {
                ui.label(text("trace.preview.pending"));
            }
        });
    }

    fn show_settings(&mut self, ui: &mut Ui, session: &mut Session) {
        self.show_presets(ui, session);
        if self.settings.mode != TraceMode::FlatLogo {
            let mut enhanced = self.settings.recognize_shapes && self.settings.auto_group;
            if dtool_design::toggle(ui, &mut enhanced, text("trace.enhanced")).changed() {
                self.settings.recognize_shapes = enhanced;
                self.settings.auto_group = enhanced;
            }
        }
        ui.horizontal(|ui| {
            ui.label(text("trace.view"));
            egui::ComboBox::from_id_salt("trace.view")
                .width((ui.available_width() - 32.0).max(75.0))
                .selected_text(text(self.view.label()))
                .show_ui(ui, |ui| {
                    for (view, label) in PreviewView::CHOICES {
                        ui.selectable_value(&mut self.view, view, text(label));
                    }
                });
            let eye = dtool_design::tooltip(dtool_design::icon_button(ui, "◉"), text("trace.compare_source"));
            self.compare_source = eye.is_pointer_button_down_on();
        });
        let modes = [(TraceMode::Color, "trace.mode.color"), (TraceMode::Grayscale, "trace.mode.grayscale"),
            (TraceMode::BlackWhite, "trace.mode.bw"), (TraceMode::FlatLogo, "trace.mode.logo")];
        let previous_mode = self.settings.mode;
        trace_choice(ui, "trace.mode", &mut self.settings.mode, &modes);
        if self.settings.mode == TraceMode::FlatLogo && previous_mode != TraceMode::FlatLogo {
            self.settings.fills = true;
            self.settings.strokes = false;
            self.settings.ignore_color = None;
        }
        if self.settings.mode == TraceMode::BlackWhite || !self.settings.fills {
            let mut value = f32::from(self.settings.threshold);
            trace_slider(ui, "trace.threshold", &mut value, 0.0..=255.0, "", true);
            self.settings.threshold = value.round() as u8;
        } else if self.settings.mode == TraceMode::Grayscale || self.settings.mode == TraceMode::FlatLogo {
            let logo = self.settings.mode == TraceMode::FlatLogo;
            let max = if logo { 10 } else { 256 };
            let initial = match self.settings.palette { PaletteKind::Limited(n) => n.min(max), _ => 8 };
            let mut amount = f32::from(initial);
            trace_slider(ui, if logo { "trace.palette.count" } else { "trace.grays" }, &mut amount, 2.0..=f32::from(max), "", true);
            self.settings.palette = PaletteKind::Limited(amount.round() as u16);
        } else {
            let palette_label = |palette| match palette {
                PaletteKind::Automatic => "trace.palette.auto",
                PaletteKind::Limited(_) => "trace.palette.limited",
                PaletteKind::FullTone => "trace.palette.full",
                PaletteKind::Document(_) => "trace.palette.document",
            };
            ui.horizontal(|ui| {
                ui.label(text("trace.palette"));
                egui::ComboBox::from_id_salt("trace.palette").width(ui.available_width().max(70.0) - 3.0)
                    .selected_text(text(palette_label(self.settings.palette)))
                    .show_ui(ui, |ui| {
                        let options = [PaletteKind::Automatic, PaletteKind::Limited(8), PaletteKind::FullTone, PaletteKind::Document(16)];
                        for choice in options {
                            let enabled = !matches!(choice, PaletteKind::Document(_)) || !session.document.swatches.is_empty();
                            if ui.add_enabled(enabled, egui::Button::selectable(
                                palette_label(choice) == palette_label(self.settings.palette), text(palette_label(choice))
                            )).clicked() && palette_label(choice) != palette_label(self.settings.palette) {
                                self.settings.palette = choice;
                            }
                        }
                    });
            });
            if let PaletteKind::Limited(count) | PaletteKind::Document(count) = &mut self.settings.palette {
                let mut amount = f32::from(*count);
                trace_slider(ui, "trace.palette.count", &mut amount, 2.0..=256.0, "", true);
                *count = amount.round() as u16;
            } else {
                let mut percent = f32::from(self.settings.color_detail);
                trace_slider(ui, "trace.palette.count", &mut percent, 0.0..=100.0, "%", true);
                self.settings.color_detail = percent.round() as u8;
            }
            if matches!(self.settings.palette, PaletteKind::Document(_)) {
                ui.weak(text("trace.document_swatches"));
                self.settings.library = session.document.swatches.iter().map(|swatch| {
                    let color = swatch.colour;
                    [color.r, color.g, color.b]
                }).take(256).collect();
            }
        }

        egui::CollapsingHeader::new(text("trace.advanced")).default_open(true).show(ui, |ui| {
            let mut paths = ((10.0 - self.settings.tolerance) / 9.9 * 100.0).clamp(0.0, 100.0);
            if trace_slider(ui, "trace.paths", &mut paths, 0.0..=100.0, "%", true) {
                self.settings.tolerance = 10.0 - paths * 0.099;
            }
            let mut corners = (180.0 - self.settings.corner_angle) * (100.0 / 179.0);
            if trace_slider(ui, "trace.corners", &mut corners, 0.0..=100.0, "%", true) {
                self.settings.corner_angle = 180.0 - corners * 1.79;
            }
            let mut noise = self.settings.min_area_px as f32;
            if trace_slider(ui, "trace.noise", &mut noise, 1.0..=100.0, " px", true) {
                self.settings.min_area_px = noise.round() as u32;
            }
            if self.settings.mode != TraceMode::FlatLogo {
                ui.horizontal_wrapped(|ui| {
                    ui.label(text("trace.method"));
                    dtool_design::with_icon_selection(ui, |ui| {
                        ui.selectable_value(&mut self.settings.method, TraceMethod::Abutting, text("trace.method.abutting"));
                        ui.selectable_value(&mut self.settings.method, TraceMethod::Overlapping, text("trace.method.overlapping"));
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(text("trace.create"));
                    ui.add_enabled_ui(!self.settings.fills || self.settings.strokes, |ui| {
                        dtool_design::toggle(ui, &mut self.settings.fills, text("trace.fills"));
                    });
                    ui.add_enabled_ui(!self.settings.strokes || self.settings.fills, |ui| {
                        dtool_design::toggle(ui, &mut self.settings.strokes, text("trace.strokes"));
                    });
                });
                trace_slider(ui, "trace.stroke_max", &mut self.settings.stroke_max_width, 1.0..=100.0,
                    " px", self.settings.strokes);
            }
            dtool_design::toggle(ui, &mut self.settings.snap_lines, text("trace.snap_lines"));
            if self.settings.mode != TraceMode::FlatLogo {
                dtool_design::toggle(ui, &mut self.settings.recognize_shapes, text("trace.shapes"));
                dtool_design::toggle(ui, &mut self.settings.auto_group, text("trace.auto_group"));
                ui.add_enabled_ui(matches!(self.settings.mode, TraceMode::Color | TraceMode::Grayscale)
                    && self.settings.fills && !self.settings.strokes, |ui| {
                    dtool_design::toggle(ui, &mut self.settings.gradients, text("trace.gradients"));
                });
            }
            ui.horizontal(|ui| {
                let mut ignore = self.settings.ignore_white || self.settings.ignore_color.is_some();
                if dtool_design::toggle(ui, &mut ignore, text("trace.ignore_color")).changed() {
                    self.settings.ignore_white = ignore;
                    if !ignore { self.settings.ignore_color = None; }
                }
                if ignore && self.settings.mode != TraceMode::FlatLogo {
                    let mut color = self.settings.ignore_color.unwrap_or([255; 3]);
                    if ui.color_edit_button_srgb(&mut color).changed() {
                        self.settings.ignore_white = color == [255; 3];
                        self.settings.ignore_color = (color != [255; 3]).then_some(color);
                    }
                }
            });
            ui.separator();
            let levels = [(720, "trace.detail.fast"), (1600, "trace.detail.standard"), (2400, "trace.detail.high")];
            trace_choice(ui, "trace.detail", &mut self.limit, &levels);
        });
    }

    fn show_presets(&mut self, ui: &mut Ui, session: &mut Session) {
        ui.horizontal(|ui| {
        ui.label(text("trace.preset"));
        egui::ComboBox::from_id_salt("trace.preset").width((ui.available_width() - 33.0).max(75.0))
            .selected_text(self.selected_preset.as_deref().unwrap_or_else(|| text("trace.custom")))
            .show_ui(ui, |ui| {
                for preset in &self.presets {
                    if ui.selectable_label(self.selected_preset.as_deref() == Some(&preset.name), &preset.name).clicked() {
                        self.selected_preset = Some(preset.name.clone());
                        self.preset_name = if preset.builtin { String::new() } else { preset.name.clone() };
                        self.settings = preset.settings.clone();
                    }
                }
            });
        let manage = dtool_design::tooltip(dtool_design::icon_button(ui, "⋯"), text("trace.preset.manage"));
        egui::Popup::menu(&manage).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.manage_presets(ui, session));
        });
    }

    fn manage_presets(&mut self, ui: &mut Ui, session: &mut Session) {
        let editable = self.presets.iter().find(|preset| self.selected_preset.as_deref() == Some(&preset.name))
            .filter(|preset| !preset.builtin).map(|preset| preset.name.clone());
        ui.add(egui::TextEdit::singleline(&mut self.preset_name).hint_text(text("trace.preset.name")));
        let name = self.preset_name.trim().to_string();
        let used = self.presets.iter().any(|preset| preset.name == name);
        ui.horizontal_wrapped(|ui| {
            let save = ui.add_enabled_ui(!name.is_empty() && (!used || editable.as_ref() == Some(&name)), |ui| {
                dtool_design::button(ui, text("trace.preset.save")).clicked()
            }).inner;
            let rename = ui.add_enabled_ui(editable.is_some() && !name.is_empty() && !used, |ui| {
                dtool_design::button(ui, text("trace.preset.rename")).clicked()
            }).inner;
            let delete = ui.add_enabled_ui(editable.is_some(), |ui| {
                dtool_design::button(ui, text("trace.preset.delete")).clicked()
            }).inner;
            let update = if save {
                Some(("trace.presets.save", serde_json::json!({"name": name, "settings": self.settings}), Some(name)))
            } else if rename {
                Some(("trace.presets.rename", serde_json::json!({"name": editable, "new_name": name}), Some(name)))
            } else if delete {
                Some(("trace.presets.delete", serde_json::json!({"name": editable}), None))
            } else {
                None
            };
            if let Some((command, params, selection)) = update {
                match session.run_named(command, params).and_then(|_| dtool_core::commands::trace_presets::presets()) {
                    Ok(presets) => {
                        self.presets = presets;
                        self.selected_preset = selection;
                        self.preset_name = self.selected_preset.clone().unwrap_or_default();
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error.message_key),
                }
            }
        });
    }

    fn update_preview(&mut self, ctx: &egui::Context) {
        let mut finished = false;
        if let Some(worker) = &self.worker {
            let current = worker.settings == self.settings && worker.limit == self.limit
                && !worker.cancel.load(Ordering::Relaxed);
            loop {
                match worker.events.try_recv() {
                    Ok(TraceEvent::Progress(percent, stage)) if current => self.progress = Some((percent, stage)),
                    Ok(TraceEvent::Progress(..)) => {}
                    Ok(TraceEvent::Finished(preview)) => {
                        if current {
                            self.progress = None;
                            self.preview = preview;
                            self.result_texture = self.preview.as_ref().map(|preview| {
                                ctx.load_texture("trace.result", preview.image.clone(), egui::TextureOptions::LINEAR)
                            });
                            if self.preview.is_none() {
                                self.error = Some("trace.preview.complex".into());
                                self.apply_when_ready = false;
                            }
                        }
                        finished = true;
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        ctx.request_repaint_after(Duration::from_millis(45));
                        return;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if current {
                            self.progress = None;
                            self.error = Some("error.trace_preview_missing_count".into());
                            self.apply_when_ready = false;
                        }
                        finished = true;
                        break;
                    }
                }
            }
        }
        if finished { self.worker = None; }
        let Some(deadline) = self.pending else { return };
        let now = Instant::now();
        if deadline > now {
            ctx.request_repaint_after(deadline - now);
            return;
        }
        let Some(source) = self.source.clone() else { return };
        self.pending = None;
        self.progress = Some((0, "trace.progress.prepare"));
        let (send, events) = mpsc::channel();
        let settings = self.settings.clone();
        let request = settings.clone();
        let limit = self.limit;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let mut report = |percent, stage| {
                !worker_cancel.load(Ordering::Relaxed) && send.send(TraceEvent::Progress(percent, stage)).is_ok()
            };
            let preview = trace_preview(&source, limit, &request, &mut report);
            let _ = send.send(TraceEvent::Finished(preview));
            repaint.request_repaint();
        });
        self.worker = Some(Worker { settings, limit, cancel, events });
        ctx.request_repaint_after(Duration::from_millis(45));
    }
}

fn trace_choice<T: Copy + PartialEq>(ui: &mut Ui, key: &str, value: &mut T, choices: &[(T, &str)]) {
    ui.horizontal(|ui| {
        ui.label(text(key));
        egui::ComboBox::from_id_salt(key).width((ui.available_width() - 4.0).max(64.0))
            .selected_text(choices.iter().find(|(item, _)| item == value).map_or("", |(_, label)| text(label)))
            .show_ui(ui, |ui| {
                for (item, label) in choices {
                    ui.selectable_value(value, *item, text(label));
                }
            });
    });
}

fn trace_slider(ui: &mut Ui, key: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str, enabled: bool) -> bool {
    ui.horizontal(|ui| {
        let label = text(key);
        let label_width = (ui.available_width() * 0.27).clamp(50.0, 122.0);
        let caption = ui.add_sized([label_width, ui.spacing().interact_size.y], egui::Label::new(label).truncate());
        let width = (ui.available_width() - 66.0).max(26.0);
        let rail = ui.scope(|ui| {
            ui.visuals_mut().selection.bg_fill = Color32::from_rgb(220, 107, 89);
            dtool_design::slider_rail(ui, value, range.clone(), width, label, enabled)
        }).inner;
        let field = ui.add_enabled(enabled, egui::DragValue::new(value)
            .range(range).speed(0.5).fixed_decimals(0).suffix(suffix));
        (rail | field.labelled_by(caption.id)).changed()
    }).inner
}

fn trace_preview(
    source: &Source, limit: usize, settings: &TraceSettings, report: &mut impl FnMut(u32, &'static str) -> bool,
) -> Option<Preview> {
    use dtool_core::paint::{Colour, Paint};
    use dtool_core::scene::{Prim, Scene, rasterize_scene};
    let [width, height] = source.size;
    let size = fitted_size(source.size, limit);
    let rgba = if size == source.size {
        std::borrow::Cow::Borrowed(source.rgba.as_slice())
    } else {
        std::borrow::Cow::Owned(sample_image(source, size))
    };
    let [w, h] = size.map(|side| side as u32);
    let mut paths = dtool_core::trace::trace_watched(&RasterView::new(w, h, &rgba), settings, &mut *report)?;
    let anchors = paths.iter().map(|path| path.path.elements().len()).sum();
    if anchors > 220_000 || paths.len() > 12_000 { return None; }
    if !report(98, "trace.progress.preview") { return None; }

    let mut colors = std::collections::HashSet::new();
    let mut scene = Scene::default();
    let mut contours = Vec::new();
    let paint = |[r, g, b, a]: [u8; 4]| Paint::Solid { colour: Colour::new(r, g, b, a) };
    for (index, path) in paths.iter_mut().enumerate() {
        if index % 256 == 0 && !report(98, "trace.progress.preview") { return None; }
        let subs = dtool_core::outline::flatten_subpaths(&path.path, (w.max(h) as f64 / 768.0).max(0.25));
        let fill = path.fill_paint([1.0; 2]);
        colors.extend(path.colors());
        if !matches!(fill, Paint::None) {
            scene.prims.push(Prim::Fill { object: ObjectId(0), origin: [0.0; 2], paint: fill, opacity: 1.0,
                contours: subs.iter().filter(|(_, closed)| *closed)
                    .map(|(points, _)| points.iter().map(|[x, y]| [*x as f32, *y as f32]).collect()).collect() });
        }
        if let Some((stroke, width)) = path.stroke {
            scene.prims.extend(subs.iter().map(|(points, closed)| Prim::Stroke {
                object: ObjectId(0), points: points.iter().map(|[x, y]| [*x as f32, *y as f32]).collect(),
                closed: *closed, width, paint: paint(stroke), opacity: 1.0,
            }));
        }
        for (points, closed) in subs.into_iter().filter(|(points, _)| points.len() >= 2) {
            let mut points: Vec<_> = points.into_iter().map(|[x, y]| [(x / f64::from(w)) as f32, (y / f64::from(h)) as f32]).collect();
            if closed { points.push(points[0]); }
            contours.push(points);
        }
    }
    let preview_size = fitted_size([width, height], 360);
    let doc = dtool_core::Document::new([w as f32, h as f32], dtool_core::document::Units::Px, 96.0, 1);
    let pixels = rasterize_scene(&doc, &scene, [0.0, 0.0, w as f32, h as f32],
        preview_size[0] as u32, preview_size[1] as u32, None);
    Some(Preview { paths, size: [w, h], contours, anchors, colors: colors.len(),
        image: egui::ColorImage::from_rgba_unmultiplied(preview_size, &pixels) })
}

fn fitted_size([width, height]: [usize; 2], limit: usize) -> [usize; 2] {
    let scale = (limit as f64 / width.max(height) as f64).min(1.0);
    [width, height].map(|side| (side as f64 * scale).round().max(1.0) as usize)
}

fn thumbnail(source: &Source) -> egui::ColorImage {
    let size = fitted_size(source.size, 512);
    egui::ColorImage::from_rgba_unmultiplied(size, &sample_image(source, size))
}

fn sample_image(source: &Source, [w, h]: [usize; 2]) -> Vec<u8> {
    let [width, height] = source.size;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let i = (y * height / h * width + x * width / w) * 4;
            rgba.extend_from_slice(&source.rgba[i..i + 4]);
        }
    }
    rgba
}
