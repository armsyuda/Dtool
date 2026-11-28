use dtool_core::Session;
use dtool_strings::text;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub index: u64,
    pub label: String,
    pub current: bool,
}

pub struct HistoryPanel {
    pub show_advanced: bool,
}

impl Default for HistoryPanel {
    fn default() -> Self {
        Self { show_advanced: false }
    }
}

pub fn entry_label(entry: &Value) -> String {
    let key = entry["label_key"].as_str().unwrap_or("");
    if dtool_strings::has(key) {
        return text(key).to_string();
    }
    entry["label"].as_str().unwrap_or(key).to_string()
}

pub fn rows(session: &mut Session) -> Vec<HistoryRow> {
    let Ok(list) = session.run_named("history.list", json!({})) else { return Vec::new() };
    let cursor = list["cursor"].as_u64();
    list["entries"].as_array().map(|entries| entries.iter().filter_map(|entry| {
        let index = entry["index"].as_u64()?;
        Some(HistoryRow { index, label: entry_label(entry), current: cursor == Some(index) })
    }).collect()).unwrap_or_default()
}

pub fn jump(session: &mut Session, index: u64) -> bool {
    session.run_named("history.jump", json!({"index": index})).is_ok()
}

pub(crate) fn brush_source_button(ui: &mut egui::Ui, selected: bool) -> egui::Response {
    let label = text("history.brush.source.set");
    let state = dtool_design::ControlState::default().tooltip(label);
    let icon = if selected { "◉" } else { "◎" };
    let response = dtool_design::small_icon_button_with(ui, icon, &state);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    response
}

impl HistoryPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        let rows = rows(session);
        use dtool_core::history_store::BrushSource;
        let source = session.history.brush_source.as_ref().map(|(source, _)| *source);
        let mut set_source = None;
        let mut jump_to: Option<u64> = None;
        egui::ScrollArea::vertical().id_salt("history_rows").show(ui, |ui| {
            for row in &rows {
                ui.horizontal(|ui| {
                    let selected = session.undo.history_step_id(row.index)
                        .is_some_and(|id| source == Some(BrushSource::History(id)));
                    if brush_source_button(ui, selected).clicked() { set_source = Some(row.index); }
                    if row.current {
                        ui.add_enabled_ui(false, |ui| { dtool_design::button(ui, row.label.clone()); });
                    } else if dtool_design::button(ui, row.label.clone()).clicked() {
                        jump_to = Some(row.index);
                    }
                });
            }
        });
        if let Some(index) = set_source {
            let _ = session.run_named(dtool_core::commands::history_menu::BRUSH_SOURCE_SET, json!({"index": index}));
        }
        if let Some(index) = jump_to {
            jump(session, index);
        }
        dtool_design::toggle(ui, &mut self.show_advanced, text("panel.history.advanced"));
        if !self.show_advanced {
            return;
        }
        ui.separator();
        let Ok(budget) = session.run_named("history.budget.query", json!({})) else { return };
        let mut max_steps = budget["max_steps"].as_u64().unwrap_or(0).min(dtool_core::commands::history_menu::MAX_HISTORY_STEPS as u64) as u32;
        let before = max_steps;
        ui.horizontal(|ui| {
            ui.label(text("history.budget.steps"));
            let mut steps = f64::from(max_steps);
            let range = 1.0..=f64::from(dtool_core::commands::history_menu::MAX_HISTORY_STEPS);
            if dtool_design::number_drag(ui, egui::Id::NULL.with("dtool.history.max_steps"), &mut steps, range, 1.0).changed() {
                max_steps = steps.round() as u32;
            }
        });
        if max_steps != before && max_steps > 0 {
            let _ = session.run_named("history.budget.set", json!({"max_steps": max_steps}));
        }
        let bytes = budget["memory_estimate_bytes"].as_u64().unwrap_or(0).to_string();
        ui.label(dtool_strings::format("history.budget.memory", &[("bytes", &bytes)]));
    }
}
