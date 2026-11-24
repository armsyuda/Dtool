use dtool_core::brush::BrushTip;
use std::collections::{HashMap, HashSet};
use dtool_core::commands::pixel_presets::{self, APPLY};
use dtool_core::workspace::Workspace;
use dtool_core::{Command, Scope};
use dtool_strings::text;
use egui::{Color32, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::{json, Value};

pub const PANEL_KEY: &str = "brushes";
pub const WINDOW_PANEL: &str = "window.pixel.brushes";

#[derive(Default)]
pub struct Panel {
    category: String,
    search: String,
    grid: bool,
    form: Option<Form>,
    message: Option<String>,
    import_path: Option<std::path::PathBuf>,
    import_count: Option<(u64, u64, u64, u64)>,
    pub(crate) edit_target: Option<String>,
    previews: HashMap<String, (BrushTip, egui::TextureHandle)>,
}

struct Form {
    title: &'static str,
    verb: &'static str,
    field: Option<&'static str>,
    input: String,
    params: Value,
}

impl Form {
    fn input(title: &'static str, verb: &'static str, field: &'static str, input: String, params: Value) -> Self {
        Self { title, verb, field: Some(field), input, params }
    }

    fn delete(title: &'static str, verb: &'static str, params: Value) -> Self {
        Self { title, verb, field: None, input: String::new(), params }
    }
}

impl Panel {
    pub fn show(&mut self, ui: &mut Ui, workspace: &mut Workspace) {
        let library = match pixel_presets::library() {
            Ok(library) => library,
            Err(error) => { ui.weak(text(&error.message_key)); return; }
        };
        if !library.categories.contains(&self.category) {
            self.category = library.categories.first().cloned().unwrap_or_default();
        }
        let active = workspace.session().and_then(|session| session.tool_options.raster_tip(&session.active_tool))
            .map(|tip| tip.id.clone());
        let available = workspace.session().is_some_and(|session| session.has_document);
        let from_selection = workspace.session().is_some_and(|session| session.can_execute_named("brush.presets.from_selection"));
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(text("path_brushes.search")).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("raster-brush-category")
                .width((ui.available_width() - 66.0).max(44.0))
                .selected_text(category_label(&self.category))
                .show_ui(ui, |ui| {
                    for name in &library.categories {
                        ui.selectable_value(&mut self.category, name.clone(), category_label(name));
                    }
                });
            let toggle_label = text(if self.grid { "pixel_brushes.list" } else { "pixel_brushes.grid" });
            let toggle = dtool_design::tooltip(ui.small_button(if self.grid { "☰" } else { "▦" }), toggle_label);
            toggle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), toggle_label));
            if toggle.clicked() { self.grid = !self.grid; }
            let menu = ui.menu_button("⋯", |ui| self.menu(ui, &library.tips, from_selection));
            let label = text("pixel_brushes.options");
            menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
            dtool_design::tooltip(menu.response, label);
        });
        if let Some(message) = &self.message { ui.weak(text(message)); }
        if let Some((added, updated, skipped, limited)) = self.import_count {
            let label = text("pixel_brushes.import_result").replace("{added}", &added.to_string())
                .replace("{skipped}", &skipped.to_string());
            ui.weak(label);
            if updated > 0 {
                ui.weak(text("pixel_brushes.import_updated").replace("{count}", &updated.to_string()));
            }
            if limited > 0 {
                ui.weak(text("pixel_brushes.import_partial").replace("{count}", &limited.to_string()));
            }
        }
        let search = self.search.trim().to_lowercase();
        let rows: Vec<_> = library.tips.iter().filter(|tip| tip.category == self.category
            && (search.is_empty() || tip_label(tip).to_lowercase().contains(&search))).collect();
        let mut picked = None;
        let mut visible_previews = HashSet::new();
        let columns = if self.grid { (ui.available_width() / 78.0).floor().max(1.0) as usize } else { 1 };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 48.0, rows.len().div_ceil(columns), |ui, range| {
            for index in range {
                let page = &rows[index * columns..((index + 1) * columns).min(rows.len())];
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - ui.spacing().item_spacing.x * (columns - 1) as f32) / columns as f32;
                    for &tip in page {
                        visible_previews.insert(tip.id.as_str());
                        let preview = self.previews.entry(tip.id.clone()).or_insert_with(|| {
                            let image = crate::brush_editor::preview(tip);
                            (tip.clone(), ui.ctx().load_texture(format!("pixel-brush.preview.{}", tip.id), image, egui::TextureOptions::LINEAR))
                        });
                        if &preview.0 != tip {
                            preview.0 = tip.clone();
                            preview.1.set(crate::brush_editor::preview(tip), egui::TextureOptions::LINEAR);
                        }
                        let response = row(ui, tip, &preview.1, width, active.as_deref() == Some(&tip.id));
                        if available && response.double_clicked() {
                            self.edit_target = Some(tip.id.clone());
                        } else if available && response.clicked() {
                            picked = Some(tip.id.clone());
                        }
                        response.context_menu(|ui| {
                            let builtin = pixel_presets::builtin(&tip.id);
                            if ui.add_enabled(available, egui::Button::new(text("brush_editor.edit"))).clicked() {
                                self.edit_target = Some(tip.id.clone());
                                ui.close();
                            }
                            if ui.add_enabled(!builtin, egui::Button::new(text("pixel_brushes.rename"))).clicked() {
                                self.form = Some(Form::input("pixel_brushes.rename", "rename", "name", tip.name.clone(), json!({"id":tip.id})));
                                ui.close();
                            }
                            if ui.button(text("pixel_brushes.duplicate")).clicked() {
                                self.form = Some(Form::input("pixel_brushes.duplicate", "duplicate", "name", tip_label(tip).into(), json!({"id":tip.id})));
                                ui.close();
                            }
                            if ui.add_enabled(!builtin, egui::Button::new(text("pixel_brushes.delete"))).clicked() {
                                self.form = Some(Form::delete("pixel_brushes.delete", "delete", json!({"id":tip.id})));
                                ui.close();
                            }
                        });
                    }
                });
            }
        });
        self.previews.retain(|id, _| visible_previews.contains(id.as_str()));
        if let Some(id) = picked {
            self.message = workspace.run(Scope::active(), Command::with_params(APPLY, json!({"id":id}))).1.err().map(|error| error.message_key);
        }
        if let Some(path) = self.import_path.take() {
            self.run_form(workspace, "import", "", json!({"path": path}));
        }
        self.show_form(ui, workspace);
    }

    fn menu(&mut self, ui: &mut Ui, tips: &[BrushTip], from_selection: bool) {
        let choices = [
            ("pixel_brushes.new_round", "new", "name", json!({"category":self.category,"kind":"round"})),
            ("pixel_brushes.new_square", "new", "name", json!({"category":self.category,"kind":"square"})),
            ("pixel_brushes.new_image", "new", "path", json!({"category":self.category,"kind":"image"})),
            ("pixel_brushes.new_intensity", "new", "path", json!({"category":self.category,"kind":"intensity"})),
            ("pixel_brushes.from_selection", "from_selection", "name", json!({"category":self.category})),
            ("pixel_brushes.category_add", "category.add", "name", json!({})),
            ("pixel_brushes.category_rename", "category.rename", "new_name", json!({"name":self.category})),
            ("pixel_brushes.import", "import", "path", json!({})),
            ("pixel_brushes.export", "export", "path", json!({"category":self.category})),
        ];
        for (title, verb, field, params) in choices {
            if matches!(verb, "category.add" | "import") { ui.separator(); }
            let enabled = (verb != "category.rename" || self.category != "Basic")
                && (verb != "from_selection" || from_selection);
            if ui.add_enabled(enabled, egui::Button::new(text(title))).clicked() {
                ui.close();
                if verb == "new" && field == "path" {
                    self.choose_stamp(title, params);
                } else if verb == "import" {
                    self.choose_import(title);
                } else {
                    let name = if verb == "from_selection" { text("pixel_brushes.selection_name").to_owned() } else { String::new() };
                    self.form = Some(Form::input(title, verb, field, name, params));
                }
            }
        }
        let empty = self.category != "Basic" && !tips.iter().any(|tip| tip.category == self.category);
        if ui.add_enabled(empty, egui::Button::new(text("pixel_brushes.category_delete"))).clicked() {
            self.form = Some(Form::delete("pixel_brushes.category_delete", "category.delete", json!({"name":self.category})));
            ui.close();
        }
    }

    #[cfg(windows)]
    fn choose_stamp(&mut self, title: &'static str, mut params: Value) {
        let picked = crate::dialogs::pick_file_path_with_extensions(text(title), text("pixel_brushes.image_files"),
            &["png", "jpg", "jpeg", "webp", "gif", "tif", "tiff", "bmp", "tga", "hdr", "exr"], None);
        match picked {
            Ok(Some(path)) => {
                let name = path.file_stem().and_then(|name| name.to_str()).unwrap_or("").trim().chars().take(64).collect();
                params["path"] = json!(path);
                self.form = Some(Form::input(title, "new", "name", name, params));
            }
            Ok(None) => {}
            Err(_) => self.message = Some("error.io_error".to_owned()),
        }
    }

    #[cfg(not(windows))]
    fn choose_stamp(&mut self, title: &'static str, params: Value) {
        self.form = Some(Form::input(title, "new", "path", String::new(), params));
    }

    #[cfg(windows)]
    fn choose_import(&mut self, title: &'static str) {
        match crate::dialogs::pick_file_path_with_extensions(
            text(title), text("pixel_brushes.brush_files"), &["dtoolbrushes", "abr", "afbrushes"], None,
        ) {
            Ok(path) => self.import_path = path,
            Err(_) => self.message = Some("error.io_error".to_owned()),
        }
    }

    #[cfg(not(windows))]
    fn choose_import(&mut self, title: &'static str) {
        self.form = Some(Form::input(title, "import", "path", String::new(), json!({})));
    }

    fn show_form(&mut self, ui: &mut Ui, workspace: &mut Workspace) {
        let Some(form) = self.form.as_mut() else { return; };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new(text(form.title)).id(egui::Id::new("pixel-brush-library-form"))
            .collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                if let Some(field) = form.field {
                    let label = if field == "path" { "pixel_brushes.path" } else { "path_brushes.name" };
                    ui.label(text(label));
                    ui.add(egui::TextEdit::singleline(&mut form.input).desired_width(280.0));
                } else {
                    ui.label(text("pixel_brushes.delete_confirm"));
                }
                ui.horizontal(|ui| {
                    let enabled = form.field.is_none() || !form.input.trim().is_empty();
                    submit = ui.add_enabled(enabled, egui::Button::new(text("dialog.ok"))).clicked();
                    cancel = ui.button(text("dialog.cancel")).clicked();
                });
            });
        if cancel { self.form = None; }
        if !submit { return; }
        let Some(mut form) = self.form.take() else { return; };
        if let Some(field) = form.field { form.params[field] = json!(form.input.trim()); }
        self.run_form(workspace, form.verb, form.input.trim(), form.params);
    }

    fn run_form(&mut self, workspace: &mut Workspace, verb: &str, label: &str, params: Value) {
        let command = Command::with_params(&format!("brush.presets.{verb}"), params);
        self.import_count = None;
        match workspace.run(Scope::active(), command).1 {
            Ok(result) if verb == "import" => {
                if let Some(category) = result["category"].as_str() {
                    self.category = category.to_owned();
                    self.search.clear();
                }
                self.import_count = Some((
                    result["imported"].as_u64().unwrap_or(0),
                    result["updated"].as_u64().unwrap_or(0),
                    result["skipped"].as_u64().unwrap_or(0),
                    result["limited"].as_u64().unwrap_or(0),
                ));
                self.message = None;
            }
            Ok(_) => {
                if matches!(verb, "category.add" | "category.rename") {
                    self.category = label.to_owned();
                    self.search.clear();
                }
                self.message = Some("pixel_brushes.done".to_owned());
            }
            Err(error) => self.message = Some(error.message_key),
        }
    }
}

pub(crate) fn category_label(category: &str) -> &str {
    if category == "Basic" { text("pixel_brushes.basic") } else { category }
}

pub(crate) fn tip_label(tip: &BrushTip) -> &str {
    match tip.id.as_str() {
        "round_hard" => text("pixel_brushes.round_hard"),
        "round_soft" => text("pixel_brushes.round_soft"),
        "square_hard" => text("pixel_brushes.square_hard"),
        _ => &tip.name,
    }
}

fn row(ui: &mut Ui, tip: &BrushTip, texture: &egui::TextureHandle, width: f32, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width.max(1.0), 46.0), Sense::click());
    let label = tip_label(tip);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    let palette = ui.visuals();
    let painter = ui.painter().with_clip_rect(rect);
    let fill = if selected { palette.selection.bg_fill } else if response.hovered() { palette.widgets.hovered.weak_bg_fill }
        else { palette.widgets.inactive.weak_bg_fill };
    painter.rect_filled(rect, 2.0, fill);
    let preview = Rect::from_min_size(rect.min + egui::vec2(3.0, 1.0), egui::vec2((rect.width() - 6.0).max(1.0), 29.0));
    painter.image(texture.id(), preview, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    painter.text(rect.left_bottom() + egui::vec2(5.0, -3.0), egui::Align2::LEFT_BOTTOM, label,
        egui::TextStyle::Small.resolve(ui.style()), ui.visuals().text_color());
    dtool_design::tooltip(response, format!("{} · {} px", label, tip.size_px))
}
