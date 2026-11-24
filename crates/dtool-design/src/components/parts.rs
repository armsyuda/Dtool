use egui::{Response, Sense, TextStyle, TextWrapMode, Ui, Vec2, WidgetText};

use super::{corner, ControlState};
use crate::tokens::{color, radius, size, space};

pub fn list_row(ui: &mut Ui, selected: bool, text: impl Into<WidgetText>) -> Response {
    list_row_with(ui, selected, text, &ControlState::default())
}

pub fn list_row_with(ui: &mut Ui, selected: bool, text: impl Into<WidgetText>, state: &ControlState) -> Response {
    let response = ui
        .scope(|ui| {
            let visuals = ui.visuals_mut();
            for widget in [&mut visuals.widgets.hovered, &mut visuals.widgets.active] {
                widget.weak_bg_fill = color("button.bg_hover");
                widget.bg_fill = color("button.bg_hover");
            }
            let mut text = text.into();
            if selected {
                text = text.color(visuals.selection.stroke.color);
            }
            let row = egui::Button::selectable(selected, text)
                .corner_radius(corner(radius("sm")))
                .min_size(Vec2::new(0.0, size("size.list_row")));
            ui.add_enabled(state.enabled, row)
        })
        .inner;
    state.decorate(response)
}

pub fn small_icon_button_side() -> f32 {
    size("icon.size.sm") + space("xs") * 2.0
}

pub fn small_icon_button(ui: &mut Ui, icon: impl Into<WidgetText>) -> Response {
    small_icon_button_with(ui, icon, &ControlState::default())
}

pub fn small_icon_button_with(ui: &mut Ui, icon: impl Into<WidgetText>, state: &ControlState) -> Response {
    let side = small_icon_button_side();
    let icon = icon.into();
    let response = ui
        .add_enabled_ui(state.enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
            if ui.is_rect_visible(rect) {
                let visuals = ui.style().interact(&response);
                if response.hovered() || response.is_pointer_button_down_on() {
                    ui.painter().rect_filled(rect, corner(radius("sm")), color("button.bg_hover"));
                }
                let galley = icon.into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, TextStyle::Button);
                let at = rect.center() - galley.size() * 0.5;
                ui.painter().galley(at, galley, visuals.text_color());
            }
            response
        })
        .inner;
    state.decorate(response)
}

pub fn frameless_button(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    frameless_button_with(ui, text, &ControlState::default())
}

pub fn frameless_button_with(ui: &mut Ui, text: impl Into<WidgetText>, state: &ControlState) -> Response {
    let response = ui
        .scope(|ui| {
            let widgets = &mut ui.visuals_mut().widgets;
            for widget in [&mut widgets.hovered, &mut widgets.active] {
                widget.weak_bg_fill = egui::Color32::TRANSPARENT;
                widget.bg_fill = egui::Color32::TRANSPARENT;
                widget.bg_stroke = egui::Stroke::NONE;
                widget.expansion = 0.0;
            }
            ui.add_enabled(state.enabled, egui::Button::new(text).frame(false))
        })
        .inner;
    state.decorate(response)
}
