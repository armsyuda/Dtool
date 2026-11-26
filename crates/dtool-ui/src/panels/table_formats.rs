use dtool_core::table::model::{TableFormatDef, TableFormatId};
use dtool_core::table::resolve::{pattern_slot, slot_format};
use dtool_core::table::styles::{DEFAULT_RESET, DEFAULT_SAVE};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{Command, Document, ObjectKind, Session};
use dtool_design::{button, panel_frame, panel_header};
use dtool_strings::text;
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui};
use serde_json::json;

mod editor;
mod images;

pub const PANEL_KEY: &str = "table_formats";

#[derive(Default)]
pub struct TableFormatsPanel {
    context: Option<ContextId>,
    selected: Option<TableFormatId>,
    name: String,
    message: Option<String>,
    import: Option<ImportDialog>,
    editor: Option<editor::FormatEditor>,
    images: images::Images,
}

impl TableFormatsPanel {
    pub fn show(&mut self, ui: &mut Ui, workspace: &mut Workspace) {
        let context = workspace.active();
        if self.context != context {
            *self = Self { context, ..Self::default() };
        }
        panel_frame(ui, |ui| {
            panel_header(ui, text("panel.table_formats"));
            let Some(session) = workspace.session_mut() else { return; };
            self.images.sync(&session.document);
            let selected_table = match session.selection.ids.as_slice() {
                [id] => session.document.get_object(*id).filter(|obj| matches!(obj.kind, ObjectKind::Table(_))),
                _ => None,
            };
            let can_capture = selected_table.is_some();
            let can_apply = selected_table.is_some_and(|obj| !obj.locked);
            let formats = &session.document.table_formats;
            self.selected = self.selected.filter(|id| formats.iter().any(|format| format.id == *id));
            let mut command = None;
            if formats.is_empty() {
                ui.label(text("table.formats.empty"));
            }
            ui.horizontal_wrapped(|ui| {
                for format in formats {
                    let id = format.id;
                    let response = tile(ui, format, self.selected == Some(id), &session.document, &mut self.images);
                    if response.clicked() || response.secondary_clicked() {
                        self.selected = Some(id);
                    }
                    if response.double_clicked() && can_apply {
                        command = Some(apply_command(id, false));
                    }
                    response.context_menu(|ui| {
                        if button(ui, text("table.formats.edit")).clicked() {
                            match editor::FormatEditor::existing(format) {
                                Ok(editor) => self.editor = Some(editor),
                                Err(error) => self.message = Some(error.message_key),
                            }
                            ui.close();
                        }
                        for (key, enabled, action) in [
                            ("button.apply", can_apply, apply_command(id, false)),
                            ("table.formats.override", can_apply, apply_command(id, true)),
                            ("table.formats.update", can_capture, Command::with_params("style.table.from_selection", json!({"id":id.0,"update":true}))),
                            ("table.formats.duplicate", true, Command::with_params("style.table.duplicate", json!({"id":id.0}))),
                            ("button.delete", !format_in_use(&session.document, id), Command::with_params("style.table.delete", json!({"id":id.0}))),
                        ] {
                            if ui.add_enabled(enabled, egui::Button::new(text(key))).clicked() {
                                command = Some(action);
                                ui.close();
                            }
                        }
                    });
                }
            });
            if let Some(format) = formats.iter().find(|format| Some(format.id) == self.selected) {
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::Label::new(&format.name).truncate());
                    if ui.add_enabled(can_apply, egui::Button::new(text("button.apply"))).clicked() {
                        command = Some(apply_command(format.id, false));
                    }
                });
            }
            if let Some(default) = formats.iter().find(|format| Some(format.id) == session.document.default_table_format) {
                ui.label(format!("{} · {}", text("table.formats.defaults"), default.name));
            }
            ui.separator();
            let create = ui.add_enabled_ui(can_capture, |ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut self.name).hint_text(text("table.formats.name")).desired_width(f32::INFINITY));
                let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                let valid = !self.name.trim().is_empty();
                let click = ui.add_enabled(valid, egui::Button::new(text("table.formats.capture"))).clicked();
                valid && (enter || click)
            }).inner;
            if create {
                command = Some(Command::with_params("style.table.from_selection", json!({"name":self.name.trim()})));
            }
            if button(ui, text("table.formats.import")).clicked() {
                self.import = Some(ImportDialog::default());
            }
            if button(ui, text("table.formats.new")).clicked() {
                self.editor = Some(editor::FormatEditor::default());
            }
            ui.menu_button(text("table.formats.defaults"), |ui| {
                if ui.add_enabled(!formats.is_empty(), egui::Button::new(text("menu.text.table.save_formats_as_default")))
                    .on_hover_text(text("table.formats.save_hint")).clicked()
                {
                    command = Some(match self.selected {
                        Some(id) => Command::with_params(DEFAULT_SAVE, json!({"id": id.0})),
                        None => Command::new(DEFAULT_SAVE),
                    });
                    ui.close();
                }
                if button(ui, text("table.formats.restore_defaults")).on_hover_text(text("table.formats.restore_hint")).clicked() {
                    command = Some(Command::new(DEFAULT_RESET));
                    ui.close();
                }
            });
            if let Some(command) = command {
                match session.run(&command) {
                    Ok(result) => {
                        self.message = None;
                        match command.name.as_str() {
                            DEFAULT_SAVE => self.message = Some("table.formats.saved_defaults".into()),
                            DEFAULT_RESET => {
                                if result.get("changes").is_some() {
                                    self.message = Some("table.formats.restored_defaults".into());
                                }
                            }
                            "style.table.delete" => self.selected = None,
                            _ => self.selected = result["id"].as_u64().map(TableFormatId),
                        }
                        if create { self.name.clear(); }
                    }
                    Err(error) => self.message = Some(error.message_key),
                }
            }
            if let Some(message) = &self.message { ui.label(text(message)); }
            if let Some(dialog) = &mut self.import {
                if dialog.show(ui.ctx(), session, &mut self.images) { self.import = None; }
            }
            if let Some(editor) = &mut self.editor {
                match editor.show(ui.ctx(), session, &mut self.images) {
                    editor::Outcome::Saved(id) => { self.selected = Some(id); self.editor = None; }
                    editor::Outcome::Cancelled => self.editor = None,
                    editor::Outcome::Open => {}
                }
            }
        });
    }
}

#[derive(Default)]
struct ImportDialog {
    path: String,
    source: Option<Document>,
    images: images::Images,
    rows: Vec<ImportRow>,
    error: Option<String>,
}

struct ImportRow {
    format: TableFormatDef,
    selected: bool,
    name: String,
}

impl ImportDialog {
    fn load(&mut self) {
        self.rows.clear();
        self.source = None;
        self.images = Default::default();
        let path = std::path::Path::new(self.path.trim());
        match dtool_core::commands::document_files::read_document(path) {
            Ok(source) => {
                self.rows = source.table_formats.iter().cloned().map(|format| ImportRow { name: format.name.clone(), format, selected: true }).collect();
                self.images.sync(&source);
                self.source = Some(source);
                self.error = None;
            }
            Err(error) => self.error = Some(error.message_key),
        }
    }

    fn show(&mut self, ctx: &egui::Context, session: &mut Session, existing_images: &mut images::Images) -> bool {
        use crate::dialog_chrome::{show_modal, ClosePolicy, DialogResult};
        let (event, close) = show_modal(ctx, "table-format-import", text("table.formats.import"), ClosePolicy::CancelOnly, false, |ui| {
            ui.set_width(ui.ctx().content_rect().width().min(540.0) * 0.9);
            ui.label(text("dialog.file.path"));
            ui.horizontal(|ui| {
                let path = ui.add(egui::TextEdit::singleline(&mut self.path).desired_width((ui.available_width() - 100.0).max(90.0)));
                if path.changed() {
                    self.rows.clear();
                    self.source = None;
                    self.images = Default::default();
                    self.error = None;
                }
                if button(ui, text("table.formats.browse")).clicked() {
                    let picked = crate::dialogs::pick_file_path(text("table.formats.import"), text("dialog.file.filter.native"), "dtool", None);
                    match picked {
                        Ok(Some(path)) => { self.path = path.to_string_lossy().into_owned(); self.load(); }
                        Ok(None) => {}
                        Err(_) => self.error = Some("table.formats.browse_error".into()),
                    }
                }
                if ui.add_enabled(!self.path.trim().is_empty(), egui::Button::new(text("table.formats.read"))).clicked() {
                    self.load();
                }
            });
            if self.source.is_some() && self.rows.is_empty() { ui.label(text("table.formats.empty")); }
            let mut taken = std::collections::HashSet::new();
            let duplicates: std::collections::HashSet<_> = self.rows.iter().filter(|row| row.selected)
                .filter_map(|row| (!taken.insert(row.name.trim())).then_some(row.name.trim().to_string()))
                .collect();
            let mut valid = true;
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for row in &mut self.rows {
                    ui.push_id(row.format.id.0, |ui| {
                        ui.separator();
                        dtool_design::toggle(ui, &mut row.selected, &row.format.name);
                        ui.horizontal(|ui| {
                            ui.label(text("table.formats.new_name"));
                            ui.add_enabled(row.selected, egui::TextEdit::singleline(&mut row.name).desired_width(175.0));
                        });
                        let name = row.name.trim();
                        let existing = session.document.table_formats.iter().find(|format| format.name == name);
                        if row.selected && (name.is_empty() || existing.is_some() || duplicates.contains(name)) {
                            valid = false;
                            ui.label(text("table.formats.name_conflict"));
                        }
                        ui.horizontal(|ui| {
                            if let Some(source) = &self.source {
                                preview(ui, &row.format, "table.formats.incoming", source, &mut self.images);
                            }
                            if row.selected {
                                if let Some(format) = existing { preview(ui, format, "table.formats.existing", &session.document, existing_images); }
                            }
                        });
                    });
                }
            });
            if let Some(error) = &self.error { ui.label(text(error)); }
            let mut done = false;
            ui.horizontal(|ui| {
                let enabled = valid && self.rows.iter().any(|row| row.selected);
                if ui.add_enabled(enabled, egui::Button::new(text("button.ok"))).clicked() {
                    let picks: Vec<_> = self.rows.iter().filter(|row| row.selected)
                        .map(|row| json!({"id":row.format.id.0,"rename_to":row.name.trim()})).collect();
                    let command = Command::with_params("style.table.import", json!({"path":self.path.trim(),"picks":picks}));
                    match session.run(&command) {
                        Ok(_) => done = true,
                        Err(error) => self.error = Some(error.message_key),
                    }
                }
                done |= button(ui, text("button.cancel")).clicked();
            });
            done
        });
        close || event == DialogResult::Cancel
    }
}

fn preview(ui: &mut Ui, format: &TableFormatDef, key: &str, doc: &Document, images: &mut images::Images) {
    ui.vertical(|ui| {
        ui.label(text(key));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(84.0, 39.0), egui::Sense::hover());
        paint_tile(ui.painter(), rect, format, doc, images);
    });
}

fn apply_command(id: TableFormatId, override_local: bool) -> Command {
    Command::with_params("style.table.apply", json!({"id":id.0,"override_local":override_local}))
}

fn format_in_use(doc: &dtool_core::Document, id: TableFormatId) -> bool {
    doc.default_table_format == Some(id)
        || doc.objects.iter().any(|obj| matches!(&obj.kind, ObjectKind::Table(table) if table.format == Some(id)))
}

fn tile(ui: &mut Ui, format: &TableFormatDef, selected: bool, doc: &Document, images: &mut images::Images) -> Response {
    let response = ui.add_sized([72.0, 45.0], egui::Button::new("").selected(selected))
        .on_hover_text(&format.name);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, &format.name));
    paint_tile(ui.painter(), response.rect.shrink(5.0), format, doc, images);
    response
}

fn paint_tile(painter: &egui::Painter, rect: Rect, format: &TableFormatDef, doc: &Document, images: &mut images::Images) {
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    images.fill(painter, rect, format.frame.fill.as_ref(), doc);
    let rows = u32::from(format.pattern.rows.clamp(1, 5));
    let cols = u32::from(format.pattern.cols.clamp(1, 6));
    let step = egui::vec2(rect.width() / cols as f32, rect.height() / rows as f32);
    for row in 0..rows {
        for col in 0..cols {
            let pos = rect.min + egui::vec2(col as f32 * step.x, row as f32 * step.y);
            let cell_rect = Rect::from_min_size(pos, step);
            let style = pattern_slot(&format.pattern, rows, cols, row, col).and_then(|slot| slot_format(format, slot));
            images.fill(painter, cell_rect, style.and_then(|style| style.fill.as_ref()), doc);
            painter.rect_stroke(cell_rect, 0.0, Stroke::new(0.55, Color32::from_gray(140)), egui::StrokeKind::Inside);
            if let Some(style) = style {
                paint_edges(painter, cell_rect, &style.edges);
            }
        }
    }
    paint_edges(painter, rect, &format.frame.edges);
}

fn paint_edges(painter: &egui::Painter, rect: Rect, edges: &[Option<dtool_core::table::model::EdgeStyle>; 4]) {
    use dtool_core::paint::LineStyle;
    let segments: [[Pos2; 2]; 4] = [
        [rect.left_bottom(), rect.left_top()], [rect.left_top(), rect.right_top()],
        [rect.right_top(), rect.right_bottom()], [rect.right_bottom(), rect.left_bottom()],
    ];
    for (segment, edge) in segments.into_iter().zip(edges.iter()) {
        if let Some(edge) = edge.as_ref().filter(|edge| edge.width_pt > 0.0) {
            if let Some(colour) = crate::fill_stroke::paint_well_colour(&edge.paint) {
                let width = edge.width_pt.clamp(0.7, 2.0);
                let stroke = Stroke::new(width, colour);
                match edge.line_style {
                    LineStyle::Solid => { painter.line_segment(segment, stroke); }
                    LineStyle::Dashed => painter.extend(egui::Shape::dashed_line(&segment, stroke, 4.0 * width, 2.0 * width)),
                    LineStyle::Dotted => painter.extend(egui::Shape::dotted_line(&segment, colour, 3.0 * width, width * 0.5)),
                }
            }
        }
    }
}
