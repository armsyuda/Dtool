use egui::{Response, Ui, Vec2, WidgetText};

use crate::theme::surface;
use crate::tokens::{color, font_size, radius, size, space, stroke_width};

mod numeric_field;
mod parts;

pub use numeric_field::{format_number, numeric_area_id, numeric_cancel_edit, numeric_caption, numeric_field, numeric_request_edit, numeric_state, numeric_value, number_drag, number_drag_with, NumericEvent, NumericSpec, NumericState, NumericValue};
pub use parts::{frameless_button, frameless_button_with, list_row, list_row_with, preview_list_row, small_icon_button, small_icon_button_side, small_icon_button_with};

fn corner(r: f32) -> egui::CornerRadius {
    egui::CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}

fn panel_stroke() -> egui::Stroke {
    egui::Stroke::new(stroke_width("thin"), color("border"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlState {
    pub enabled: bool,
    pub mixed: bool,
    pub tooltip: Option<String>,
    pub disabled_reason: Option<String>,
}

impl Default for ControlState {
    fn default() -> Self {
        Self { enabled: true, mixed: false, tooltip: None, disabled_reason: None }
    }
}

impl ControlState {
    pub fn enabled(enabled: bool) -> Self {
        Self { enabled, ..Self::default() }
    }

    pub fn disabled(reason: impl Into<String>) -> Self {
        Self { enabled: false, disabled_reason: Some(reason.into()), ..Self::default() }
    }

    pub fn mixed(mut self, mixed: bool) -> Self {
        self.mixed = mixed;
        self
    }

    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn reason(mut self, text: impl Into<String>) -> Self {
        self.disabled_reason = Some(text.into());
        self
    }

    pub fn decorate(&self, mut response: Response) -> Response {
        if let Some(text) = &self.tooltip {
            response = response.on_hover_text(text.clone());
        }
        match (&self.disabled_reason, &self.tooltip) {
            (Some(reason), _) => response.on_disabled_hover_text(reason.clone()),
            (None, Some(text)) => response.on_disabled_hover_text(text.clone()),
            (None, None) => response,
        }
    }
}

pub const MIXED_TEXT: &str = "\u{2014}";

pub fn button_visuals(ui: &mut Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = color("button.bg");
    widgets.inactive.bg_fill = color("button.bg");
    widgets.hovered.weak_bg_fill = color("button.bg_hover");
    widgets.hovered.bg_fill = color("button.bg_hover");
    widgets.active.weak_bg_fill = color("button.bg_pressed");
    widgets.active.bg_fill = color("button.bg_pressed");
}

fn add_button(ui: &mut Ui, button: egui::Button<'_>, state: &ControlState) -> Response {
    let response = ui
        .scope(|ui| {
            button_visuals(ui);
            ui.add_enabled(state.enabled, button)
        })
        .inner;
    state.decorate(response)
}

pub fn button(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    button_with(ui, text, &ControlState::default())
}

pub fn primary_button(ui: &mut Ui, label: &str, minimum: Vec2) -> Response {
    let selection = ui.visuals().selection;
    ui.add(egui::Button::new(egui::RichText::new(label).strong().color(selection.stroke.color))
        .fill(selection.bg_fill)
        .min_size(minimum))
}

pub fn button_with(ui: &mut Ui, text: impl Into<WidgetText>, state: &ControlState) -> Response {
    let button = egui::Button::new(text)
        .corner_radius(corner(size("radius.button")))
        .min_size(Vec2::new(size("button.width"), size("button.height")));
    add_button(ui, button, state)
}

pub fn icon_button(ui: &mut Ui, icon: impl Into<WidgetText>) -> Response {
    icon_button_with(ui, icon, &ControlState::default())
}

pub fn icon_button_with(ui: &mut Ui, icon: impl Into<WidgetText>, state: &ControlState) -> Response {
    let button = egui::Button::new(icon)
        .corner_radius(corner(size("radius.button")))
        .min_size(Vec2::splat(size("control.hit_size")));
    add_button(ui, button, state)
}

pub fn icon_image_button(ui: &mut Ui, texture: &egui::TextureHandle, state: &ControlState) -> Response {
    icon_image_button_selected(ui, texture, false, state)
}

pub fn selected_icon_ink(ui: &Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { crate::theme::dark().text } else { crate::theme::light().text }
}

pub fn with_icon_selection<R>(ui: &mut Ui, draw: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        let ink = selected_icon_ink(ui);
        let fill = color("control.selected");
        let selection = &mut ui.visuals_mut().selection;
        selection.bg_fill = fill;
        selection.stroke.color = ink;
        draw(ui)
    }).inner
}

pub fn with_dialog_choice<R>(ui: &mut Ui, draw: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        let fill = surface(ui.ctx(), "dialog.choice");
        let ink = selected_icon_ink(ui);
        ui.visuals_mut().selection = egui::style::Selection { bg_fill: fill, stroke: egui::Stroke::new(stroke_width("thin"), ink) };
        draw(ui)
    }).inner
}

/// Use the dark control selection for text choices, preserving the document accent.
pub fn choice_value<T: PartialEq>(ui: &mut Ui, value: &mut T, choice: T, label: impl Into<WidgetText>) -> Response {
    with_icon_selection(ui, |ui| ui.selectable_value(value, choice, label))
}

pub fn icon_image_button_selected(ui: &mut Ui, texture: &egui::TextureHandle, selected: bool, state: &ControlState) -> Response {
    let side = size("icon.size.md");
    let image = egui::Image::new(texture).fit_to_exact_size(Vec2::splat(side));
    let button = egui::Button::image(image).selected(selected)
        .corner_radius(corner(size("radius.button")))
        .min_size(Vec2::splat(size("control.hit_size")));
    with_icon_selection(ui, |ui| {
        let response = add_button(ui, button, state);
        if selected {
            let stroke = if response.hovered() && !response.has_focus() {
                egui::Stroke::new(stroke_width("thin"), crate::theme::widget_token("focus").unwrap_or(ui.visuals().selection.bg_fill))
            } else {
                ui.style().interact(&response).bg_stroke
            };
            ui.painter().rect_stroke(response.rect, corner(size("radius.button")), stroke, egui::StrokeKind::Inside);
        }
        response
    })
}

pub fn studio_icon_button(ui: &mut Ui, button: egui::Button<'_>, width: f32, selected: bool) -> Response {
    with_icon_selection(ui, |ui| {
        button_visuals(ui);
        ui.add_sized(
            Vec2::new(width, size("control.icon.height")),
            button.selected(selected).corner_radius(corner(size("radius.button"))),
        )
    })
}

pub fn icon_texture_px(ui: &Ui) -> u32 {
    (size("icon.size.md") * ui.ctx().pixels_per_point()).ceil().max(1.0) as u32
}

pub fn toggle(ui: &mut Ui, on: &mut bool, label: impl Into<WidgetText>) -> Response {
    switch(ui, on, label.into(), false, false)
}

pub fn menu_toggle(ui: &mut Ui, on: &mut bool, label: impl Into<WidgetText>) -> Response {
    switch(ui, on, label.into(), false, true)
}

fn switch(ui: &mut Ui, on: &mut bool, label: WidgetText, mixed: bool, menu: bool) -> Response {
    let track_size = Vec2::new(size("control.switch.width"), size("control.switch.height"));
    let gap = if label.is_empty() { 0.0 } else { ui.spacing().icon_spacing.max(6.0) };
    let padding = if menu { ui.spacing().button_padding.x } else { 0.0 };
    let name = label.text().to_owned();
    let label_width = (ui.available_width() - track_size.x - gap - 2.0 * padding).max(0.0);
    let label = label.into_galley(ui, None, label_width, egui::TextStyle::Button);
    let desired = if menu {
        Vec2::new(ui.available_width(), label.size().y.max(size("menu.row.height")))
    } else {
        Vec2::new(track_size.x + gap + label.size().x, label.size().y.max(size("control.hit_size")))
    };
    let (rect, mut response) = ui.allocate_exact_size(desired, egui::Sense::click());
    if response.clicked() {
        *on = mixed || !*on;
        response.mark_changed();
    }
    let mixed = mixed && !response.changed();
    response.widget_info(|| {
        if mixed {
            egui::WidgetInfo::labeled(egui::Role::CheckBox, ui.is_enabled(), &name)
        } else {
            egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *on, &name)
        }
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        if menu && (response.hovered() || response.has_focus()) {
            ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        }
        let track_left = if menu { rect.right() - padding - track_size.x } else { rect.left() };
        let track = egui::Rect::from_center_size(
            egui::pos2(track_left + track_size.x * 0.5, rect.center().y), track_size,
        );
        let active = *on && !mixed;
        let fill = if !response.enabled() {
            color("input.disabled")
        } else if response.is_pointer_button_down_on() {
            color("button.bg_pressed")
        } else if active {
            ui.visuals().selection.bg_fill
        } else if response.hovered() {
            color("control.selected_hover")
        } else {
            ui.visuals().extreme_bg_color
        };
        let stroke = if response.has_focus() || response.hovered() {
            visuals.fg_stroke
        } else {
            egui::Stroke::new(1.0, visuals.fg_stroke.color.gamma_multiply(0.58))
        };
        let inset = track.height() * 0.5;
        ui.painter().rect(track, corner(inset), fill, stroke, egui::StrokeKind::Inside);
        let x = if mixed { track.center().x } else if active { track.right() - inset } else { track.left() + inset };
        let knob = if active && response.enabled() { ui.visuals().selection.stroke.color } else { visuals.fg_stroke.color };
        ui.painter().circle_filled(egui::pos2(x, track.center().y), size("control.switch.knob_radius"), knob);
        if mixed {
            let center = egui::pos2(x, track.center().y);
            ui.painter().line_segment([center - Vec2::new(2.0, 0.0), center + Vec2::new(2.0, 0.0)], egui::Stroke::new(1.0, fill));
        }
        let text_x = if menu { rect.left() + padding } else { track.right() + gap };
        let text_pos = egui::pos2(text_x, rect.center().y - label.size().y * 0.5);
        ui.painter().galley(text_pos, label, visuals.text_color());
    }
    response
}

/// Small studio state controls use drawn paths, avoiding font-specific glyphs.
pub fn visibility_toggle(ui: &mut Ui, visible: &mut bool) -> Response {
    state_icon(ui, visible, false)
}

pub fn lock_toggle(ui: &mut Ui, locked: &mut bool) -> Response {
    state_icon(ui, locked, true)
}

fn state_icon(ui: &mut Ui, value: &mut bool, lock: bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *value, if lock { "Lock" } else { "Visible" }));
    if response.clicked() { *value = !*value; response.mark_changed(); }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let c = rect.center();
        let foreground = color("icon");
        let inactive = !response.enabled() || (!*value && !response.hovered() && !response.has_focus());
        let ink = if inactive { foreground.gamma_multiply(0.62) } else { foreground };
        let stroke = egui::Stroke::new(1.25, ink);
        let point = |x, y| c + Vec2::new(x, y);
        if response.hovered() || response.has_focus() {
            painter.rect_filled(rect, 2.0, color("widget.bg_hover"));
        }
        if lock {
            let body = egui::Rect::from_center_size(point(0.0, 2.5), Vec2::new(9.0, 7.0));
            let shackle_tip = if *value { point(2.8, -0.4) } else { point(3.6, -2.4) };
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [point(-2.8, -0.4), point(-4.0, -7.0), point(3.8, -7.0), shackle_tip],
                false,
                egui::Color32::TRANSPARENT,
                stroke,
            ));
            if *value {
                painter.rect_filled(body, 1.2, ink);
                painter.circle_filled(point(0.0, 2.4), 0.95, color("widget.bg"));
            } else {
                painter.rect_stroke(body, 1.2, stroke, egui::StrokeKind::Inside);
            }
        } else {
            let left = point(-6.4, 0.0);
            let right = point(6.4, 0.0);
            for y in [-5.5, 5.5] {
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [left, point(-2.5, y), point(2.5, y), right],
                    false,
                    egui::Color32::TRANSPARENT,
                    stroke,
                ));
            }
            if *value {
                painter.circle_filled(c, 1.85, ink);
            } else {
                painter.line_segment([point(-5.5, 5.5), point(5.5, -5.5)], stroke);
            }
        }
    }
    response
}

pub fn toggle_with(ui: &mut Ui, on: &mut bool, label: impl Into<WidgetText>, state: &ControlState) -> Response {
    let response = ui.add_enabled_ui(state.enabled, |ui| switch(ui, on, label.into(), state.mixed, false)).inner;
    state.decorate(response)
}

pub fn slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, label: &str) -> Response {
    slider_with(ui, value, range, label, &ControlState::default())
}

pub fn slider_with(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, label: &str, state: &ControlState) -> Response {
    let id = ui.auto_id_with(("dtool.slider", label));
    let mut response = ui.add_enabled_ui(state.enabled, |ui| ui.horizontal(|ui| {
        if !label.is_empty() { ui.label(label); }
        let mut shown = if state.mixed { *range.start() } else { *value };
        let field_width = size("control.slider.field_width");
        let rail_width = (ui.available_width() - field_width - ui.spacing().item_spacing.x)
            .clamp(size("control.slider.min_width"), 220.0);
        let rail = slider_rail(ui, &mut shown, range.clone(), rail_width, label, state.enabled);
        if rail.changed() { *value = shown; }
        let spec = NumericSpec::new("").range(f64::from(*range.start())..=f64::from(*range.end()))
            .width(field_width).decimals(2).enabled(state.enabled);
        let display = if state.mixed && !rail.changed() { NumericValue::Mixed } else { NumericValue::Value(f64::from(*value)) };
        let (field, event) = numeric_field(ui, id.with("value"), display, &spec);
        if let NumericEvent::Commit(next) | NumericEvent::Preview(next) = event { *value = next as f32; }
        let changed = rail.changed() || field.changed();
        let mut response = rail.union(field);
        if changed { response.mark_changed(); }
        response
    })).inner.inner;
    if !state.enabled { response = response.on_disabled_hover_text(state.disabled_reason.clone().unwrap_or_default()); }

    state.decorate(response)
}

pub fn slider_rail(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, width: f32, label: &str, enabled: bool) -> Response {
    draw_slider_rail(ui, value, range, width, label, enabled, None)
}

pub fn logarithmic_slider_rail(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, width: f32, label: &str, enabled: bool, smallest_positive: f64) -> Response {
    draw_slider_rail(ui, value, range, width, label, enabled, Some(smallest_positive))
}

fn draw_slider_rail(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, width: f32, label: &str, enabled: bool, logarithmic_minimum: Option<f64>) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = width;
        ui.spacing_mut().interact_size.y = size("control.hit_size");
        let accent = ui.visuals().selection.bg_fill;
        let visuals = ui.visuals_mut();
        visuals.widgets.inactive.bg_fill = color("control.selected");
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, accent);
        visuals.widgets.hovered.bg_fill = accent;
        visuals.widgets.active.bg_fill = accent;
        let mut slider = egui::Slider::new(value, range)
            .show_value(false).trailing_fill(true)
            .handle_shape(egui::style::HandleShape::Circle);
        if let Some(minimum) = logarithmic_minimum {
            slider = slider.logarithmic(true).smallest_positive(minimum);
        }
        let response = ui.add_enabled(enabled, slider);
        response.widget_info(|| egui::WidgetInfo::slider(enabled && ui.is_enabled(), f64::from(*value), label));
        response
    }).inner
}

pub fn dropdown_value<T: PartialEq + Clone>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, &str)], state: &ControlState) -> Response {
    let shown = if state.mixed {
        MIXED_TEXT.to_string()
    } else {
        options.iter().find(|(v, _)| v == value).map(|(_, label)| label.to_string()).unwrap_or_default()
    };
    let inner = ui.add_enabled_ui(state.enabled, |ui| {
        let mut picked = None;
        let response = egui::ComboBox::from_id_salt(id).selected_text(shown).show_ui(ui, |ui| {
            with_icon_selection(ui, |ui| {
                for (option, label) in options {
                    let selected = !state.mixed && option == value;
                    if ui.selectable_label(selected, *label).clicked() {
                        picked = Some(option.clone());
                    }
                }
            });
        });
        (response.response, picked)
    });
    let (mut response, picked) = inner.inner;
    if let Some(next) = picked {
        if state.mixed || next != *value {
            *value = next;
            response.mark_changed();
        }
    }
    state.decorate(response)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WellValue {
    Colour(egui::Color32),
    None,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKey {
    Confirm,
    Cancel,
}

fn multiline_key() -> egui::Id {
    egui::Id::NULL.with("dtool.design.multiline_focus")
}

pub fn mark_multiline(ui: &Ui, response: &Response) {
    if response.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(multiline_key(), response.id));
    }
}

pub fn multiline_focused(ui: &Ui) -> bool {
    let stored: Option<egui::Id> = ui.ctx().data(|d| d.get_temp(multiline_key()));
    let focused = ui.ctx().memory(|m| m.focused());
    stored.is_some() && stored == focused
}

pub fn dialog_keys(ui: &Ui) -> Option<DialogKey> {
    let multiline = multiline_focused(ui);
    ui.input(|i| {
        if i.key_pressed(egui::Key::Escape) {
            Some(DialogKey::Cancel)
        } else if !multiline && i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if !modifiers.alt && !i.modifiers.alt)) {
            Some(DialogKey::Confirm)
        } else {
            None
        }
    })
}

pub fn number_field(ui: &mut Ui, value: &mut f64, label: &str) -> Response {
    let id = ui.auto_id_with(("dtool.number_field", label));
    numeric_value(ui, id, value, &NumericSpec::new(label))
}

pub fn dropdown(
    ui: &mut Ui,
    id: &str,
    selected: &mut usize,
    options: &[&str],
) -> Response {
    let shown = options.get(*selected).copied().unwrap_or("");
    let before = *selected;
    let mut response = egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            with_icon_selection(ui, |ui| {
                for (index, label) in options.iter().enumerate() {
                    ui.selectable_value(selected, index, *label);
                }
            });
        })
        .response;
    if *selected != before {
        response.mark_changed();
    }
    response
}

pub fn colour_well(ui: &mut Ui, fill: egui::Color32, id: &str) -> Response {
    colour_well_with(ui, WellValue::Colour(fill), id, &ControlState::default())
}

pub fn colour_well_with(ui: &mut Ui, value: WellValue, id: &str, state: &ControlState) -> Response {
    colour_well_sized(ui, value, id, state, Vec2::splat(size("icon.size.lg")), false)
}

pub fn colour_well_sized(ui: &mut Ui, value: WellValue, id: &str, state: &ControlState, dimensions: Vec2, outline: bool) -> Response {
    let (rect, _) = ui.allocate_exact_size(dimensions, egui::Sense::hover());
    let sense = if state.enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let response = ui.interact(rect, ui.make_persistent_id(("colour_well", id)), sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let border = if response.has_focus() {
            egui::Stroke::new(stroke_width("md").max(1.0), ui.visuals().selection.bg_fill)
        } else {
            egui::Stroke::new(stroke_width("thin"), color("well.border"))
        };
        let inner = rect.shrink(1.0);
        match value {
            WellValue::Colour(fill) if outline => {
                painter.rect_filled(inner, 0.0, color("panel.bg"));
                let sample = inner.shrink(3.0);
                painter.rect_stroke(sample, corner(radius("sm")), egui::Stroke::new(3.0, fill), egui::StrokeKind::Inside);
            }
            WellValue::Colour(fill) => {
                if fill.a() < 255 {
                    let half = inner.size() * 0.5;
                    painter.rect_filled(inner, 0.0, egui::Color32::WHITE);
                    for offset in [Vec2::ZERO, half] {
                        painter.rect_filled(egui::Rect::from_min_size(inner.min + offset, half), 0.0, egui::Color32::from_gray(204));
                    }
                }
                painter.rect_filled(inner, 0.0, fill);
            }
            WellValue::None => {
                painter.rect_filled(inner, 0.0, egui::Color32::WHITE);
                painter.line_segment([inner.left_bottom(), inner.right_top()], egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 0, 0)));
            }
            WellValue::Mixed => {
                painter.rect_filled(inner, 0.0, color("panel.bg"));
                painter.text(inner.center(), egui::Align2::CENTER_CENTER, MIXED_TEXT, egui::FontId::proportional(font_size("ui")), color("text"));
            }
        }
        if !state.enabled {
            painter.rect_filled(inner, 0.0, color("panel.bg").gamma_multiply(0.5));
        }
        painter.rect_stroke(rect, corner(radius("sm")), border, egui::StrokeKind::Inside);
    }
    state.decorate(response)
}

pub fn panel_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    if in_studio(ui) { return egui::Frame::new().show(ui, add); }
    egui::Frame::new()
        .fill(surface(ui.ctx(), "panel.bg"))
        .stroke(panel_stroke())
        .corner_radius(egui::CornerRadius::ZERO)
        .inner_margin(space("sm"))
        .show(ui, add)
}

pub fn panel_header(ui: &mut Ui, title: impl Into<String>) -> Response {
    if in_studio(ui) { return ui.allocate_response(Vec2::ZERO, egui::Sense::hover()); }
    let text = egui::RichText::new(title.into()).size(font_size("ui"));
    ui.label(text)
}

pub fn studio_row<R>(ui: &mut Ui, label: &str, controls: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    ui.horizontal(|ui| {
        let width = size("panel.label_width").min(ui.available_width() / 3.0);
        let caption = ui.allocate_ui_with_layout(
            Vec2::new(width, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(width);
                ui.add(egui::Label::new(label).wrap())
            },
        ).inner;
        controls(ui, caption.id)
    }).inner
}

/// Choose the largest group that fits without changing the control hit sizes.
pub fn panel_column_count(ui: &Ui, width: f32, count: usize, minimum: f32) -> usize {
    let gap = ui.spacing().item_spacing.x;
    (((width.max(0.0) + gap) / (minimum + gap)).floor() as usize).clamp(1, count.max(1))
}

/// Places related controls left to right, then wraps them in reading order.
pub fn panel_columns(ui: &mut Ui, count: usize, minimum: f32, mut draw: impl FnMut(&mut Ui, usize)) {
    let columns = panel_column_count(ui, ui.available_width(), count.min(3), minimum);
    if columns == 1 {
        for index in 0..count {
            draw(ui, index);
        }
    } else {
        for start in (0..count).step_by(columns) {
            ui.columns(columns, |cells| {
                for (index, cell) in (start..count).zip(cells) {
                    draw(cell, index);
                }
            });
        }
    }
}

const STUDIO_BODY: &str = "dtool.design.studio_body";

pub fn in_studio(ui: &Ui) -> bool {
    ui.stack().iter().any(|entry| entry.tags().contains(STUDIO_BODY))
}

/// Dock tabs own the title and border. Bodies share one inset without drawing
/// duplicate headings; floating and standalone panels retain their own chrome.
pub fn studio_content<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    studio_content_with(ui, true, add)
}

pub fn studio_content_with<R>(ui: &mut Ui, scroll: bool, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new().fill(surface(ui.ctx(), "panel.bg")).inner_margin(space("sm")).show(ui, |ui| {
        ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag(STUDIO_BODY)), |ui| {
            if scroll {
                egui::ScrollArea::vertical().id_salt("studio.body").auto_shrink([false,false]).show(ui, add).inner
            } else {
                add(ui)
            }
        }).inner
    })
}

pub fn tab(ui: &mut Ui, selected: bool, text: impl Into<WidgetText>) -> Response {
    let fill = if selected {
        surface(ui.ctx(), "tab_row")
    } else {
        color("tab.inactive")
    };
    let button = egui::Button::new(text)
        .fill(fill)
        .corner_radius(corner(radius("sm")))
        .min_size(Vec2::new(0.0, size("bar.tab.height")));
    let response = ui.add(button);
    if selected {
        let rect = response.rect.shrink(1.0);
        ui.painter().hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, ui.visuals().selection.bg_fill));
    }
    response
}

pub fn toolbar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(surface(ui.ctx(), "bars"))
        .inner_margin(space("sm"))
        .show(ui, |ui| {
            ui.set_min_height(size("bar.toolbar.height"));
            ui.horizontal(add).inner
        })
}

pub fn context_bar_section<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(surface(ui.ctx(), "context_bar"))
        .inner_margin(space("sm"))
        .show(ui, |ui| {
            ui.set_min_height(size("bar.context.height"));
            ui.horizontal(add).inner
        })
}

pub fn dialog_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(surface(ui.ctx(), "dialog.bg"))
        .stroke(panel_stroke())
        .corner_radius(corner(radius("lg")))
        .inner_margin(space("lg"))
        .show(ui, add)
}

pub fn dialog_options_scroll<R>(ui: &mut Ui, id: &'static str, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    dialog_options_scroll_with_offset(ui, id, height, None, add)
}

pub fn dialog_options_scroll_with_offset<R>(
    ui: &mut Ui, id: &'static str, height: f32, offset: Option<f32>, add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.scope(|ui| {
        let mut scroll = egui::style::ScrollStyle::solid();
        scroll.bar_width = size("dialog.scroll.width");
        scroll.handle_min_length = size("dialog.scroll.min_handle");
        scroll.bar_outer_margin = space("xs");
        scroll.foreground_color = true;
        ui.spacing_mut().scroll = scroll;
        let mut area = egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
        if let Some(offset) = offset {
            area = area.vertical_scroll_offset(offset);
        }
        area.show(ui, add).inner
    }).inner
}

pub fn dialog_section<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(color("dialog.section"))
        .stroke(egui::Stroke::new(stroke_width("thin"), color("dialog.separator")))
        .corner_radius(corner(radius("sm")))
        .inner_margin(space("sm"))
        .show(ui, add)
}

pub fn menu_row_style(style: &mut egui::Style) {
    style.override_font_id = Some(egui::FontId::proportional(size("menu.font_size")));
    style.wrap_mode = Some(egui::TextWrapMode::Truncate);
    style.spacing.button_padding.x = size("menu.row.padding");
    style.spacing.item_spacing.y = size("menu.row.gap");
    style.visuals.widgets.hovered.weak_bg_fill = color("menu.row_hover");
    style.visuals.widgets.open.weak_bg_fill = color("menu.row_hover");
    style.visuals.widgets.active.weak_bg_fill = color("menu.row_pressed");
}

pub fn menu_item(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    ui.add(
        egui::Button::new(text)
            .frame(false)
            .min_size(Vec2::new(0.0, size("menu.row.height"))),
    )
}

pub fn tooltip(response: Response, text: impl Into<String>) -> Response {
    response.on_hover_text(text.into())
}

pub fn tooltip_at_pointer(response: &Response, text: &str) {
    let tooltip = if response.enabled() {
        egui::Tooltip::for_enabled(response)
    } else {
        egui::Tooltip::for_disabled(response)
    };
    tooltip.at_pointer().show(|ui| ui.label(text));
}

pub fn status_bar_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(surface(ui.ctx(), "bars"))
        .inner_margin(egui::Margin::symmetric(space("xs").round() as i8, 0))
        .show(ui, |ui| ui.horizontal_centered(add).inner)
}

pub fn text_input_multiline(ui: &mut Ui, value: &mut String) -> Response {
    let response = ui.text_edit_multiline(value);
    mark_multiline(ui, &response);
    response
}

pub fn canvas_text_hint(ui:&mut Ui,point:[f32;2],label:&str) {
    ui.painter().text(egui::pos2(point[0]+space("sm"),point[1]-space("sm")),egui::Align2::LEFT_BOTTOM,label,egui::FontId::proportional(font_size("ui")),color("text"));
}
pub fn canvas_line(painter:&egui::Painter,a:egui::Pos2,b:egui::Pos2,colour:egui::Color32) {
    painter.line_segment([a,b],egui::Stroke::new(stroke_width("thin").max(1.0),colour));
}

pub fn canvas_frame_outline(ui:&mut Ui,rect:egui::Rect) {
    ui.painter().rect_stroke(rect,egui::CornerRadius::ZERO,egui::Stroke::new(stroke_width("thin"),color("accent")),egui::StrokeKind::Middle);
}
