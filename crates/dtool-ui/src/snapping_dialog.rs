use dtool_core::{Command, CommandResult, Scope, Workspace};
use dtool_core::snap_prefs::{MAX_CANDIDATES_RANGE, SCREEN_TOLERANCE_RANGE};
use dtool_strings::text;
use serde_json::{json, Value};

pub const SHOW_SNAPPING_MANAGER: &str = "snap.prefs.query";
pub const SET_SNAPPING: &str = "snap.prefs.set";
pub const MENU_ID: &str = "view.snapping";

pub use dtool_core::snap_prefs::CANDIDATE_MODES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Toggle,
    Count,
    Mode,
}

pub const ROWS: &[(&str, &str, RowKind, Option<&str>)] = &[
    ("enabled", "dialog.snapping.enabled", RowKind::Toggle, None),
    ("screen_tolerance", "dialog.snapping.screen_tolerance", RowKind::Count, Some("enabled")),
    ("candidates_mode", "dialog.snapping.candidates_mode", RowKind::Mode, Some("enabled")),
    ("max_candidates", "dialog.snapping.max_candidates", RowKind::Count, Some("enabled")),
    ("show_candidates", "dialog.snapping.show_candidates", RowKind::Toggle, Some("enabled")),
    ("force_pixel_alignment", "dialog.snapping.force_pixel_alignment", RowKind::Toggle, Some("enabled")),
    ("move_by_whole_pixels", "dialog.snapping.move_by_whole_pixels", RowKind::Toggle, Some("enabled")),
    ("page_snapping", "dialog.snapping.page_snapping", RowKind::Toggle, Some("enabled")),
    ("snap_to_grid", "dialog.snapping.grid", RowKind::Toggle, Some("enabled")),
    ("snap_to_baseline_grid", "dialog.snapping.snap_to_baseline_grid", RowKind::Toggle, Some("enabled")),
    ("snap_to_guides", "dialog.snapping.guides", RowKind::Toggle, Some("enabled")),
    ("snap_to_spreads", "dialog.snapping.snap_to_spreads", RowKind::Toggle, Some("enabled")),
    ("include_spread_midpoints", "dialog.snapping.include_spread_midpoints", RowKind::Toggle, Some("snap_to_spreads")),
    ("snap_to_margin", "dialog.snapping.snap_to_margin", RowKind::Toggle, Some("enabled")),
    ("include_margin_midpoints", "dialog.snapping.include_margin_midpoints", RowKind::Toggle, Some("snap_to_margin")),
    ("object_snapping", "dialog.snapping.objects", RowKind::Toggle, Some("enabled")),
    ("only_visible_objects", "dialog.snapping.only_visible_objects", RowKind::Toggle, Some("object_snapping")),
    ("snap_to_object_bounds", "dialog.snapping.snap_to_object_bounds", RowKind::Toggle, Some("object_snapping")),
    ("include_bbox_midpoints", "dialog.snapping.include_bbox_midpoints", RowKind::Toggle, Some("snap_to_object_bounds")),
    ("snap_to_gaps_and_sizes", "dialog.snapping.snap_to_gaps_and_sizes", RowKind::Toggle, Some("object_snapping")),
    ("snap_to_shape_key_points", "dialog.snapping.snap_to_shape_key_points", RowKind::Toggle, Some("object_snapping")),
    ("snap_to_object_geometry", "dialog.snapping.snap_to_object_geometry", RowKind::Toggle, Some("object_snapping")),
    ("snap_to_pixel_selection_bounds", "dialog.snapping.snap_to_pixel_selection_bounds", RowKind::Toggle, Some("object_snapping")),
    ("dynamic_guides", "dialog.snapping.dynamic_guides", RowKind::Toggle, Some("enabled")),
];

#[derive(Debug, Clone, Default)]
pub struct SnappingDialog {
    pub open: bool,
    pub error_key: Option<String>,
}

pub fn query_prefs(workspace: &mut Workspace) -> CommandResult {
    workspace.run(Scope::active(), Command::with_params(SHOW_SNAPPING_MANAGER, json!({}))).1
}

pub fn apply_toggle(workspace: &mut Workspace, field: &str, value: Value) -> CommandResult {
    let mut params = serde_json::Map::new();
    params.insert(field.to_string(), value);
    workspace.run(Scope::active(), Command::with_params(SET_SNAPPING, Value::Object(params))).1
}

pub fn row_enabled(prefs: &Value, parent: Option<&str>) -> bool {
    let mut current = parent;
    while let Some(field) = current {
        if !prefs.get(field).and_then(Value::as_bool).unwrap_or(false) {
            return false;
        }
        current = ROWS.iter().find(|row| row.0 == field).and_then(|row| row.3);
    }
    true
}

pub fn handles_menu(action_id: &str, dialog: &mut SnappingDialog) -> bool {
    if action_id == MENU_ID {
        dialog.open();
        true
    } else {
        false
    }
}

impl SnappingDialog {
    pub fn open(&mut self) {
        self.open = true;
        self.error_key = None;
    }

    pub fn show(&mut self, ctx: &egui::Context, workspace: &mut Workspace) {
        if !self.open {
            return;
        }
        let prefs = match query_prefs(workspace) {
            Ok(value) => Some(value),
            Err(error) => {
                self.error_key = Some(error.message_key);
                None
            }
        };
        let mut changes: Vec<(&'static str, Value)> = Vec::new();
        let mut open = self.open;
        let mut close = false;
        let height = (ctx.content_rect().height() - 160.0).clamp(160.0, 620.0);
        egui::Window::new(text("dialog.snapping.title"))
            .collapsible(false).default_width(370.0).min_width(270.0)
            .open(&mut open).show(ctx, |ui| {
            dtool_design::dialog_frame(ui, |ui| {
                dtool_design::dialog_options_scroll(ui, "snapping.options", height, |ui| {
                    if let Some(prefs) = &prefs {
                        for (field, key, kind, parent) in ROWS {
                            if matches!(*field, "candidates_mode" | "force_pixel_alignment" | "page_snapping" | "object_snapping" | "dynamic_guides") {
                                ui.separator();
                            }
                            let enabled = row_enabled(prefs, *parent);
                            ui.add_enabled_ui(enabled, |ui| match kind {
                                RowKind::Toggle => {
                                    let mut on = prefs.get(*field).and_then(Value::as_bool).unwrap_or(false);
                                    if dtool_design::toggle(ui, &mut on, text(key)).changed() {
                                        changes.push((*field, json!(on)));
                                    }
                                }
                                RowKind::Count => {
                                    let range = if *field == "screen_tolerance" { SCREEN_TOLERANCE_RANGE } else { MAX_CANDIDATES_RANGE };
                                    let mut count = prefs.get(*field).and_then(Value::as_u64).unwrap_or(1) as f64;
                                    dtool_design::studio_row(ui, text(key), |ui, label| {
                                        let spec = dtool_design::NumericSpec::new("")
                                            .range(f64::from(*range.start())..=f64::from(*range.end()))
                                            .width(72.0).decimals(0).step(1.0).enabled(enabled);
                                        let id = ui.id().with(field);
                                        if dtool_design::numeric_value(ui, id, &mut count, &spec).labelled_by(label).changed() {
                                            changes.push((*field, json!(count.round() as u32)));
                                        }
                                    });
                                }
                                RowKind::Mode => {
                                    let current = prefs.get(*field).and_then(Value::as_str).unwrap_or(CANDIDATE_MODES[0]).to_string();
                                    let mut chosen = current.clone();
                                    egui::ComboBox::from_label(text(key)).selected_text(text(&mode_key(&current))).show_ui(ui, |ui| {
                                        for mode in CANDIDATE_MODES {
                                            ui.selectable_value(&mut chosen, mode.to_string(), text(&mode_key(mode)));
                                        }
                                    });
                                    if chosen != current {
                                        changes.push((*field, json!(chosen)));
                                    }
                                }
                            });
                        }
                    }
                    if let Some(key) = &self.error_key {
                        ui.colored_label(ui.visuals().error_fg_color, text(key));
                    }
                });
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = dtool_design::button(ui, text("button.close")).clicked();
                });
            });
        });
        self.open = open && !close;
        for (field, value) in changes {
            match apply_toggle(workspace, field, value) {
                Ok(_) => self.error_key = None,
                Err(error) => self.error_key = Some(error.message_key),
            }
        }
    }
}

pub fn mode_key(mode: &str) -> String {
    format!("dialog.snapping.mode.{}", mode.to_lowercase().replace(' ', "_"))
}

pub fn view_snapping_menu_item(ui: &mut egui::Ui) -> bool {
    dtool_design::menu_item(ui, text("menu.view.snapping")).clicked()
}
