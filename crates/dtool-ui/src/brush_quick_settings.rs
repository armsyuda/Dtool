use dtool_core::commands::pixel_presets;
use dtool_core::context_toolbar::{OptionKind, find_option};
use dtool_core::{Command, Session};
use dtool_strings::text;
use egui::Ui;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct QuickSettings {
    category: String,
    search: String,
    library: Option<Arc<dtool_core::brush::BrushLibrary>>,
    previews: HashMap<String, egui::TextureHandle>,
}

#[derive(Default)]
pub(crate) struct QuickAction {
    pub edit: bool,
    pub changed: bool,
}

impl QuickSettings {
    pub fn show(&mut self, ui: &mut Ui, session: &mut Session) -> QuickAction {
        let width = (ui.ctx().content_rect().width() - 28.0).clamp(160.0, 326.0);
        ui.set_width(width);
        ui.strong(text("brush_editor.current_settings"));
        let height = ui.ctx().content_rect().height();
        if height < 440.0 {
            egui::ScrollArea::vertical().max_height((height - 52.0).max(90.0))
                .show(ui, |ui| self.controls(ui, session, width, height)).inner
        } else {
            self.controls(ui, session, width, height)
        }
    }

    fn controls(&mut self, ui: &mut Ui, session: &mut Session, width: f32, height: f32) -> QuickAction {
        use crate::panels::pixel_brushes::{category_label, row, tip_label};
        let tool = session.active_tool.clone();
        let current = session.tool_options.raster_tip(&tool);
        let caption = current.map_or_else(|| text("brush_editor.current"), tip_label);
        let edit = ui.horizontal(|ui| {
            let button = dtool_design::tooltip(dtool_design::icon_button(ui, "⚙"), text("brush_editor.more"));
            button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text("brush_editor.more")));
            ui.add(egui::Label::new(caption).truncate());
            button.clicked()
        }).inner;
        if edit {
            ui.close();
            return QuickAction { edit: true, changed: false };
        }
        let mut action = QuickAction::default();
        ui.separator();
        for (id, suffix) in [("brush.width", " px"), ("brush.opacity", "%"), ("brush.hardness", "%"), ("brush.flow", "%")] {
            let Some(spec) = find_option(&tool, id) else { continue };
            let OptionKind::Number { min, max, step, default } = spec.kind else { continue };
            let mut value = session.tool_options.number(&tool, id).unwrap_or(default) as f32;
            let enabled = id != "brush.hardness" || session.tool_options.raster_tip(&tool).is_none_or(|tip| tip.image.is_none());
            if crate::brush_editor::brush_control(ui, spec.key, &mut value, min as f32..=max as f32,
                step, suffix, enabled, Some(default), true)
            {
                action.changed |= session.tool_options.set(&tool, id, &json!(value)).is_some();
            }
        }
        ui.separator();
        let Ok(library) = pixel_presets::library_snapshot() else { return action };
        if !self.library.as_ref().is_some_and(|old| Arc::ptr_eq(old, &library)) {
            self.previews.clear();
            self.library = Some(Arc::clone(&library));
        }
        if !library.categories.contains(&self.category) {
            self.category = library.categories.first().cloned().unwrap_or_default();
        }
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(text("path_brushes.search")).desired_width(width));
        egui::ComboBox::from_id_salt("quick_brush_category").width(width - 12.0)
            .selected_text(category_label(&self.category)).show_ui(ui, |ui| {
                for category in &library.categories {
                    ui.selectable_value(&mut self.category, category.clone(), category_label(category));
                }
            });
        let search = self.search.trim().to_lowercase();
        let rows: Vec<_> = library.tips.iter().filter(|tip| tip.category == self.category
            && (search.is_empty() || tip_label(tip).to_lowercase().contains(&search))).collect();
        let active = session.tool_options.raster_tip(&tool).map(|tip| tip.id.clone());
        let mut chosen = None;
        let height = (height - 310.0).clamp(68.0, 244.0);
        egui::ScrollArea::vertical().max_height(height).show_rows(ui,
            crate::panels::pixel_brushes::ROW_HEIGHT, rows.len(), |ui, range| {
                for tip in &rows[range] {
                    let preview = self.previews.entry(tip.id.clone()).or_insert_with(|| {
                        ui.ctx().load_texture(format!("quick_brush.{}", tip.id),
                            crate::brush_editor::preview(tip), egui::TextureOptions::LINEAR)
                    });
                    if row(ui, tip, preview, ui.available_width(), active.as_deref() == Some(tip.id.as_str())).clicked() {
                        chosen = Some(tip.id.clone());
                    }
                }
            });
        if let Some(id) = chosen {
            action.changed |= session.run(&Command::with_params(pixel_presets::APPLY, json!({"id": id, "tool": tool}))).is_ok();
        }
        action
    }
}
