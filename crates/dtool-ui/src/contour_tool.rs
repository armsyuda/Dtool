use dtool_core::commands::contour::{self, ContourCap, ContourJoin, ContourSpec};
use dtool_core::{Command, Object, ObjectId, Session};
use dtool_design::{ControlState, NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;
use egui::{Pos2, Rect, Response, Stroke, Ui, Vec2};
use serde_json::json;

const JOIN: [(ContourJoin, &str); 3] = [
    (ContourJoin::Round, "contour.round"),
    (ContourJoin::Miter, "contour.miter"),
    (ContourJoin::Bevel, "contour.bevel"),
];
const CAP: [(ContourCap, &str); 3] = [
    (ContourCap::None, "contour.cap.none"),
    (ContourCap::Round, "contour.round"),
    (ContourCap::Square, "contour.cap.square"),
];

pub(crate) fn toolbar(ui: &mut Ui, session: &Session) -> Option<Command> {
    let Some(object) = contour::selected(session) else {
        ui.label(text("contour.select"));
        return None;
    };
    let writable = !session.document.is_object_locked(object);
    let original = object.contour.unwrap_or_default();
    let mut next = original;
    let mut preview = None;
    let mut bake = false;
    ui.push_id(("contour", object.id), |ui| {
        ui.add_enabled_ui(writable, |ui| {
            let radius_spec = NumericSpec::new(text("contour.radius"))
                .range(-contour::MAX_RADIUS..=contour::MAX_RADIUS).step(0.25).width(72.0)
                .suffix(dtool_core::units::suffix(session.document.units)).reset_to(0.0);
            let radius_id = ui.make_persistent_id("radius");
            match dtool_design::numeric_field(ui, radius_id, NumericValue::Value(original.radius), &radius_spec).1 {
                NumericEvent::Preview(value) => preview = Some(ContourSpec { radius: value, ..original }),
                NumericEvent::Commit(value) => next.radius = value,
                NumericEvent::Cancel => crate::app_canvas::clear_geometry_preview(ui),
                NumericEvent::None => {}
            }
            let compact = ui.available_width() < 410.0;
            if compact {
                ui.menu_button(text("contour.options"), |ui| {
                    for (value, label) in JOIN {
                        if dtool_design::choice_value(ui, &mut next.join, value, text(label)).changed() { ui.close(); }
                    }
                    ui.separator();
                    for (value, label) in CAP {
                        if dtool_design::choice_value(ui, &mut next.cap, value, text(label)).changed() { ui.close(); }
                    }
                    miter_input(ui, &mut next, original, &mut preview);
                });
            } else {
                ui.label(text("contour.join"));
                for (value, label) in JOIN {
                    dtool_design::choice_value(ui, &mut next.join, value, text(label));
                }
                if next.join == ContourJoin::Miter {
                    miter_input(ui, &mut next, original, &mut preview);
                }
                ui.label(text("contour.cap"));
                for (value, label) in CAP {
                    dtool_design::choice_value(ui, &mut next.cap, value, text(label));
                }
            }
            bake = dtool_design::button_with(ui, text("contour.bake"), &ControlState::enabled(object.has_live_contour())).clicked();
        });
    });
    if let Some(spec) = preview {
        crate::app_canvas::stage_geometry_preview(ui, session, contour::set_command(object.id, spec));
    }
    if bake {
        Some(Command::with_params(contour::BAKE, json!({"id":object.id})))
    } else {
        (next != original).then(|| contour::set_command(object.id, next))
    }
}

fn miter_input(ui: &mut Ui, next: &mut ContourSpec, original: ContourSpec, preview: &mut Option<ContourSpec>) {
    let spec = NumericSpec::new(text("contour.miter.limit")).range(1.0..=100.0).step(0.1)
        .width(42.0).reset_to(4.0).enabled(next.join == ContourJoin::Miter);
    let id = ui.make_persistent_id("miter");
    match dtool_design::numeric_field(ui, id, NumericValue::Value(original.miter), &spec).1 {
        NumericEvent::Preview(value) => *preview = Some(ContourSpec { miter: value, ..original }),
        NumericEvent::Commit(value) => next.miter = value,
        NumericEvent::Cancel => crate::app_canvas::clear_geometry_preview(ui),
        NumericEvent::None => {}
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ContourTool {
    drag: Option<Gesture>,
}

#[derive(Debug, Clone)]
struct Gesture {
    object: ObjectId,
    generation: u64,
    start: Pos2,
    direction: Vec2,
    scale: f32,
    original: ContourSpec,
}

fn handle(session: &Session, object: &Object, rect: Rect) -> Option<(Pos2, Vec2)> {
    let path = dtool_core::outline::base_object_outline(object, dtool_core::boolean::DEFAULT_TOLERANCE)?;
    let [_, top, right, bottom] = dtool_core::geometry::bez_path_bounds(&path);
    let transform = dtool_core::geometry::object_matrix(object);
    let screen = |x| {
        let p = dtool_core::geometry::apply_matrix(&transform, [object.pos[0] + x, object.pos[1] + (top + bottom) * 0.5]);
        let p = dtool_core::doc_to_view(&session.view, p);
        rect.min + egui::vec2(p[0], p[1])
    };
    let origin = screen(right);
    let unit = screen(right + 1.0) - origin;
    let point = origin + unit * object.contour.unwrap_or_default().radius as f32;
    point.is_finite().then_some((point, unit))
}

impl ContourTool {
    pub(crate) fn active(&self) -> bool { self.drag.is_some() }

    pub(crate) fn cancel(&mut self) { self.drag = None; }

    pub(crate) fn input(&mut self, ui: &Ui, response: &Response, session: &Session) -> (bool, Option<Command>) {
        let Some(object) = contour::selected(session).filter(|object| !session.document.is_object_locked(object)) else {
            self.cancel();
            return (false, None);
        };
        let Some((point, unit)) = handle(session, object, response.rect) else {
            self.cancel();
            return (false, None);
        };
        if self.drag.as_ref().is_some_and(|drag| drag.object != object.id || drag.generation != session.generation)
            || ui.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.cancel();
            crate::app_canvas::clear_geometry_preview(ui);
            return (true, None);
        }
        let hover = response.hover_pos().is_some_and(|pointer| point.distance(pointer) <= 10.0);
        if hover { ui.ctx().set_cursor_icon(egui::CursorIcon::Grab); }
        let (pressed, released) = crate::input::primary_button_positions(ui);
        if let Some(start) = pressed.filter(|start| response.contains_pointer() && point.distance(*start) <= 10.0) {
            let scale = unit.length();
            if scale > 0.0001 {
                self.drag = Some(Gesture { object: object.id, generation: session.generation, start,
                    direction: unit / scale, scale, original: object.contour.unwrap_or_default() });
            }
        }
        let Some(drag) = &self.drag else { return (hover, None); };
        let (down, latest) = ui.input(|input| (input.pointer.primary_down(), input.pointer.latest_pos()));
        let Some(pointer) = released.or(latest).filter(|_| down || released.is_some()) else {
            self.cancel();
            crate::app_canvas::clear_geometry_preview(ui);
            return (true, None);
        };
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let delta = (pointer - drag.start).dot(drag.direction) / drag.scale;
        let radius = (drag.original.radius + f64::from(delta)).clamp(-contour::MAX_RADIUS, contour::MAX_RADIUS);
        let moved = pointer.distance(drag.start) >= 2.0;
        let command = contour::set_command(drag.object, ContourSpec { radius, ..drag.original });
        if released.is_some() {
            self.cancel();
            crate::app_canvas::clear_geometry_preview(ui);
            return (true, moved.then_some(command));
        }
        if moved {
            crate::app_canvas::stage_geometry_preview(ui, session, command);
        }
        (true, None)
    }

    pub(crate) fn paint(&self, ui: &Ui, rect: Rect, session: &Session, stroke: Stroke) {
        let Some(object) = contour::selected(session).filter(|object| !session.document.is_object_locked(object)) else { return; };
        let Some((point, _)) = handle(session, object, rect) else { return; };
        if rect.contains(point) {
            let painter = ui.painter_at(rect);
            painter.circle_filled(point, 5.0, ui.visuals().extreme_bg_color);
            painter.circle_stroke(point, 5.0, stroke);
        }
    }
}

