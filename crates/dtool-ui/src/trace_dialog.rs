use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use dtool_core::{ObjectId, Session};
use dtool_core::raster_view::RasterView;
use dtool_core::trace::{PaletteKind, TraceMethod, TraceMode, TracePreset, TraceSettings};
use dtool_strings::text;
use egui::{Color32, Pos2, Stroke, TextureHandle, Ui, vec2};

use crate::dialog_chrome::{ClosePolicy, DialogMode, DialogResult};

const PREVIEW_DELAY: Duration = Duration::from_millis(150);

struct Source {
    size: [usize; 2],
    rgba: Vec<u8>,
}

struct Preview {
    count: usize,
    contours: Vec<Vec<[f32; 2]>>,
}

#[derive(Default)]
pub struct TraceDialog {
    node: Option<ObjectId>,
    source: Option<Arc<Source>>,
    texture: Option<TextureHandle>,
    chrome: crate::dialog_chrome::DialogChrome,
    settings: TraceSettings,
    preview: Option<Preview>,
    pending: Option<Instant>,
    worker: Option<(TraceSettings, mpsc::Receiver<Preview>)>,
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
        if session.document.get_object(node).is_none() {
            self.close();
            return false;
        }

        let ctx = ui.ctx();
        if self.texture.is_none() && let Some(source) = &self.source {
            self.texture = Some(ctx.load_texture("trace.source", thumbnail(source), egui::TextureOptions::LINEAR));
        }
        let old_settings = self.settings.clone();
        let mut chrome = std::mem::take(&mut self.chrome);
        let (result, (apply, cancel)) = chrome.show(ctx, "dtool.dialog.image_trace", text("trace.title"),
            DialogMode::Modal, ClosePolicy::CancelOnly, false, |ui| {
                ui.horizontal_top(|ui| {
                    self.show_preview(ui);
                    ui.vertical(|ui| self.show_settings(ui, session));
                });
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, text(error));
                }
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let apply = ui.add_enabled_ui(self.settings.fills || self.settings.strokes, |ui| {
                        dtool_design::button(ui, text("dialog.ok")).clicked()
                    }).inner;
                    let cancel = dtool_design::button(ui, text("dialog.cancel")).clicked();
                    (apply, cancel)
                }).inner
            });
        self.chrome = chrome;

        if cancel || result == DialogResult::Cancel {
            self.close();
            return false;
        }
        if apply {
            match session.run_named("image.trace", serde_json::json!({ "node": node.0, "settings": self.settings })) {
                Ok(_) => { self.close(); return true; }
                Err(error) => self.error = Some(error.message_key),
            }
        }
        if self.settings != old_settings {
            self.preview = None;
            self.pending = Some(Instant::now() + PREVIEW_DELAY);
            ctx.request_repaint_after(PREVIEW_DELAY);
        }
        self.update_preview(ctx);
        false
    }

    fn show_preview(&self, ui: &mut Ui) {
        ui.vertical(|ui| {
            let Some(texture) = &self.texture else { return };
            let image_size = texture.size_vec2();
            let scale = (320.0 / image_size.x).min(290.0 / image_size.y);
            let (rect, _) = ui.allocate_exact_size(image_size * scale, egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_gray(215));
            painter.image(texture.id(), rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            if let Some(preview) = &self.preview {
                for contour in &preview.contours {
                    let points = contour.iter().map(|[x, y]| rect.min + vec2(x * rect.width(), y * rect.height())).collect::<Vec<_>>();
                    painter.add(egui::Shape::line(points.clone(), Stroke::new(2.5, Color32::from_black_alpha(190))));
                    painter.add(egui::Shape::line(points, Stroke::new(1.0, Color32::from_rgb(255, 188, 71))));
                }
                ui.label(dtool_strings::format("trace.preview.paths", &[("count", &preview.count.to_string())]));
            } else {
                ui.label(text("trace.preview.pending"));
            }
        });
    }

    fn show_settings(&mut self, ui: &mut Ui, session: &mut Session) {
        ui.set_min_width(280.0);
        ui.horizontal(|ui| {
            ui.label(text("trace.mode"));
            for (mode, label) in [(TraceMode::BlackWhite, "trace.mode.bw"), (TraceMode::Grayscale, "trace.mode.grayscale"), (TraceMode::Color, "trace.mode.color")] {
                ui.selectable_value(&mut self.settings.mode, mode, text(label));
            }
        });
        if self.settings.mode == TraceMode::BlackWhite || !self.settings.fills {
            let mut percent = f32::from(self.settings.threshold) / 255.0 * 100.0;
            if ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).fixed_decimals(0).suffix("%").text(text("trace.threshold"))).changed() {
                self.settings.threshold = (percent / 100.0 * 255.0).round() as u8;
            }
        }
        ui.add(egui::Slider::new(&mut self.settings.tolerance, 0.1..=10.0).suffix(" px").text(text("trace.tolerance")));
        ui.horizontal(|ui| {
            dtool_design::toggle(ui, &mut self.settings.fills, text("trace.fills"));
            dtool_design::toggle(ui, &mut self.settings.strokes, text("trace.strokes"));
        });
        if self.settings.strokes {
            ui.add(egui::Slider::new(&mut self.settings.stroke_max_width, 1.0..=50.0).suffix(" px").text(text("trace.stroke_max")));
        }
        dtool_design::toggle(ui, &mut self.settings.ignore_white, text("trace.ignore_white"));

        if self.settings.mode != TraceMode::BlackWhite && self.settings.fills {
            let palette_key = |palette| match palette {
                PaletteKind::Automatic => "trace.palette.auto",
                PaletteKind::Limited(_) => "trace.palette.limited",
                PaletteKind::FullTone => "trace.palette.full",
            };
            egui::ComboBox::from_label(text("trace.palette"))
                .selected_text(text(palette_key(self.settings.palette)))
                .show_ui(ui, |ui| {
                    for choice in [PaletteKind::Automatic, PaletteKind::Limited(8), PaletteKind::FullTone] {
                        if ui.selectable_label(palette_key(choice) == palette_key(self.settings.palette), text(palette_key(choice))).clicked()
                            && palette_key(choice) != palette_key(self.settings.palette)
                        {
                            self.settings.palette = choice;
                        }
                    }
                });
            if let PaletteKind::Limited(count) = &mut self.settings.palette {
                ui.add(egui::Slider::new(count, 2..=30).text(text("trace.palette.count")));
            }
        }
        if self.settings.mode == TraceMode::Color && self.settings.fills {
            ui.horizontal(|ui| {
                ui.label(text("trace.method"));
                ui.selectable_value(&mut self.settings.method, TraceMethod::Abutting, text("trace.method.abutting"));
                ui.selectable_value(&mut self.settings.method, TraceMethod::Overlapping, text("trace.method.overlapping"));
            });
        }
        ui.collapsing(text("trace.advanced"), |ui| self.show_presets(ui, session));
    }

    fn show_presets(&mut self, ui: &mut Ui, session: &mut Session) {
        egui::ComboBox::from_id_salt("trace.preset")
            .selected_text(self.selected_preset.as_deref().unwrap_or_else(|| text("trace.preset")))
            .show_ui(ui, |ui| {
                for preset in &self.presets {
                    if ui.selectable_label(self.selected_preset.as_deref() == Some(&preset.name), &preset.name).clicked() {
                        self.selected_preset = Some(preset.name.clone());
                        self.preset_name = if preset.builtin { String::new() } else { preset.name.clone() };
                        self.settings = preset.settings.clone();
                    }
                }
            });
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
        if let Some((settings, receiver)) = &self.worker {
            match receiver.try_recv() {
                Ok(preview) => {
                    if *settings == self.settings {
                        self.preview = Some(preview);
                        self.pending = None;
                        ctx.request_repaint();
                    }
                    self.worker = None;
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.error = Some("error.trace_preview_missing_count".into());
                }
            }
        }
        let Some(deadline) = self.pending else { return };
        let now = Instant::now();
        if deadline > now {
            ctx.request_repaint_after(deadline - now);
            return;
        }
        let Some(source) = self.source.clone() else { return };
        self.pending = None;
        let (send, receive) = mpsc::channel();
        let settings = self.settings.clone();
        let repaint = ctx.clone();
        let request = settings.clone();
        std::thread::spawn(move || {
            let [width, height] = source.size;
            let view = RasterView::new(width as u32, height as u32, &source.rgba);
            let paths = dtool_core::trace::trace_image(&view, &request);
            let contours = paths.iter().flat_map(|path| dtool_core::outline::flatten_subpaths(&path.path, 0.35))
                .filter(|(points, _)| points.len() >= 2)
                .map(|(points, closed)| {
                    let mut points: Vec<_> = points.into_iter().map(|[x, y]| [(x / width as f64) as f32, (y / height as f64) as f32]).collect();
                    if closed { points.push(points[0]); }
                    points
                }).collect();
            let _ = send.send(Preview { count: paths.len(), contours });
            repaint.request_repaint();
        });
        self.worker = Some((settings, receive));
    }
}

fn thumbnail(source: &Source) -> egui::ColorImage {
    let [width, height] = source.size;
    let scale = (512.0 / width.max(height) as f32).min(1.0);
    let [w, h] = [width, height].map(|v| (v as f32 * scale).round().max(1.0) as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let i = (y * height / h * width + x * width / w) * 4;
            rgba.extend_from_slice(&source.rgba[i..i + 4]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}
