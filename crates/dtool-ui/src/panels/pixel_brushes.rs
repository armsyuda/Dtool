use crate::dialog_chrome::{ClosePolicy, DialogResult, confirm_cancel_buttons, show_modal};
use dtool_core::brush::{BrushLibrary, BrushTip};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use dtool_core::commands::pixel_presets::{self, APPLY};
use dtool_core::workspace::Workspace;
use dtool_core::{Command, Scope};
use dtool_strings::text;
use egui::{Color32, Pos2, Rect, Ui};
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
    file_action: Option<(&'static str, Value)>,
    import_count: Option<(u64, u64, u64, u64)>,
    pub(crate) edit_target: Option<String>,
    preview_library: Option<Arc<BrushLibrary>>,
    previews: HashMap<String, egui::TextureHandle>,
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
        let library = match pixel_presets::library_snapshot() {
            Ok(library) => library,
            Err(error) => { ui.weak(text(&error.message_key)); return; }
        };
        if !self.preview_library.as_ref().is_some_and(|previous| Arc::ptr_eq(previous, &library)) {
            self.preview_library = Some(Arc::clone(&library));
            self.previews.clear();
        }
        if !library.categories.contains(&self.category) {
            self.category = library.categories.first().cloned().unwrap_or_default();
        }
        let active = workspace.session().and_then(|session| session.tool_options.raster_tip(&session.active_tool))
            .map(|tip| tip.id.clone());
        let available = workspace.session().is_some_and(|session| session.has_document);
        let from_selection = workspace.session().is_some_and(|session| session.can_execute_named("brush.presets.from_selection"));
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(text("path_brushes.search")).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            let trailing = 2.0 * (dtool_design::size("control.icon.width") + ui.spacing().item_spacing.x);
            let width = (ui.available_width() - trailing).max(1.0);
            let size = egui::vec2(width, ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(width);
                let category = egui::ComboBox::from_id_salt("raster-brush-category")
                    .width(width)
                    .truncate()
                    .selected_text(category_label(&self.category))
                    .show_ui(ui, |ui| {
                        for name in &library.categories {
                            ui.selectable_value(&mut self.category, name.clone(), category_label(name));
                        }
                    });
                dtool_design::tooltip(category.response, category_label(&self.category));
            });
            let toggle_label = text(if self.grid { "pixel_brushes.list" } else { "pixel_brushes.grid" });
            let toggle = dtool_design::tooltip(dtool_design::icon_button(ui, if self.grid { "☰" } else { "▦" }), toggle_label);
            toggle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), toggle_label));
            if toggle.clicked() { self.grid = !self.grid; }
            let label = text("pixel_brushes.options");
            let menu = dtool_design::tooltip(dtool_design::icon_button(ui, "⋯"), label);
            menu.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
            egui::Popup::menu(&menu).align(egui::RectAlign::BOTTOM_END)
                .show(|ui| self.menu(ui, &library.tips, from_selection));
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
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_HEIGHT, rows.len().div_ceil(columns), |ui, range| {
            for index in range {
                let page = &rows[index * columns..((index + 1) * columns).min(rows.len())];
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - ui.spacing().item_spacing.x * (columns - 1) as f32) / columns as f32;
                    for &tip in page {
                        visible_previews.insert(tip.id.as_str());
                        let preview = self.previews.entry(tip.id.clone()).or_insert_with(|| {
                            let image = crate::brush_editor::preview(tip);
                            ui.ctx().load_texture(format!("pixel-brush.preview.{}", tip.id), image, egui::TextureOptions::LINEAR)
                        });
                        let response = row(ui, tip, preview, width, active.as_deref() == Some(&tip.id));
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
        if let Some((verb, params)) = self.file_action.take() {
            self.run_form(workspace, verb, "", params);
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
                } else if matches!(verb, "import" | "export") {
                    self.choose_brush_file(title, verb, params);
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
    fn choose_brush_file(&mut self, title: &'static str, verb: &'static str, mut params: Value) {
        let export = verb == "export";
        let extensions: &[&str] = if export { &["dtoolbrushes"] } else { &["dtoolbrushes", "abr", "afbrushes"] };
        let filter = if export { "pixel_brushes.native_file" } else { "pixel_brushes.brush_files" };
        match crate::dialogs::pick_file_path_with_extensions(
            text(title), text(filter), extensions, export.then_some("brushes.dtoolbrushes"),
        ) {
            Ok(Some(path)) => {
                if export && path.exists() { params["replace_existing"] = json!(true); }
                params["path"] = json!(path);
                self.file_action = Some((verb, params));
            }
            Ok(None) => {}
            Err(_) => self.message = Some("error.io_error".to_owned()),
        }
    }

    #[cfg(not(windows))]
    fn choose_brush_file(&mut self, title: &'static str, verb: &'static str, params: Value) {
        self.form = Some(Form::input(title, verb, "path", String::new(), params));
    }

    fn show_form(&mut self, ui: &mut Ui, workspace: &mut Workspace) {
        let Some(form) = self.form.as_mut() else { return; };
        let (close, (submit, cancel)) = show_modal(
            ui.ctx(), "pixel-brush-library-form", text(form.title), ClosePolicy::CancelOnly, false, |ui| {
                ui.set_width((ui.ctx().content_rect().width() - 32.0).clamp(1.0, 300.0));
                if let Some(field) = form.field {
                    let label = if field == "path" { "pixel_brushes.path" } else { "path_brushes.name" };
                    ui.label(text(label));
                    ui.add(egui::TextEdit::singleline(&mut form.input).desired_width(ui.available_width()));
                } else {
                    ui.label(text("pixel_brushes.delete_confirm"));
                }
                let enabled = form.field.is_none() || !form.input.trim().is_empty();
                confirm_cancel_buttons(ui, text("dialog.ok"), enabled)
            },
        );
        if cancel || close == DialogResult::Cancel { self.form = None; }
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
            Ok(_) if verb == "export" => self.message = Some("pixel_brushes.export_done".to_owned()),
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

pub(crate) const ROW_HEIGHT: f32 = 46.0;

pub(crate) fn row(ui: &mut Ui, tip: &BrushTip, texture: &egui::TextureHandle, width: f32, selected: bool) -> egui::Response {
    let label = tip_label(tip);
    dtool_design::preview_list_row(ui, egui::vec2(width.max(1.0), ROW_HEIGHT), selected, label,
        format!("{:.0} px · {label}", tip.size_px), |ui, preview| {
            ui.painter().image(texture.id(), preview, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        })
}
