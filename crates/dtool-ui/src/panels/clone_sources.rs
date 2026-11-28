use dtool_core::commands::pixel_brush::global_source::{ADD, ANCHOR, REMOVE, SELECT, Source};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use dtool_strings::text;
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui, vec2};
use serde_json::json;

pub const PANEL_KEY: &str = "clone_sources";
pub const WINDOW_PANEL: &str = "window.pixel.sources";

#[derive(Default)]
pub struct Panel {
    show_preview: bool,
    texture: Option<(u64, TextureHandle)>,
    error: Option<String>,
}

impl Panel {
    pub fn show(&mut self, ui: &mut Ui, workspace: &mut Workspace) {
        let Some(session) = workspace.session() else { return };
        let entries = session.clone_sources.entries();
        let selected = session.clone_sources.selected();
        let add_enabled = session.can_execute_named(ADD);
        let active = selected.as_ref().map(|item| item.id);
        let mut action = None;

        ui.horizontal_wrapped(|ui| {
            if dtool_design::button_with(ui, text("clone.sources.add"),
                &dtool_design::ControlState::enabled(add_enabled)).clicked() {
                action = Some(Command::with_params(ADD, json!({})));
            }
            if dtool_design::button_with(ui, text("clone.sources.remove"),
                &dtool_design::ControlState::enabled(active.is_some())).clicked()
                && let Some(id) = active {
                action = Some(Command::with_params(REMOVE, json!({"id": id})));
            }
        });
        ui.add_enabled_ui(active.is_some(), |ui| {
            dtool_design::toggle(ui, &mut self.show_preview, text("clone.sources.preview"));
        });
        ui.separator();
        if entries.is_empty() {
            self.texture = None;
            ui.weak(text("clone.sources.empty"));
        } else {
            egui::ScrollArea::vertical().id_salt("clone_sources.list")
                .max_height((ui.available_height() * 0.45).clamp(46.0, 210.0)).show(ui, |ui| {
                    for item in &entries {
                        let label = if item.name.is_empty() {
                            format!("{} {}", text("panel.clone_sources"), item.id)
                        } else {
                            format!("{} · {}", item.id, item.name)
                        };
                        let width = ui.available_width();
                        let row = dtool_design::with_icon_selection(ui, |ui| {
                            ui.add(egui::Button::selectable(active == Some(item.id), &label)
                                .min_size(vec2(width, dtool_design::size("control.hit_size"))).truncate())
                        });
                        if row.on_hover_text(&label).clicked() {
                            action = Some(Command::with_params(SELECT, json!({"id": item.id})));
                        }
                    }
                });
            if self.show_preview && let Some(source) = &selected {
                ui.separator();
                if let Some(point) = self.preview(ui, source) {
                    action = Some(Command::with_params(ANCHOR, json!({"id": source.id, "point": point})));
                }
            } else {
                self.texture = None;
            }
        }
        if let Some(action) = action {
            self.error = workspace.run(Scope::active(), action).1.err().map(|error| error.message_key);
        }
        if let Some(key) = &self.error {
            ui.label(egui::RichText::new(text(key)).color(ui.visuals().error_fg_color));
        }
    }

    fn preview(&mut self, ui: &mut Ui, source: &Source) -> Option<[f32; 2]> {
        if self.texture.as_ref().is_none_or(|(id, _)| *id != source.id) {
            let (size, rgba) = source.preview()?;
            self.texture = Some((source.id, ui.ctx().load_texture(
                "clone.source.preview", ColorImage::from_rgba_unmultiplied(size, &rgba), TextureOptions::LINEAR,
            )));
        }
        let texture = &self.texture.as_ref()?.1;
        let available = vec2(ui.available_width(), ui.available_height().clamp(30.0, 220.0));
        if available.x <= 0.0 { return None; }
        let image_size = texture.size_vec2();
        let scale = (available.x / image_size.x).min(available.y / image_size.y);
        let preview = ui.allocate_response(image_size * scale, egui::Sense::click())
            .on_hover_text(text("clone.sources.pick_preview")).on_hover_cursor(egui::CursorIcon::Crosshair);
        let rect = preview.rect;
        let [x, y, w, h] = source.bounds;
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, dtool_design::theme::surface(ui.ctx(), "dialog.section"));
        painter.image(texture.id(), rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        let fraction = vec2((source.point[0] - x) / w, (source.point[1] - y) / h);
        let point = rect.min + fraction * rect.size();
        for (width, ink) in [(3.0, Color32::BLACK), (1.0, Color32::WHITE)] {
            let stroke = egui::Stroke::new(width, ink);
            painter.hline(point.x - 7.0..=point.x + 7.0, point.y, stroke);
            painter.vline(point.x, point.y - 7.0..=point.y + 7.0, stroke);
        }
        let pos = preview.clicked().then(|| preview.interact_pointer_pos()).flatten()?;
        let fraction = (pos - rect.min) / rect.size();
        Some([x + fraction.x.clamp(0.0, 0.999_999) * w, y + fraction.y.clamp(0.0, 0.999_999) * h])
    }
}
