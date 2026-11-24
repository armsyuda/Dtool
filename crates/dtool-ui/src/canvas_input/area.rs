use super::{click_select_command, screen, Mods};
use crate::input::primary_button_positions;
use dtool_core::commands::measure::area::{totals, units, Geometry, Metrics};
use dtool_core::commands::measure::readout_decimals;
use dtool_core::object::ObjectId;
use dtool_core::scene::Scene;
use dtool_core::selection::{selection_marquee_with_flags, MarqueeMode, SelectOp};
use dtool_core::Session;
use dtool_protocol::Command;
use dtool_strings::text;
use egui::{Pos2, Rect, Response, Stroke, Ui};
use serde_json::json;

type Hover = (ObjectId, u64, Option<Geometry>);
type SelectionReading = (u64, Vec<ObjectId>, usize, Metrics);

#[derive(Clone, Debug, Default)]
pub(super) struct Area {
    start: Option<[f32; 2]>,
    hover: Option<Hover>,
    selection: Option<SelectionReading>,
    segment: Option<(f64, [[f32; 2]; 2])>,
    position: Option<[f32; 2]>,
    segments_mode: bool,
    scale: Option<([f32; 2], f64)>,
}

impl Area {
    pub(super) fn clear(&mut self) {
        let mode = self.segments_mode;
        *self = Self {
            segments_mode: mode,
            ..Self::default()
        };
    }

    pub(super) fn active(&self) -> bool {
        self.start.is_some()
    }

    pub(super) fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        session: &Session,
        scene: &Scene,
        mods: Mods,
        to_doc: impl Fn(Pos2) -> [f32; 2],
    ) -> (Option<[f32; 4]>, Option<Command>) {
        let (pressed, released) = primary_button_positions(ui);
        if response.contains_pointer() || response.is_pointer_button_down_on() {
            if let Some(point) = pressed.filter(|point| response.interact_rect.contains(*point)) {
                self.start = Some(to_doc(point));
            }
        }
        let pointer = ui.input(|input| input.pointer.latest_pos());
        let position = pointer.filter(|_| response.hovered()).map(&to_doc);
        let doc = &session.document;
        let tolerance = super::HIT_TOLERANCE_PX / session.view.zoom.max(1e-4);
        let hit = |p| dtool_core::scene_hit::hit_test_in_view(scene, session, p, tolerance, None);
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let hover_id = position.and_then(hit);
        let hover_key = self.hover.as_ref().map(|(id, version, _)| (*id, *version));
        let hover_changed = hover_key != hover_id.map(|id| (id, doc.version));
        if hover_changed {
            self.hover = hover_id.map(|id| {
                (
                    id,
                    doc.version,
                    doc.get_object(id).and_then(Geometry::for_object),
                )
            });
        }
        self.position = position;
        self.segment = if self.segments_mode {
            self.hover
                .as_ref()
                .and_then(|(_, _, geometry)| geometry.as_ref())
                .zip(position)
                .and_then(|(geometry, point)| geometry.segment(point, tolerance))
        } else {
            None
        };
        let end = released.or(pointer).map(&to_doc);
        let marquee = self.start.zip(end).and_then(|([x, y], [ex, ey])| {
            let (w, h) = ((ex - x).abs(), (ey - y).abs());
            (w.hypot(h) > 3.0 / session.view.zoom.max(1e-4)).then_some([x.min(ex), y.min(ey), w, h])
        });
        if let (Some(_), Some(end), Some(_)) =
            (self.start.take_if(|_| released.is_some()), end, released)
        {
            let command = if let Some(rect) = marquee {
                let mode = if mods.ctrl {
                    MarqueeMode::Enclosed
                } else {
                    MarqueeMode::Touch
                };
                let op = if mods.shift {
                    SelectOp::Add
                } else {
                    SelectOp::Replace
                };
                let selection = selection_marquee_with_flags(doc, rect, mode, op, &session.selection, &session.view.canvas_flags);
                let ids: Vec<_> = selection
                    .ids
                    .into_iter()
                    .filter(|id| scene.quad(*id).is_some())
                    .collect();
                (ids != session.selection.ids)
                    .then(|| Command::with_params("selection.set", json!({"ids":ids})))
            } else {
                click_select_command(hit(end), &session.selection.ids, mods.shift)
            };
            ui.ctx().request_repaint();
            return (None, command);
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            self.start = None;
        }
        if hover_changed {
            ui.ctx().request_repaint();
        }
        (marquee.filter(|_| self.start.is_some()), None)
    }

    pub(super) fn paint(&self, ui: &Ui, rect: Rect, session: &Session, stroke: Stroke) {
        let Some((geometry, position)) = self
            .hover
            .as_ref()
            .and_then(|(_, _, geometry)| geometry.as_ref())
            .zip(self.position)
        else {
            return;
        };
        let painter = ui.painter_at(rect);
        let to_screen = |p| screen(rect, &session.view, p);
        if let Some((_, points)) = &self.segment {
            painter.line_segment([to_screen(points[0]), to_screen(points[1])], stroke);
        } else {
            for (points, closed) in &geometry.outlines {
                let points = points.iter().copied().map(to_screen).collect();
                painter.add(if *closed {
                    egui::Shape::closed_line(points, stroke)
                } else {
                    egui::Shape::line(points, stroke)
                });
            }
        }
        let (unit, factor) = units(&session.document);
        let decimals = readout_decimals(unit, false);
        let area_decimals = readout_decimals(unit, true);
        let metrics = geometry.metrics;
        let label = if let Some((length, _)) = &self.segment {
            format!("{:.decimals$} {unit}", length * factor)
        } else if self.segments_mode {
            return;
        } else {
            format!(
                "{:.area_decimals$} {unit}² · {:.decimals$} {unit}",
                metrics.area * factor.powi(2),
                (metrics.perimeter + metrics.hole_perimeter) * factor
            )
        };
        let origin = to_screen(position) + egui::vec2(10.0, 14.0);
        let galley = painter.layout_no_wrap(
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        let background = Rect::from_min_size(origin, galley.size() + egui::vec2(12.0, 6.0));
        painter.rect_filled(background, 3.0, ui.visuals().panel_fill);
        painter.galley(
            origin + egui::vec2(6.0, 3.0),
            galley,
            ui.visuals().text_color(),
        );
    }

    pub(super) fn bar(&mut self, ui: &mut Ui, session: &Session) -> Option<Command> {
        let doc = &session.document;
        let ids = &session.selection.ids;
        if self
            .selection
            .as_ref()
            .is_none_or(|(version, saved, _, _)| *version != doc.version || saved != ids)
        {
            let (count, metrics) = totals(doc, ids);
            self.selection = Some((doc.version, ids.clone(), count, metrics));
        }
        let (_, _, count, metrics) = self.selection.as_ref()?;
        let hover = self
            .hover
            .as_ref()
            .and_then(|(_, _, geometry)| geometry.as_ref());
        let shown = if *count > 0 {
            *metrics
        } else {
            hover.map(|geometry| geometry.metrics).unwrap_or_default()
        };
        let (unit, factor) = units(doc);
        let decimals = readout_decimals(unit, false);
        let area_decimals = readout_decimals(unit, true);
        let [drawing, real] = doc.drawing_scale.map(f64::from);
        let mut value = self
            .scale
            .filter(|(base, _)| *base == doc.drawing_scale)
            .map_or(real / drawing, |(_, value)| value);
        let mut action = None;
        ui.horizontal(|ui| {
            ui.label(text("ctx.measure.drawing_scale"));
            ui.add(
                egui::DragValue::new(&mut value)
                    .range(0.000001..=1e9)
                    .max_decimals(6)
                    .prefix("1:"),
            );
            if dtool_design::button_with(
                ui,
                text("button.apply"),
                &dtool_design::ControlState::enabled((value - real / drawing).abs() > 1e-7),
            )
            .clicked()
            {
                action = Some(Command::with_params(
                    "doc.scale.set",
                    json!({"drawing_scale":[1.0,value]}),
                ));
            }
            ui.separator();
            ui.label(text("ctx.area.perimeter"));
            if ui
                .selectable_label(!self.segments_mode, text("ctx.area.full_edges"))
                .clicked()
            {
                self.segments_mode = false;
            }
            if ui
                .selectable_label(self.segments_mode, text("ctx.area.segments"))
                .clicked()
            {
                self.segments_mode = true;
            }
            ui.separator();
            ui.label(format!(
                "{} {:.area_decimals$} {unit}²",
                text("ctx.area.value"),
                shown.area * factor.powi(2)
            ));
            ui.label(format!(
                "{} {:.decimals$} {unit}",
                text("ctx.area.outside"),
                shown.perimeter * factor
            ));
            if shown.hole_perimeter > 0.0 {
                ui.label(format!(
                    "{} {:.decimals$} {unit}",
                    text("ctx.area.with_holes"),
                    (shown.perimeter + shown.hole_perimeter) * factor
                ));
            }
        });
        self.scale = Some((doc.drawing_scale, value));
        action
    }
}
