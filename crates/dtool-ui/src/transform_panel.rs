use dtool_core::command::Session;
use dtool_core::document::Units;
use dtool_core::transform::{parse_field, Anchor, FlipAxis, RotateDirection, Transform};
use dtool_design::{numeric_field, numeric_request_edit, numeric_state, NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;
use egui::{Response, Ui};

pub const LAYOUT_SOURCE: &str = "ui__studio__transformpage.xaml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformPanelMode {
    Standard,
    Relative,
    Edge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformField {
    X,
    Y,
    W,
    H,
    R,
    S,
    Anchor,
}

pub const FOCUS_ORDER: [TransformField; 7] = [
    TransformField::X,
    TransformField::Y,
    TransformField::W,
    TransformField::H,
    TransformField::R,
    TransformField::S,
    TransformField::Anchor,
];

pub const RELATIVE_NEUTRAL: [f32; 4] = [0.0, 0.0, 100.0, 0.0];

pub fn next_field(current: TransformField, backwards: bool) -> TransformField {
    let index = FOCUS_ORDER.iter().position(|f| *f == current).unwrap_or(0);
    let len = FOCUS_ORDER.len();
    if backwards {
        FOCUS_ORDER[(index + len - 1) % len]
    } else {
        FOCUS_ORDER[(index + 1) % len]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleOverride {
    pub enabled: bool,
    pub line_weights: bool,
    pub shape_corners: bool,
    pub filter_effect: bool,
    pub text_frame: bool,
}

impl Default for ScaleOverride {
    fn default() -> Self {
        Self {
            enabled: false,
            line_weights: true,
            shape_corners: true,
            filter_effect: true,
            text_frame: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PanelEdit {
    Set(serde_json::Value),
    SelectionBox(serde_json::Value),
    Flip(serde_json::Value),
    Rotate90(serde_json::Value),
}

pub struct TransformPanel {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub s: f32,
    pub anchor: Anchor,
    pub aspect_locked: bool,
    pub mixed_x: bool,
    pub mixed_y: bool,
    pub mixed_w: bool,
    pub mixed_h: bool,
    pub mixed_r: bool,
    pub mixed_s: bool,
    pub mode: TransformPanelMode,
    pub relative: [f32; 4],
    pub length: f32,
    pub scale_override: ScaleOverride,
    pub scale_override_open: bool,
    pub locked: bool,
    pub focused: Option<TransformField>,
    pub pending_focus: Option<TransformField>,
    pub anchor_focus_request: bool,
    pub units: Units,
    pub dpi: f32,
    pub error: Option<String>,
    pub edits: Vec<PanelEdit>,
    scrub_origin: Option<(TransformField, f32)>,
    preview_edit: Option<PanelEdit>,
    selection_available: bool,
    selection_length: Option<f32>,
    binding: Option<(dtool_core::ContextId, Vec<dtool_core::ObjectId>)>,
    icons: crate::icon_cache::IconCache,
}

impl Default for TransformPanel {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            r: 0.0,
            s: 0.0,
            anchor: Anchor::TopLeft,
            aspect_locked: false,
            mixed_x: false,
            mixed_y: false,
            mixed_w: false,
            mixed_h: false,
            mixed_r: false,
            mixed_s: false,
            mode: TransformPanelMode::Standard,
            relative: RELATIVE_NEUTRAL,
            length: 0.0,
            scale_override: ScaleOverride::default(),
            scale_override_open: false,
            locked: false,
            focused: None,
            pending_focus: None,
            anchor_focus_request: false,
            units: Units::Px,
            dpi: 72.0,
            error: None,
            edits: Vec::new(),
            scrub_origin: None,
            preview_edit: None,
            selection_available: true,
            selection_length: None,
            binding: None,
            icons: Default::default(),
        }
    }
}

pub fn field_id(field: TransformField) -> egui::Id {
    egui::Id::NULL.with(("dtool.transform_panel", field_key(field)))
}

pub fn relative_field_id(slot: usize) -> egui::Id {
    egui::Id::NULL.with(("dtool.transform_panel.relative", slot))
}

pub fn length_field_id() -> egui::Id {
    egui::Id::NULL.with("dtool.transform_panel.length")
}

fn field_key(field: TransformField) -> &'static str {
    match field {
        TransformField::X => "x",
        TransformField::Y => "y",
        TransformField::W => "w",
        TransformField::H => "h",
        TransformField::R => "r",
        TransformField::S => "s",
        TransformField::Anchor => "anchor",
    }
}

fn anchor_label_key(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::TopLeft => "panel.transform.anchor.top_left",
        Anchor::Top => "panel.transform.anchor.top",
        Anchor::TopRight => "panel.transform.anchor.top_right",
        Anchor::Left => "panel.transform.anchor.left",
        Anchor::Centre => "panel.transform.anchor.centre",
        Anchor::Right => "panel.transform.anchor.right",
        Anchor::BottomLeft => "panel.transform.anchor.bottom_left",
        Anchor::Bottom => "panel.transform.anchor.bottom",
        Anchor::BottomRight => "panel.transform.anchor.bottom_right",
    }
}

fn rounded(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

fn parse_plain(input: &str) -> Option<f32> {
    parse_field(input.trim().trim_end_matches('%').trim_end_matches('\u{b0}'), Units::Px, 72.0).ok()
}

fn parse_document_length(input: &str, units: Units, dpi: f32) -> Option<f32> {
    let pixels = parse_field(input, units, dpi).ok()?;
    dtool_core::units::from_px(pixels, units, dpi)
}

fn panel_width(ui: &Ui) -> f32 {
    ui.available_width().min(ui.clip_rect().right() - ui.cursor().min.x)
}

fn is_plain(field: TransformField) -> bool {
    matches!(field, TransformField::R | TransformField::S)
}

fn scrub_value(ui: &Ui, id: egui::Id) -> Option<f32> {
    let state = numeric_state(ui.ctx(), id);
    (state.scrubbing && state.previewed).then_some(state.scrub as f32)
}

impl TransformPanel {
    pub fn show(&mut self, ui: &mut Ui) -> Response {
        self.draw(ui)
    }

    pub(crate) fn bind_selection(&mut self, ui: &Ui, binding: Option<(dtool_core::ContextId, Vec<dtool_core::ObjectId>)>) {
        if self.binding != binding {
            self.binding = binding;
            self.focused = None;
            self.pending_focus = None;
            self.anchor_focus_request = false;
            self.scrub_origin = None;
            self.selection_length = None;
            self.edits.clear();
            for id in FOCUS_ORDER.into_iter().filter(|field| *field != TransformField::Anchor).map(field_id)
                .chain((0..self.relative.len()).map(relative_field_id)).chain([length_field_id()])
            {
                dtool_design::numeric_cancel_edit(ui.ctx(), id);
            }
            self.relative = RELATIVE_NEUTRAL;
            self.error = None;
        }
        if self.binding.is_none() {
            self.selection_available = false;
            self.locked = false;
        }
    }

    pub fn show_with_session(&mut self, ui: &mut Ui, session: &mut Session) -> Response {
        self.sync_from_session(session);
        let response = self.draw(ui);
        if let Some(command) = self.preview_edit.take().and_then(|edit| self.command_for(session, edit)) {
            crate::app_canvas::stage_geometry_preview(ui, session, command);
        }
        self.apply_edits(session);
        response
    }

    pub fn sync_from_session(&mut self, session: &Session) {
        self.units = session.document.units;
        self.dpi = session.document.dpi;
        self.selection_available = session.selection.ids.iter().any(|id| session.document.get_object(*id).is_some());
        self.locked = !session.selection.ids.is_empty()
            && session
                .selection
                .ids
                .iter()
                .any(|id| session.document.get_object(*id).map_or(false, |o| o.locked));
        if self.focused.is_some() {
            return;
        }
        let ids = session.selection.ids.clone();
        let frac = dtool_core::command::anchor_fraction(self.anchor);
        let rows: Vec<[f32; 6]> = ids
            .iter()
            .filter_map(|id| session.document.get_object(*id))
            .map(|o| {
                let [x, y, w, h] = dtool_core::command::transform_box(&session.document, o);
                [x + w * frac[0], y + h * frac[1], w, h, o.rotation_deg, o.shear_deg]
            })
            .collect();
        self.selection_length = if rows.len() > 1 {
            session.selection_bounds().map(|[_, _, w, h]| w.hypot(h))
        } else {
            None
        };
        let first = match rows.first() {
            Some(first) => *first,
            None => return,
        };
        let mixed = |i: usize| rows.iter().any(|r| (r[i] - first[i]).abs() > 0.0001);
        self.mixed_x = mixed(0);
        self.mixed_y = mixed(1);
        self.mixed_w = mixed(2);
        self.mixed_h = mixed(3);
        self.mixed_r = mixed(4);
        self.mixed_s = mixed(5);
        self.x = rounded(first[0]);
        self.y = rounded(first[1]);
        self.w = rounded(first[2]);
        self.h = rounded(first[3]);
        self.r = rounded(first[4]);
        self.s = rounded(first[5]);
        self.length = rounded((first[2] * first[2] + first[3] * first[3]).sqrt());
    }

    pub fn apply_edits(&mut self, session: &mut Session) {
        let edits: Vec<PanelEdit> = self.edits.drain(..).collect();
        for edit in edits {
            let Some(command) = self.command_for(session, edit) else { continue };
            let result = session.run(&command);
            match result {
                Ok(_) => self.error = None,
                Err(err) => {
                    let key = if err.code == "NOT_IMPLEMENTED" {
                        "error.not_implemented"
                    } else {
                        "error.invalid_params"
                    };
                    self.error = Some(text(key).to_string());
                }
            }
        }
        if self.focused.is_none() {
            self.sync_from_session(session);
        }
    }

    fn command_for(&self, session: &Session, edit: PanelEdit) -> Option<dtool_core::Command> {
        let (name, params) = match edit {
            PanelEdit::Set(params) => {
                if let (Some(angle), Some(origin)) = (params["rotation_deg"].as_f64(), session.selection_transform_origin())
                    && let Some(object) = session.selection.ids.first().and_then(|id| session.document.get_object(*id))
                {
                    let delta = (angle - f64::from(object.rotation_deg)).rem_euclid(360.0);
                    ("transform.selection_box", serde_json::json!({ "origin": origin, "rotation_deg": delta }))
                } else {
                    ("transform.set", params)
                }
            }
            PanelEdit::SelectionBox(mut params) => {
                let bounds = session.selection_bounds()?;
                let frac = dtool_core::command::anchor_fraction(self.anchor);
                let origin = session.selection_transform_origin()
                    .unwrap_or([bounds[0] + bounds[2] * frac[0], bounds[1] + bounds[3] * frac[1]]);
                params["origin"] = serde_json::json!(origin);
                ("transform.selection_box", params)
            }
            PanelEdit::Flip(params) => ("transform.flip", params),
            PanelEdit::Rotate90(params) => ("transform.rotate_90", params),
        };
        Some(dtool_core::Command::with_params(name, params))
    }

    fn draw(&mut self, ui: &mut Ui) -> Response {
        self.preview_edit = None;
        if let Some(field) = self.pending_focus.take() {
            if field == TransformField::Anchor {
                self.anchor_focus_request = true;
            } else {
                numeric_request_edit(ui.ctx(), field_id(field));
            }
        }

        if dtool_design::in_studio(ui) {
            return ui.scope(|ui| self.draw_controls(ui)).response;
        }
        ui.collapsing(text("panel.transform"), |ui| self.draw_controls(ui)).header_response
    }

    fn draw_controls(&mut self, ui: &mut Ui) {
        let can_edit = self.selection_available && !self.locked && ui.is_enabled();
        let visible_width = panel_width(ui);
        ui.horizontal_wrapped(|ui| {
            let modes = [
                (TransformPanelMode::Standard, "panel.transform.mode.standard"),
                (TransformPanelMode::Relative, "panel.transform.mode.relative"),
                (TransformPanelMode::Edge, "panel.transform.mode.edge"),
            ];
            for (mode, key) in modes {
                if dtool_design::tab(ui, self.mode == mode, text(key)).clicked() {
                    self.mode = mode;
                }
            }
            if visible_width < 240.0 {
                ui.end_row();
            }
            ui.add_enabled_ui(can_edit, |ui| {
                if self.icons.studio_button(
                    ui,
                    "panel-scale-options",
                    text("panel.transform.scale_override"),
                    self.scale_override_open,
                ).clicked() {
                    self.scale_override_open = !self.scale_override_open;
                }
            });
            if self.action_icon(ui, "cmd-flip", "tool.transform.flip", can_edit) {
                self.menu_flip(FlipAxis::Horizontal);
            }
            if self.action_icon(ui, "tool-transform-rotate", "menu.document.rotate_90_clockwise", can_edit) {
                self.menu_rotate_90(RotateDirection::Right);
            }
            if self.mode == TransformPanelMode::Standard {
                ui.add_enabled_ui(can_edit, |ui| {
                    let icon = if self.aspect_locked { "panel-aspect-linked" } else { "panel-aspect-unlinked" };
                    if self.icons.studio_button(
                        ui,
                        icon,
                        text("panel.transform.aspect_lock"),
                        self.aspect_locked,
                    ).clicked() {
                        self.aspect_locked = !self.aspect_locked;
                    }
                });
            }
        });

        if self.locked {
            ui.label(text("panel.transform.locked"));
        }

        ui.add_enabled_ui(can_edit, |ui| {
            match self.mode {
                TransformPanelMode::Standard => self.draw_standard(ui),
                TransformPanelMode::Relative => self.draw_relative(ui),
                TransformPanelMode::Edge => self.draw_edge(ui),
            }
            if self.scale_override_open {
                self.draw_scale_override(ui);
            }
        });

        if let Some(err) = &self.error {
            ui.label(err.as_str());
        }
    }

    fn action_icon(&mut self, ui: &mut Ui, icon: &str, label_key: &str, enabled: bool) -> bool {
        let label = text(label_key);
        let state = dtool_design::ControlState::enabled(enabled).tooltip(label);
        let response = match self.icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)) {
            Some(texture) => dtool_design::icon_image_button(ui, &texture, &state),
            None => dtool_design::button_with(ui, label, &state),
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
        response.clicked()
    }

    pub fn value(&self, field: TransformField) -> Option<f32> {
        match field {
            TransformField::X => Some(self.x),
            TransformField::Y => Some(self.y),
            TransformField::W => Some(self.w),
            TransformField::H => Some(self.h),
            TransformField::R => Some(self.r),
            TransformField::S => Some(self.s),
            TransformField::Anchor => None,
        }
    }

    pub fn is_mixed(&self, field: TransformField) -> bool {
        match field {
            TransformField::X => self.mixed_x,
            TransformField::Y => self.mixed_y,
            TransformField::W => self.mixed_w,
            TransformField::H => self.mixed_h,
            TransformField::R => self.mixed_r,
            TransformField::S => self.mixed_s,
            TransformField::Anchor => false,
        }
    }

    fn set_value(&mut self, field: TransformField, value: f32) {
        match field {
            TransformField::X => self.x = value,
            TransformField::Y => self.y = value,
            TransformField::W => self.w = value,
            TransformField::H => self.h = value,
            TransformField::R => self.r = value,
            TransformField::S => self.s = value,
            TransformField::Anchor => {}
        }
    }

    fn track_focus(&mut self, ui: &Ui, field: TransformField, id: egui::Id) {
        let state = numeric_state(ui.ctx(), id);
        let active = state.buffer.is_some() || state.scrubbing;
        let (tab, shift) = ui.input(|i| (i.key_pressed(egui::Key::Tab), i.modifiers.shift));
        let was_focused = self.focused == Some(field);
        if active {
            self.focused = Some(field);
            if state.invalid {
                self.error = Some(text("error.invalid_params").to_string());
            }
        }
        if tab && (active || was_focused) && field != TransformField::Anchor && self.pending_focus.is_none() {
            let next = next_field(field, shift);
            self.focused = Some(next);
            self.pending_focus = Some(next);
        } else if !active && was_focused && self.pending_focus != Some(field) {
            self.focused = None;
        }
    }

    pub fn handle_event(&mut self, field: TransformField, event: NumericEvent) {
        match event {
            NumericEvent::None => {}
            NumericEvent::Preview(v) => {
                if self.scrub_origin.is_none() {
                    if let Some(origin) = self.value(field) {
                        self.scrub_origin = Some((field, origin));
                    }
                }
                self.set_value(field, v as f32);
            }
            NumericEvent::Commit(v) => {
                self.scrub_origin = None;
                self.set_value(field, v as f32);
                self.commit_value(field, v as f32);
            }
            NumericEvent::Cancel => {
                if let Some((origin_field, origin)) = self.scrub_origin.take() {
                    self.set_value(origin_field, origin);
                }
            }
        }
    }

    fn field_row(&mut self, ui: &mut Ui, field: TransformField, label_key: &str, width: f32, label_width: Option<f32>) {
        let id = field_id(field);
        let label = text(label_key).to_string();
        let mixed_hint = if self.selection_available { text("panel.transform.mixed") } else { dtool_design::MIXED_TEXT };
        let (units, dpi) = (self.units, self.dpi);
        let parse = move |input: &str| -> Option<f64> {
            if is_plain(field) { parse_plain(input) } else { parse_document_length(input, units, dpi) }.map(f64::from)
        };
        let suffix = if is_plain(field) { "°" } else { dtool_core::commands::ruler_units::suffix(units) };
        let mut spec = NumericSpec::new(&label).decimals(3).mixed_hint(mixed_hint).parser(&parse)
            .width(width).suffix(suffix).enabled(ui.is_enabled());
        if let Some(width) = label_width {
            spec = spec.label_width(width);
        }
        if is_plain(field) {
            spec = spec.reset_to(0.0);
        }
        let value = if !self.selection_available || self.is_mixed(field) {
            NumericValue::Mixed
        } else {
            NumericValue::Value(f64::from(self.value(field).unwrap_or(0.0)))
        };
        let (_, event) = numeric_field(ui, id, value, &spec);
        self.handle_event(field, event);
        if ui.is_enabled() && let Some(value) = scrub_value(ui, id) {
            self.preview_edit = self.set_edit(field, value);
        }
        self.track_focus(ui, field, id);
    }

    fn draw_standard(&mut self, ui: &mut Ui) {
        let visible_width = panel_width(ui);
        if visible_width >= 224.0 {
            ui.horizontal_top(|ui| {
                self.draw_anchor(ui);
                self.draw_standard_fields(ui, visible_width - 40.0);
            });
        } else {
            self.draw_anchor(ui);
            self.draw_standard_fields(ui, visible_width);
        }
    }

    fn draw_standard_fields(&mut self, ui: &mut Ui, width: f32) {
        let two_columns = dtool_design::panel_column_count(ui, width, 2, 62.0) == 2;
        let field_width = if two_columns {
            ((width - 44.0) / 2.0).min(88.0)
        } else {
            (width - 20.0).clamp(30.0, 88.0)
        };
        let fields = [
            [(TransformField::X, "panel.transform.x"), (TransformField::W, "panel.transform.w")],
            [(TransformField::Y, "panel.transform.y"), (TransformField::H, "panel.transform.h")],
            [(TransformField::R, "panel.transform.r"), (TransformField::S, "panel.transform.s")],
        ];
        egui::Grid::new("transform.fields").show(ui, |ui| {
            for pair in fields {
                for (field, key) in pair {
                    self.field_row(ui, field, key, field_width, None);
                    if !two_columns { ui.end_row(); }
                }
                if two_columns { ui.end_row(); }
            }
        });
    }

    fn mode_field_widths(ui: &Ui) -> (f32, f32) {
        let width = panel_width(ui);
        let label_width = (width * 0.46).clamp(40.0, 106.0);
        let field_width = (width - label_width - ui.spacing().item_spacing.x - 2.0).clamp(34.0, 88.0);
        (label_width, field_width)
    }

    fn draw_relative(&mut self, ui: &mut Ui) {
        let rows = [
            ("panel.transform.relative_x", 0usize),
            ("panel.transform.relative_y", 1usize),
            ("panel.transform.scale", 2usize),
            ("panel.transform.relative_rotation", 3usize),
        ];
        let (label_width, field_width) = Self::mode_field_widths(ui);
        for (key, slot) in rows {
            let label = text(key).to_string();
            let (units, dpi) = (self.units, self.dpi);
            let parse = move |input: &str| -> Option<f64> {
                if slot < 2 { parse_document_length(input, units, dpi) } else { parse_plain(input) }.map(f64::from)
            };
            let suffix = match slot {
                0 | 1 => dtool_core::commands::ruler_units::suffix(units),
                2 => "%",
                _ => "°",
            };
            let spec = NumericSpec::new(&label).decimals(3).parser(&parse)
                .width(field_width).label_width(label_width).suffix(suffix).enabled(ui.is_enabled())
                .reset_to(f64::from(RELATIVE_NEUTRAL[slot]));
            let id = relative_field_id(slot);
            let (_, event) = numeric_field(ui, id, NumericValue::Value(f64::from(self.relative[slot])), &spec);
            match event {
                NumericEvent::Preview(v) => self.relative[slot] = v as f32,
                NumericEvent::Commit(v) => self.commit_relative(slot, v as f32),
                NumericEvent::Cancel => self.relative[slot] = RELATIVE_NEUTRAL[slot],
                NumericEvent::None => {}
            }
            if ui.is_enabled() && let Some(value) = scrub_value(ui, id) {
                self.preview_edit = Self::relative_edit(slot, value);
            }
        }
        self.draw_anchor(ui);
    }

    fn draw_edge(&mut self, ui: &mut Ui) {
        let (label_width, field_width) = Self::mode_field_widths(ui);
        self.field_row(ui, TransformField::X, "panel.transform.x", field_width, Some(label_width));
        self.field_row(ui, TransformField::Y, "panel.transform.y", field_width, Some(label_width));
        let label = text(if self.selection_length.is_some() {
            "panel.transform.selection_length"
        } else {
            "panel.transform.length"
        });
        let (units, dpi) = (self.units, self.dpi);
        let parse = move |input: &str| -> Option<f64> { parse_document_length(input, units, dpi).map(f64::from) };
        let spec = NumericSpec::new(label).decimals(3).parser(&parse)
            .width(field_width).label_width(label_width).enabled(ui.is_enabled())
            .suffix(dtool_core::commands::ruler_units::suffix(units));
        let length = self.selection_length.unwrap_or(self.length);
        let (_, event) = numeric_field(ui, length_field_id(), NumericValue::Value(f64::from(length)), &spec);
        if let NumericEvent::Commit(v) = event {
            self.commit_length(v as f32);
        }
        if ui.is_enabled() && let Some(value) = scrub_value(ui, length_field_id()) {
            self.preview_edit = self.length_edit(value);
        }
        self.field_row(ui, TransformField::R, "panel.transform.r", field_width, Some(label_width));
        self.draw_anchor(ui);
    }

    fn draw_anchor(&mut self, ui: &mut Ui) {
        let anchors = [
            [Anchor::TopLeft, Anchor::Top, Anchor::TopRight],
            [Anchor::Left, Anchor::Centre, Anchor::Right],
            [Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight],
        ];
        let mut focused_selected: Option<Response> = None;
        ui.vertical(|ui| {
            let surface = ui.painter().clone();
            let start = ui.cursor().min;
            let plate = egui::Rect::from_min_size(start + egui::vec2(4.0, 4.0), egui::Vec2::splat(25.0));
            let fill = ui.visuals().widgets.inactive.bg_fill;
            surface.rect_filled(plate, 1.0, fill);
            ui.spacing_mut().item_spacing = egui::vec2(1.0, 1.0);
            for row in anchors.iter() {
                ui.horizontal(|ui| {
                    for &anchor in row.iter() {
                        let selected = self.anchor == anchor;
                        let name = text(anchor_label_key(anchor));
                        let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(9.0), egui::Sense::click());
                        let marker = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(6.0));
                        let fill = if selected {
                            ui.visuals().selection.stroke.color
                        } else if response.hovered() || response.has_focus() {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        ui.painter().rect_filled(marker, 0.0, fill);
                        let response = response.on_hover_text(name);
                        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, name));
                        if response.clicked() {
                            self.anchor = anchor;
                        }
                        if selected {
                            if self.anchor_focus_request {
                                response.request_focus();
                                self.anchor_focus_request = false;
                            }
                            focused_selected = Some(response);
                        }
                    }
                });
            }
        });
        if let Some(response) = focused_selected {
            if response.has_focus() {
                self.focused = Some(TransformField::Anchor);
                let (tab, shift) = ui.input_mut(|i| {
                    let shift = i.modifiers.shift;
                    let tab = i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
                    (tab, shift)
                });
                if tab {
                    let next = next_field(TransformField::Anchor, shift);
                    self.focused = Some(next);
                    self.pending_focus = Some(next);
                }
            } else if self.focused == Some(TransformField::Anchor) {
                self.focused = None;
            }
        }
    }

    fn draw_scale_override(&mut self, ui: &mut Ui) {
        dtool_design::panel_header(ui, text("panel.transform.scale_override"));
        dtool_design::toggle(ui, &mut self.scale_override.enabled, text("panel.transform.scale_with_object"));
        ui.add_enabled_ui(self.scale_override.enabled, |ui| {
            dtool_design::toggle(ui, &mut self.scale_override.line_weights, text("panel.transform.line_weights"));
            dtool_design::toggle(ui, &mut self.scale_override.shape_corners, text("panel.transform.shape_corners"));
            dtool_design::toggle(ui, &mut self.scale_override.filter_effect, text("panel.transform.filter_effect"));
            dtool_design::toggle(ui, &mut self.scale_override.text_frame, text("panel.transform.text_frame"));
        });
    }

    fn anchor_json(&self) -> serde_json::Value {
        serde_json::to_value(self.anchor).unwrap_or_default()
    }

    pub fn commit_value(&mut self, field: TransformField, value: f32) {
        if !value.is_finite() {
            self.error = Some(text("error.invalid_params").to_string());
        } else if let Some(edit) = self.set_edit(field, value) {
            self.edits.push(edit);
        }
    }

    fn set_edit(&self, field: TransformField, value: f32) -> Option<PanelEdit> {
        let key = match field {
            TransformField::X => "x",
            TransformField::Y => "y",
            TransformField::W => "w",
            TransformField::H => "h",
            TransformField::R => "rotation_deg",
            TransformField::S => "shear_deg",
            TransformField::Anchor => return None,
        };
        let mut params = serde_json::json!({
            "anchor": self.anchor_json(),
            "aspect_locked": self.aspect_locked
        });
        params[key] = serde_json::json!(value);
        Some(PanelEdit::Set(params))
    }

    pub fn commit_relative(&mut self, slot: usize, value: f32) {
        let slot = slot.min(3);
        self.relative[slot] = RELATIVE_NEUTRAL[slot];
        if !value.is_finite() {
            self.error = Some(text("error.invalid_params").to_string());
        } else if let Some(edit) = Self::relative_edit(slot, value) {
            self.edits.push(edit);
        }
    }

    fn relative_edit(slot: usize, value: f32) -> Option<PanelEdit> {
        let params = match slot {
            0 if value != 0.0 => serde_json::json!({"translate": [value, 0.0]}),
            1 if value != 0.0 => serde_json::json!({"translate": [0.0, value]}),
            2 if value > 0.0 && value != 100.0 => {
                let scale = value / 100.0;
                serde_json::json!({"scale": [scale, scale]})
            }
            3 if value != 0.0 => serde_json::json!({"rotation_deg": value}),
            _ => return None,
        };
        Some(PanelEdit::SelectionBox(params))
    }

    pub fn menu_flip(&mut self, axis: FlipAxis) {
        self.edits.push(PanelEdit::Flip(serde_json::json!({
            "axis": axis,
            "anchor": self.anchor_json()
        })));
    }

    pub fn menu_rotate_90(&mut self, direction: RotateDirection) {
        self.edits.push(PanelEdit::Rotate90(serde_json::json!({
            "direction": direction,
            "anchor": self.anchor_json()
        })));
    }

    pub fn query_command(&self) -> dtool_protocol::Command {
        dtool_protocol::Command::with_params("transform.query", serde_json::json!({ "anchor": self.anchor_json() }))
    }

    pub fn commit_length(&mut self, length: f32) {
        if !(length.is_finite() && length > 0.0) {
            self.error = Some(text("error.invalid_params").to_string());
        } else if let Some(edit) = self.length_edit(length) {
            self.edits.push(edit);
        }
    }

    fn length_edit(&self, length: f32) -> Option<PanelEdit> {
        let current = self.selection_length.unwrap_or_else(|| self.w.hypot(self.h));
        let scale = length / current;
        if !scale.is_finite() || scale <= 0.0 || (scale - 1.0).abs() < 1e-6 {
            return None;
        }
        if self.selection_length.is_some() {
            return Some(PanelEdit::SelectionBox(serde_json::json!({ "scale": [scale, scale] })));
        }
        Some(PanelEdit::Set(serde_json::json!({
            "w": self.w * scale, "h": self.h * scale, "anchor": self.anchor_json()
        })))
    }

    pub fn get_transform(&self) -> Transform {
        Transform {
            pos: [self.x, self.y],
            size: [self.w, self.h],
            rotation_deg: self.r,
            shear_deg: self.s,
            anchor: self.anchor,
            aspect_locked: self.aspect_locked,
        }
    }
}
