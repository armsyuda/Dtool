use dtool_core::commands::measure::{reading, readout_decimals, ADD};
use dtool_core::Session;
use dtool_protocol::Command;
use egui::{pos2, vec2, Pos2, Rect, Response, Stroke, Ui};
use serde_json::json;

use super::{primary_button_positions, screen};

type Line = ([f32; 2], [f32; 2]);

#[derive(Debug, Clone, Default)]
pub(super) struct Measure {
    press: Option<[f32; 2]>,
    first: Option<[f32; 2]>,
    line: Option<Line>,
    complete: bool,
    known_length: Option<(&'static str, f64)>,
}

impl Measure {
    pub(super) fn active(&self) -> bool {
        self.press.is_some()
    }

    pub(super) fn has_state(&self) -> bool {
        self.active() || self.first.is_some() || self.line.is_some()
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        session: &Session,
        to_doc: impl Fn(Pos2) -> [f32; 2],
    ) {
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let (pressed, released) = primary_button_positions(ui);
        let snap = |p| crate::canvas_tools::snap_point(session, p, &[]).0;
        if response.contains_pointer() || response.is_pointer_button_down_on() {
            if let Some(p) = pressed.filter(|p| response.interact_rect.contains(*p)) {
                self.press = Some(snap(to_doc(p)));
            }
        }
        let Some(start) = self.first.or(self.press) else {
            return;
        };
        if !self.active() && !response.hovered() {
            return;
        }
        let pointer = released
            .or_else(|| response.interact_pointer_pos())
            .or_else(|| {
                ui.input(|i| i.pointer.latest_pos())
                    .filter(|_| response.hovered() || self.active())
            });
        let Some(pointer) = pointer else { return };
        let shift = ui.input(|i| i.modifiers.shift);
        let end = if shift {
            crate::canvas_curves::constrain_angle(start, to_doc(pointer))
        } else {
            snap(to_doc(pointer))
        };
        let [dx, dy] = [end[0] - start[0], end[1] - start[1]];
        let threshold = 2.0 / session.view.zoom.max(1e-4);
        self.complete = false;
        self.line = (dx.hypot(dy) > threshold).then_some((start, end));
        if released.is_some() && self.press.take().is_some() {
            self.known_length = None;
            if self.line.is_some() {
                self.complete = true;
                self.first = None;
            } else {
                self.first = Some(start);
            }
        } else if !ui.input(|i| i.pointer.primary_down()) {
            self.press = None;
        }
        ui.ctx().request_repaint();
    }

    pub(super) fn paint(&self, ui: &Ui, rect: Rect, session: &Session, stroke: Stroke) {
        let painter = ui.painter_at(rect);
        let screen_point = |p| screen(rect, &session.view, p);
        let Some((start, end)) = self.line else {
            if let Some(first) = self.first {
                painter.circle_stroke(screen_point(first), 4.0, stroke);
            }
            return;
        };
        let a = screen_point(start);
        let b = screen_point(end);
        painter.line_segment([a, b], stroke);
        painter.circle_stroke(a, 4.0, stroke);
        painter.circle_stroke(b, 4.0, stroke);
        let Some(reading) = reading(&session.document, start.map(f64::from), end.map(f64::from))
        else {
            return;
        };
        let decimals = readout_decimals(reading.units, false);
        let label = format!("{:.decimals$} {} · {:.1}°", reading.length, reading.units, reading.angle_deg);
        let text = painter.layout_no_wrap(
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        let middle = a.lerp(b, 0.5) - vec2(0.0, 15.0);
        let box_rect = Rect::from_center_size(middle, text.size() + vec2(12.0, 6.0));
        painter.rect_filled(box_rect, 3.0, ui.visuals().panel_fill);
        painter.galley(
            pos2(box_rect.min.x + 6.0, box_rect.min.y + 3.0),
            text,
            ui.visuals().text_color(),
        );
    }

    pub(super) fn bar(&mut self, ui: &mut Ui, session: &Session) -> Option<Command> {
        let [drawing, real] = session.document.drawing_scale;
        let mut action = None;
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} {drawing:.3}:{real:.3}",
                dtool_strings::text("ctx.measure.drawing_scale")
            ));
            let Some((start, end)) = self.line.filter(|_| self.complete) else {
                return;
            };
            let Some(value) = reading(&session.document, start.map(f64::from), end.map(f64::from))
            else {
                return;
            };
            let decimals = readout_decimals(value.units, false);
            let mut known = self.known_length.filter(|(units, _)| *units == value.units)
                .map_or(value.length, |(_, length)| length);
            ui.add(
                egui::DragValue::new(&mut known)
                    .range(0.000001..=1e9)
                    .max_decimals(decimals + 1)
                    .suffix(format!(" {}", value.units)),
            );
            self.known_length = Some((value.units, known));
            if dtool_design::button(ui, dtool_strings::text("ctx.measure.set_distance")).clicked() {
                action = Some(Command::with_params(
                    "doc.scale.set",
                    json!({ "drawing_scale": [1.0, known / value.unscaled_length] }),
                ));
            }
            if dtool_design::button(ui, dtool_strings::text("ctx.measure.copy")).clicked() {
                ui.ctx()
                    .copy_text(format!("{:.decimals$} {}", value.length, value.units));
            }
            if dtool_design::button(ui, dtool_strings::text("ctx.measure.persistent")).clicked() {
                action = Some(Command::with_params(
                    ADD,
                    json!({ "start": start, "end": end }),
                ));
                self.clear();
            }
        });
        action
    }
}
