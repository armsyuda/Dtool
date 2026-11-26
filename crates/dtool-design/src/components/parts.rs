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

/// Draws a compact preview above a single-line caption in a selectable panel list.
pub fn preview_list_row(
    ui: &mut Ui,
    size: Vec2,
    selected: bool,
    label: &str,
    caption: String,
    preview: impl FnOnce(&Ui, egui::Rect),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    let visuals = ui.visuals();
    let paint = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let fill = if selected { crate::theme::surface(ui.ctx(), "widget.bg_active") }
        else if response.hovered() { visuals.widgets.hovered.weak_bg_fill }
        else { visuals.widgets.inactive.weak_bg_fill };
    let rounding = corner(radius("sm"));
    paint.rect_filled(rect, rounding, fill);
    if selected || response.has_focus() {
        paint.rect_stroke(rect, rounding, egui::Stroke::new(crate::stroke_width("thin"), visuals.selection.bg_fill),
            egui::StrokeKind::Inside);
    }
    let font = TextStyle::Small.resolve(ui.style());
    let ink = visuals.text_color();
    let mut job = egui::text::LayoutJob::simple_singleline(caption.clone(), font, ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 10.0).max(1.0));
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let text_top = rect.bottom() - galley.size().y - 3.0;
    let preview_rect = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(4.0, 2.0),
        egui::pos2(rect.right() - 4.0, text_top - 2.0),
    );
    if preview_rect.is_positive() {
        preview(ui, preview_rect);
    }
    paint.galley(egui::pos2(rect.left() + 5.0, text_top), galley, ink);
    super::tooltip(response, caption)
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
