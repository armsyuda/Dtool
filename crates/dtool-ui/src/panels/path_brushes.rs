use dtool_core::commands::brush::{
    APPLY_SHARED_PRESET, Preset, SharedPreset, active_preset_is_shared, active_preset_name, presets,
};
use dtool_core::path_brush::{Ink, TextureMode};
use dtool_core::resources::ImageResource;
use dtool_core::{Command, Document, Object, ObjectKind, Scope, workspace::{ContextId, Workspace}};
use dtool_strings::text;
use egui::{Color32, Rect, TextureHandle, Ui};
use serde_json::json;
use std::collections::HashMap;

pub const PANEL_KEY: &str = "path_brushes";
pub const WINDOW_PANEL: &str = "window.vector.path_brushes";

#[derive(Default)]
pub struct PathBrushesPanel {
    document: Option<ContextId>,
    version: u64,
    thumbnails: HashMap<String, Option<Thumbnail>>,
    search: String,
    name: String,
    filter: Option<Option<TextureMode>>,
    error: Option<String>,
    shared: bool,
    library: Vec<SharedPreset>,
}

enum Action {
    Save { name: String, replace: bool },
    Apply(String),
    Remove(String),
}

fn library_path() -> std::path::PathBuf {
    crate::settings::UiSettings::default_path().with_file_name("path_brush_presets.json")
}

fn read_library() -> std::io::Result<Vec<SharedPreset>> {
    match std::fs::read(library_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn write_library(presets: &[SharedPreset]) -> std::io::Result<()> {
    let path = library_path();
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec(presets).map_err(std::io::Error::other)?;
    dtool_export::write_atomic(&path, &data)
}

struct Thumbnail {
    texture: TextureHandle,
    bounds: [f32; 4],
}

const SAMPLE: [[f64; 2]; 5] = [[8., 18.], [44., 12.], [82., 17.], [119., 21.], [152., 14.]];

impl PathBrushesPanel {
    pub fn show(&mut self, ui: &mut Ui, workspace: &mut Workspace) {
        let (Some(context), Some(session)) = (workspace.active(), workspace.session()) else {
            ui.weak(text("error.no_document"));
            return;
        };
        let doc = &session.document;
        if self.document != Some(context) {
            self.document = Some(context);
            self.search.clear();
            self.name.clear();
            self.error = None;
            self.thumbnails.clear();
        }
        if self.version != doc.version {
            self.version = doc.version;
            self.thumbnails.clear();
        }
        let previous_scope = self.shared;
        dtool_design::panel_columns(ui, 2, dtool_design::size("button.width"), |ui, index| {
            let (shared, label) = [(false, "path_brushes.document"), (true, "path_brushes.shared")][index];
            dtool_design::choice_value(ui, &mut self.shared, shared, text(label));
        });
        if previous_scope != self.shared {
            self.thumbnails.clear();
            self.error = None;
            if self.shared {
                match read_library() {
                    Ok(presets) => self.library = presets,
                    Err(_) => {
                        self.library.clear();
                        self.error = Some("path_brushes.library_read_error".into());
                    }
                }
            }
        }
        ui.add(egui::TextEdit::singleline(&mut self.search)
            .desired_width(ui.available_width()).hint_text(text("path_brushes.search")));
        let modes = [
            (None, "path_brushes.all"),
            (Some(None), "context.brush.tip.solid"),
            (Some(Some(TextureMode::Intensity)), "context.brush.tip.intensity"),
            (Some(Some(TextureMode::Image)), "context.brush.tip.image"),
        ];
        let current_label = modes.iter().find(|(mode, _)| *mode == self.filter).map(|(_, label)| *label).unwrap_or("path_brushes.all");
        let filter = egui::ComboBox::from_id_salt("path-brush-type").selected_text(text(current_label))
            .width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for (mode, label) in modes {
                    ui.selectable_value(&mut self.filter, mode, text(label));
                }
            });
        dtool_design::tooltip(filter.response, text(current_label));
        let mut action = None;
        dtool_design::panel_columns(ui, 2, dtool_design::size("button.width"), |ui, index| {
            if index == 0 {
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(ui.available_width())
                    .hint_text(text("path_brushes.name")));
            } else {
                let state = dtool_design::ControlState::enabled(!self.name.trim().is_empty());
                let save = dtool_design::button_with(ui, text("path_brushes.save"), &state)
                    .on_hover_text(text("path_brushes.save_hint"));
                if save.clicked() {
                    action = Some(Action::Save { name: self.name.clone(), replace: false });
                }
            }
        });
        let needle = self.search.trim().to_lowercase();
        let active = (active_preset_is_shared(doc) == self.shared).then(|| active_preset_name(doc)).flatten();
        let document_presets: Vec<_> = if self.shared { Vec::new() } else { presets(doc).collect() };
        let rows: Vec<_> = if self.shared {
            self.library.iter().map(|entry| (&entry.preset, entry.image.as_ref())).collect()
        } else {
            document_presets.iter().map(|preset| (preset, None)).collect()
        };
        let rows: Vec<_> = rows.into_iter().filter(|(preset, _)| {
            (needle.is_empty() || preset.name.to_lowercase().contains(&needle))
                && self.filter.is_none_or(|mode| preset.texture.as_ref().map(|tex| tex.mode) == mode)
        }).collect();
        if let Some(error) = &self.error {
            ui.weak(text(error));
        }
        if rows.is_empty() {
            ui.weak(text("path_brushes.empty"));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 51.0, rows.len(), |ui, range| {
            for &(preset, image) in &rows[range] {
                let response = dtool_design::preview_list_row(ui, egui::vec2(ui.available_width(), 51.0),
                    active == Some(&preset.name), &preset.name, format!("{} pt · {}", preset.stroke.width, preset.name),
                    |ui, sample| Self::preview(&mut self.thumbnails, ui, doc, preset, image, sample));
                response.context_menu(|ui| {
                    if ui.button(text("path_brushes.update")).clicked() {
                        action = Some(Action::Save { name: preset.name.clone(), replace: true });
                        ui.close();
                    }
                    if ui.button(text("button.delete")).clicked() {
                        action = Some(Action::Remove(preset.name.clone()));
                        ui.close();
                    }
                });
                if response.clicked() {
                    action = Some(Action::Apply(preset.name.clone()));
                }
            }
        });
        if let Some(action) = action {
            let clear_name = matches!(&action, Action::Save { replace: false, .. });
            match self.act(workspace, context, action) {
                Ok(_) => {
                    self.error = None;
                    if clear_name { self.name.clear(); }
                }
                Err(error) => self.error = Some(error.message_key),
            }
        }
    }

    fn act(&mut self, workspace: &mut Workspace, context: ContextId, action: Action) -> Result<(), dtool_core::CommandError> {
        let command = match (self.shared, action) {
            (false, Action::Save { name, replace }) =>
                Command::with_params("brush.vector.preset.save", json!({"name": name, "replace": replace})),
            (false, Action::Apply(name)) =>
                Command::with_params("brush.vector.preset.apply", json!({"name": name})),
            (false, Action::Remove(name)) =>
                Command::with_params("brush.vector.preset.remove", json!({"name": name})),
            (true, Action::Apply(name)) => {
                let preset = self.library.iter().find(|entry| entry.preset.name == name)
                    .ok_or_else(dtool_core::CommandError::not_found)?;
                Command::with_params(APPLY_SHARED_PRESET, json!({"preset": preset}))
            }
            (true, action) => {
                let mut library = read_library().map_err(|_| library_error("path_brushes.library_read_error"))?;
                match action {
                    Action::Save { name, replace } => {
                        let session = workspace.session().ok_or_else(dtool_core::CommandError::no_document)?;
                        let preset = SharedPreset::capture(session, &name)?;
                        if let Some(entry) = library.iter_mut().find(|entry| entry.preset.name == preset.preset.name) {
                            if !replace { return Err(library_error("path_brushes.library_name_exists")); }
                            *entry = preset;
                        } else {
                            library.push(preset);
                        }
                    }
                    Action::Remove(name) => library.retain(|entry| entry.preset.name != name),
                    Action::Apply(_) => unreachable!("apply is handled above"),
                }
                write_library(&library).map_err(|_| library_error("path_brushes.library_write_error"))?;
                self.library = library;
                self.thumbnails.clear();
                return Ok(());
            }
        };
        workspace.run(Scope::of(context), command).1.map(|_| ())
    }

    fn preview(
        thumbnails: &mut HashMap<String, Option<Thumbnail>>, ui: &Ui, doc: &Document,
        preset: &Preset, image: Option<&ImageResource>, area: Rect,
    ) {
        let painter = ui.painter().with_clip_rect(area.intersect(ui.clip_rect()));
        painter.rect_filled(area, 2.0, Color32::from_gray(160));
        let point = |x: f32, y: f32| area.left_top() + egui::vec2(x / 160.0 * area.width(), y / 32.0 * area.height());
        let width = preset.stroke.width.clamp(2.0, 14.0);
        if let Some(texture) = &preset.texture {
            let thumbnail = thumbnails.entry(preset.name.clone()).or_insert_with(|| {
                let mut stroke = preset.stroke.clone();
                stroke.width = width;
                let curve = dtool_core::curve::Curve::from_contour_and_holes(
                    dtool_core::curve::Contour { points: dtool_core::curve::Contour::smart_handles(&SAMPLE), closed: false }, Vec::new(),
                );
                let mut object = Object {
                    kind: ObjectKind::Curve(curve), size: [160.0, 32.0], stroke,
                    path_brush: Some(texture.clone()), ..Object::default()
                };
                let mut source;
                let doc = if let Some(image) = image {
                    source = Document::new([160.0, 32.0], dtool_core::document::Units::Pt, 72.0, 1);
                    let id = source.add_image_resource(image.clone());
                    object.path_brush.as_mut()?.resource_id = id.0.to_string();
                    &source
                } else {
                    doc
                };
                let ink = Ink::new(doc, &object)?;
                let size = ink.pixel_size(1.5);
                let pixels = ink.render(size);
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                Some(Thumbnail {
                    texture: ui.ctx().load_texture(("path-brush-".to_owned() + &preset.name).as_str(), image, egui::TextureOptions::LINEAR),
                    bounds: ink.bounds,
                })
            });
            if let Some(thumbnail) = thumbnail {
                let [x, y, w, h] = thumbnail.bounds;
                painter.image(thumbnail.texture.id(), Rect::from_min_max(point(x, y), point(x + w, y + h)),
                    Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE.gamma_multiply(preset.opacity));
                return;
            }
        }
        if let Some(ink) = dtool_core::scene::representative_colour(&preset.stroke.paint) {
            let stroke = egui::Stroke::new(width, Color32::from_rgba_unmultiplied(ink.r, ink.g, ink.b, ink.a).gamma_multiply(preset.opacity));
            painter.add(egui::Shape::line(SAMPLE.iter().map(|&[x,y]| point(x as f32, y as f32)).collect(), stroke));
        }
    }
}

fn library_error(key: &str) -> dtool_core::CommandError {
    dtool_core::CommandError::validation(key)
}
