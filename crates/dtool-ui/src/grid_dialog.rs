use dtool_core::grid::{Grid, GridKind};
use dtool_core::grid::baseline::{Origin, Settings};
use dtool_core::command::Session;
use dtool_core::workspace::ContextId;
use dtool_design::{button, dialog_frame, menu_item, panel_header};
use dtool_strings::text;
use serde_json::json;

pub const GRID_AND_AXIS_DIALOG_XAML: &str = "ui__dialogs__gridandaxismanagerdialog.xaml";
pub const BASELINE_GRID_DIALOG_XAML: &str = "baselinegridmanagerdialog.xaml";

#[derive(Clone, Copy)]
pub(crate) enum GridMenu {
    Axis,
    Baseline,
}

impl GridMenu {
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        match id {
            "view.grid_and_axis" => Some(Self::Axis),
            "view.baseline_grid" => Some(Self::Baseline),
            _ => None,
        }
    }
}

pub fn get_menu_label_view_show_grid() -> String {
    text("menu.view.show.grid").to_string()
}

pub fn get_menu_label_view_show_pixel_grid() -> String {
    text("menu.view.show.pixel_grid").to_string()
}

pub fn get_menu_label_view_show_baseline_grid() -> String {
    text("menu.view.show.baseline_grid").to_string()
}

pub fn get_menu_label_view_grid_and_axis() -> String {
    text("menu.view.grid_and_axis").to_string()
}

pub fn get_menu_label_view_baseline_grid() -> String {
    text("menu.view.baseline_grid").to_string()
}

pub fn grid_kind_label(kind: GridKind) -> String {
    text(&kind.label_key()).to_string()
}

pub fn grid_query_value(session: &mut Session) -> serde_json::Value {
    session
        .run_named("grid.query", json!({}))
        .ok()
        .and_then(|value| value.get("grid").cloned())
        .unwrap_or_else(|| serde_json::to_value(Grid::default()).unwrap_or(json!({})))
}

pub fn grid_query(session: &mut Session) -> Grid {
    serde_json::from_value(grid_query_value(session)).unwrap_or_else(|_| Grid::default())
}

pub fn origin_from_grid_value(value: &serde_json::Value) -> [f32; 2] {
    pair_from_value(value.get("origin")).unwrap_or([0.0, 0.0])
}

pub fn angles_from_grid_value(value: &serde_json::Value) -> [f32; 2] {
    pair_from_value(value.get("angles")).unwrap_or([30.0, 30.0])
}

fn pair_from_value(value: Option<&serde_json::Value>) -> Option<[f32; 2]> {
    let items = value?.as_array()?;
    if items.len() != 2 {
        return None;
    }
    Some([items[0].as_f64()? as f32, items[1].as_f64()? as f32])
}

pub fn with_origin_angles(mut params: serde_json::Value, origin: [f32; 2], angles: [f32; 2]) -> serde_json::Value {
    params["origin"] = json!(origin);
    params["angles"] = json!(angles);
    params
}

pub fn grid_set(session: &mut Session, params: serde_json::Value) -> bool {
    session.run_named("grid.set", params).is_ok()
}

pub fn grid_set_from(session: &mut Session, grid: &Grid) -> bool {
    let params = serde_json::to_value(dtool_core::grid::GridSetArgs::from_grid(grid)).unwrap_or_default();
    grid_set(session, params)
}

pub fn grid_set_from_dialog(session: &mut Session, grid: &Grid, origin: [f32; 2], angles: [f32; 2]) -> bool {
    let params = serde_json::to_value(dtool_core::grid::GridSetArgs::from_grid(grid)).unwrap_or_default();
    grid_set(session, with_origin_angles(params, origin, angles))
}

pub fn view_show_grid_items(ui: &mut egui::Ui, session: &mut Session) {
    let grid = grid_query(session);
    let mut show = grid.show;
    if dtool_design::toggle(ui, &mut show, get_menu_label_view_show_grid()).clicked() {
        grid_set(session, json!({ "show": show }));
    }
    let mut show_pixel = grid.show_pixel;
    if dtool_design::toggle(ui, &mut show_pixel, get_menu_label_view_show_pixel_grid()).clicked() {
        grid_set(session, json!({ "show_pixel": show_pixel }));
    }
    let mut show_baseline = grid.show_baseline;
    if dtool_design::toggle(ui, &mut show_baseline, get_menu_label_view_show_baseline_grid()).clicked() {
        grid_set(session, json!({ "show_baseline": show_baseline }));
    }
}

pub fn view_grid_dialog_items(
    ui: &mut egui::Ui,
    session: &mut Session,
    grid_and_axis: &mut GridAndAxisDialog,
    baseline: &mut BaselineGridDialog,
) {
    if menu_item(ui, get_menu_label_view_grid_and_axis().as_str()).clicked() {
        grid_and_axis.open_from_session(session);
        ui.close();
    }
    if menu_item(ui, get_menu_label_view_baseline_grid().as_str()).clicked() {
        baseline.open(&grid_query(session));
        ui.close();
    }
}

fn grid_kind_picker(ui: &mut egui::Ui, kind: &mut GridKind) {
    ui.horizontal(|ui| {
        ui.label(text("dialog.grid.kind"));
        for option in GridKind::ALL {
            if ui.selectable_label(*kind == option, grid_kind_label(option)).clicked() {
                *kind = option;
            }
        }
    });
}

#[derive(Debug, Clone)]
pub struct GridAndAxisDialog {
    pub open: bool,
    pub draft: Grid,
    pub origin: [f32; 2],
    pub angles: [f32; 2],
    pub owner: Option<ContextId>,
}

impl Default for GridAndAxisDialog {
    fn default() -> Self {
        Self {
            open: false,
            draft: Grid::default(),
            origin: [0.0, 0.0],
            angles: [30.0, 30.0],
            owner: None,
        }
    }
}

impl GridAndAxisDialog {
    pub fn source(&self) -> &'static str {
        GRID_AND_AXIS_DIALOG_XAML
    }

    pub fn open(&mut self, grid: &Grid) {
        self.open = true;
        self.draft = grid.clone();
        self.origin = grid.origin;
        self.angles = grid.angles;
        self.owner = None;
    }

    pub fn open_value(&mut self, value: &serde_json::Value) {
        let grid = serde_json::from_value(value.clone()).unwrap_or_else(|_| Grid::default());
        self.open(&grid);
        self.origin = origin_from_grid_value(value);
        self.angles = angles_from_grid_value(value);
    }

    pub fn open_from_session(&mut self, session: &mut Session) {
        self.open_value(&grid_query_value(session));
    }

    pub fn open_for(&mut self, owner: ContextId, grid: &Grid) {
        self.open(grid);
        self.owner = Some(owner);
    }

    pub fn apply(&mut self, session: &mut Session) -> bool {
        grid_set_from_dialog(session, &self.draft, self.origin, self.angles)
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) {
        if !self.open {
            return;
        }
        let mut close = false;
        egui::Area::new(egui::Id::new("dtool.dialog.grid_and_axis"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                dialog_frame(ui, |ui| {
                    panel_header(ui, text("menu.view.grid_and_axis"));
                    dtool_design::toggle(ui, &mut self.draft.show, get_menu_label_view_show_grid());
                    grid_kind_picker(ui, &mut self.draft.kind);
                    ui.horizontal(|ui| {
                        ui.label(text("dialog.grid.spacing"));
                        grid_number(ui, "spacing_x", &mut self.draft.spacing[0]);
                        grid_number(ui, "spacing_y", &mut self.draft.spacing[1]);
                    });
                    ui.horizontal(|ui| {
                        ui.label(text("dialog.grid.subdivisions"));
                        let mut value = f64::from(self.draft.subdivisions);
                        grid_value(ui, "subdivisions", &mut value);
                        self.draft.subdivisions = value.max(1.0) as u32;
                    });
                    dtool_design::toggle(ui, &mut self.draft.show_pixel, get_menu_label_view_show_pixel_grid());
                    ui.horizontal(|ui| {
                        optional_label(ui, "dialog.grid.origin");
                        grid_number(ui, "origin_x", &mut self.origin[0]);
                        grid_number(ui, "origin_y", &mut self.origin[1]);
                    });
                    if matches!(self.draft.kind, GridKind::Axonometric | GridKind::Custom) {
                        ui.horizontal(|ui| {
                            optional_label(ui, "dialog.grid.angles");
                            grid_number(ui, "angle_a", &mut self.angles[0]);
                            grid_number(ui, "angle_b", &mut self.angles[1]);
                        });
                    }
                    ui.horizontal(|ui| {
                        if button(ui, text("button.ok")).clicked() && self.apply(session) {
                            close = true;
                        }
                        if button(ui, text("button.cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if close {
            self.open = false;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BaselineGridDialog {
    pub open: bool,
    pub show_baseline: bool,
    pub spacing: f32,
    settings: Settings,
    pub owner: Option<ContextId>,
}

impl BaselineGridDialog {
    pub fn source(&self) -> &'static str {
        BASELINE_GRID_DIALOG_XAML
    }

    pub fn open(&mut self, grid: &Grid) {
        self.open = true;
        self.show_baseline = grid.show_baseline;
        self.spacing = grid.baseline_spacing;
        self.settings = grid.baseline.unwrap_or_else(|| {
            if grid.show_baseline {
                Settings { enabled: true, offset: grid.origin[1], origin: Origin::Document, threshold_percent: 0.0 }
            } else { Settings::default() }
        });
        self.owner = None;
    }

    pub fn open_for(&mut self, owner: ContextId, grid: &Grid) {
        self.open(grid);
        self.owner = Some(owner);
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) {
        if !self.open {
            return;
        }
        let mut close = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let units = dtool_core::commands::ruler_units::suffix(session.document.units);
        egui::Area::new(egui::Id::new("dtool.dialog.baseline_grid"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                dialog_frame(ui, |ui| {
                    panel_header(ui, text("menu.view.baseline_grid"));
                    dtool_design::toggle(ui, &mut self.settings.enabled, text("dialog.grid.baseline_use"));
                    ui.add_enabled_ui(self.settings.enabled, |ui| {
                        let origins = [Origin::Document, Origin::Spread, Origin::Page, Origin::Margin];
                        let keys = ["dialog.grid.baseline_document", "dialog.grid.baseline_spread", "dialog.grid.baseline_page", "dialog.grid.baseline_margin"];
                        let mut selected = origins.iter().position(|origin| *origin == self.settings.origin).unwrap_or(1);
                        ui.horizontal(|ui| {
                            ui.label(text("dialog.grid.baseline_relative"));
                            dtool_design::dropdown(ui, "baseline-origin", &mut selected, &keys.map(text));
                        });
                        self.settings.origin = origins[selected];
                        baseline_number(ui, "dialog.grid.baseline_start", &mut self.settings.offset, units, -1_000_000.0..=1_000_000.0);
                        baseline_number(ui, "dialog.grid.spacing", &mut self.spacing, units, 0.000_001..=1_000_000.0);
                    });
                    dtool_design::toggle(ui, &mut self.show_baseline, get_menu_label_view_show_baseline_grid());
                    baseline_number(ui, "dialog.grid.baseline_threshold", &mut self.settings.threshold_percent, "%", 0.0..=100_000.0);
                    ui.horizontal(|ui| {
                        if button(ui, text("button.ok")).clicked() && grid_set(session, json!({ "show_baseline": self.show_baseline, "baseline_spacing": self.spacing, "baseline": self.settings })) {
                            close = true;
                        }
                        if button(ui, text("button.cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if close {
            self.open = false;
        }
    }
}

fn baseline_number(ui: &mut egui::Ui, key: &str, value: &mut f32, suffix: &str, range: std::ops::RangeInclusive<f64>) {
    ui.horizontal(|ui| {
        ui.label(text(key));
        let id = ui.id().with(key);
        let mut number = f64::from(*value);
        let spec = dtool_design::NumericSpec::new("").range(range).suffix(suffix).decimals(4).width(98.0);
        dtool_design::numeric_value(ui, id, &mut number, &spec);
        *value = number as f32;
    });
}

pub fn isometric_planes_panel(ui: &mut egui::Ui, session: &mut Session) {
    let grid = grid_query(session);
    let mut kind = grid.kind;
    grid_kind_picker(ui, &mut kind);
    if kind != grid.kind {
        grid_set(session, json!({ "kind": kind }));
    }
    let mut show = grid.show;
    if dtool_design::toggle(ui, &mut show, get_menu_label_view_show_grid()).clicked() {
        grid_set(session, json!({ "show": show }));
    }
}

fn grid_value(ui: &mut egui::Ui, salt: &str, value: &mut f64) -> egui::Response {
    let id = egui::Id::NULL.with(("dtool.grid_dialog.number", salt));
    dtool_design::numeric_value(ui, id, value, &dtool_design::NumericSpec::new("").decimals(6))
}

fn optional_label(ui: &mut egui::Ui, key: &str) {
    if dtool_strings::has(key) {
        ui.label(text(key));
    }
}

fn grid_number(ui: &mut egui::Ui, salt: &str, value: &mut f32) {
    let mut number = f64::from(*value);
    grid_value(ui, salt, &mut number);
    *value = number as f32;
}
