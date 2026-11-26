use super::{click_select_command, screen};
use crate::input::primary_button_positions;
use dtool_core::commands::brush::{blend_for_object, linked_blend_path, BlendInfo, BlendLayout, BLEND_SPACING, DEFAULT_BLEND_STEPS, MAX_BLEND_STEPS};
use dtool_core::geometry::box_centre;
use dtool_core::object::ObjectId;
use dtool_core::Session;
use dtool_design::{NumericEvent, NumericSpec, NumericValue};
use dtool_protocol::Command;
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui};
use serde_json::json;

#[derive(Clone, Debug)]
struct SpineDrag {
    blend: u64,
    start: [f32; 2],
    original: [f32; 2],
    offset: [f32; 2],
}

#[derive(Clone, Debug)]
pub(super) struct Blend {
    drag: Option<(ObjectId, [f32; 2], [f32; 2])>,
    spine_drag: Option<SpineDrag>,
    steps: u32,
    endpoint: Option<ObjectId>,
    picking_path: Option<u64>,
}

impl Default for Blend {
    fn default() -> Self {
        Self {
            drag: None,
            spine_drag: None,
            steps: DEFAULT_BLEND_STEPS,
            endpoint: None,
            picking_path: None,
        }
    }
}

impl Blend {
    pub(super) fn active(&self) -> bool {
        self.drag.is_some() || self.spine_drag.is_some()
    }

    pub(super) fn cancel(&mut self) {
        self.drag = None;
        self.spine_drag = None;
        self.picking_path = None;
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    fn current(&self, session: &Session) -> Option<BlendInfo> {
        self.endpoint
            .or_else(|| session.selection.ids.first().copied())
            .and_then(|id| blend_for_object(&session.document, id))
    }

    fn spine(&self, session: &Session) -> Option<(BlendInfo, BlendLayout, [[f32; 2]; 2])> {
        let blend = self.current(session)?;
        let layout = blend.layout?;
        let from = session.document.get_object(blend.from)?;
        let to = session.document.get_object(blend.to)?;
        Some((blend, layout, [box_centre(from), box_centre(to)]))
    }

    pub(super) fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        session: &Session,
        to_doc: impl Fn(Pos2) -> [f32; 2],
        hit: impl Fn([f32; 2]) -> Option<ObjectId>,
    ) -> Option<Command> {
        let (pressed, released) = primary_button_positions(ui);
        let pointer = released.or_else(|| ui.input(|input| input.pointer.latest_pos()));
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if let Some(id) = self.picking_path {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.picking_path = None;
            } else if let Some(blend) = self.current(session).filter(|blend| blend.id == id) {
                if response.clicked()
                    && let Some(path_id) = pointer.map(&to_doc).and_then(&hit)
                    && linked_blend_path(&session.document, blend.from, blend.to, path_id).is_some()
                {
                    self.picking_path = None;
                    return Some(Command::with_params("blend.update", json!({"id": id, "path_id": path_id})));
                }
            } else {
                self.picking_path = None;
            }
            return None;
        }
        if let Some((blend, layout, [from, to])) = self.spine(session)
            && layout.path_id.is_none()
        {
            let middle = midpoint(from, to);
            let handle = std::array::from_fn(|i| middle[i] + layout.spine[i]);
            let handle_screen = screen(response.rect, &session.view, handle);
            let near_handle = |point: Pos2| point.distance(handle_screen) <= 9.0;
            if (response.hovered() && pointer.is_some_and(near_handle)) || self.spine_drag.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if response.contains_pointer()
                && let Some(start) = pressed.filter(|point| near_handle(*point)).map(&to_doc)
            {
                self.spine_drag = Some(SpineDrag {
                    blend: blend.id, start, original: layout.spine, offset: layout.spine,
                });
            }
            if let Some(drag) = &mut self.spine_drag
                && drag.blend == blend.id
            {
                if let Some(point) = pointer.map(&to_doc) {
                    drag.offset = std::array::from_fn(|i| drag.original[i] + point[i] - drag.start[i]);
                }
                if released.is_some() {
                    let offset = drag.offset;
                    self.spine_drag = None;
                    if offset != layout.spine {
                        return Some(Command::with_params("blend.update", json!({"id": blend.id, "spine": offset})));
                    }
                } else if !ui.input(|input| input.pointer.primary_down()) {
                    self.spine_drag = None;
                }
                return None;
            }
        }
        self.spine_drag = None;
        if response.contains_pointer() || response.is_pointer_button_down_on() {
            if let Some(point) = pressed
                .filter(|p| response.interact_rect.contains(*p))
                .map(&to_doc)
            {
                if let Some(id) = hit(point) {
                    self.drag = Some((id, point, point));
                }
            }
        }
        let point = pointer.map(&to_doc);
        if let (Some((_, _, end)), Some(point)) = (&mut self.drag, point) {
            *end = point;
        }
        if released.is_some() {
            if let Some((from, _, end)) = self.drag.take() {
                self.endpoint = Some(from);
                if let Some(to) = hit(end).filter(|id| *id != from) {
                    return Some(Command::with_params(
                        "blend.create",
                        json!({"from":from,"to":to,"steps":self.steps}),
                    ));
                }
            }
            if response.clicked() {
                self.endpoint = point.and_then(hit);
                return click_select_command(self.endpoint, &session.selection.ids, false);
            }
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            self.cancel();
        }
        None
    }

    pub(super) fn paint(&self, ui: &Ui, canvas: Rect, session: &Session, stroke: Stroke) {
        let painter = ui.painter_at(canvas);
        let project = |point| screen(canvas, &session.view, point);
        if let Some((_, start, end)) = self.drag {
            painter.line_segment([project(start), project(end)], stroke);
        } else if let Some((blend, mut layout, [from, to])) = self.spine(session) {
            if let Some(id) = layout.path_id
                && let Some(path) = linked_blend_path(&session.document, blend.from, blend.to, id)
            {
                for (points, _) in dtool_core::outline::flatten_subpaths(&path, 0.1) {
                    painter.add(egui::Shape::line(points.into_iter().map(|p| project(p.map(|v| v as f32))).collect(), stroke));
                }
                return;
            }
            if let Some(drag) = self.spine_drag.as_ref().filter(|drag| drag.blend == blend.id) {
                layout.spine = drag.offset;
            }
            let path = layout.path(from, to);
            let points = [path.p0, path.p1, path.p2].map(|point| project([point.x as f32, point.y as f32]));
            painter.add(egui::epaint::QuadraticBezierShape::from_points_stroke(
                points, false, Color32::TRANSPARENT, stroke,
            ));
            let middle = midpoint(from, to);
            let handle = std::array::from_fn(|i| middle[i] + layout.spine[i]);
            if layout.path_id.is_none() {
                painter.circle_filled(project(handle), 5.0, ui.visuals().panel_fill);
                painter.circle_stroke(project(handle), 5.0, stroke);
            }
        }
    }

    pub(super) fn bar(&mut self, ui: &mut Ui, session: &Session) -> Option<Command> {
        let live = self.current(session);
        let target = (session.document.instance, live.map(|blend| blend.id));
        ui.push_id(target, |ui| ui.horizontal(|ui| {
            let steps = live.map_or(self.steps, |blend| blend.steps);
            let steps_spec = NumericSpec::new(dtool_strings::text("ctx.blend.steps"))
                .range(1.0..=f64::from(MAX_BLEND_STEPS)).decimals(0).width(46.0)
                .reset_to(f64::from(DEFAULT_BLEND_STEPS));
            let steps_edit = blend_number(ui, "steps", steps, &steps_spec);
            let Some(blend) = live else {
                if let NumericEvent::Commit(value) = steps_edit { self.steps = value.round() as u32; }
                return None;
            };
            self.steps = blend.steps;
            let mut changes = json!({"id": blend.id});
            let mut preview = changes.clone();
            let mut scrubbing = matches!(steps_edit, NumericEvent::Preview(_) | NumericEvent::Cancel);
            edit_blend_number(&mut changes, &mut preview, "steps", blend.steps, steps_edit);
            if let Some(layout) = blend.layout {
                ui.separator();
                let spacing_spec = NumericSpec::new(dtool_strings::text("ctx.blend.spacing"))
                    .range(f64::from(*BLEND_SPACING.start())..=f64::from(*BLEND_SPACING.end()))
                    .suffix("%").decimals(0).width(54.0)
                    .reset_to(f64::from(BlendLayout::default().spacing));
                let spacing_edit = blend_number(ui, "spacing", layout.spacing, &spacing_spec);
                scrubbing |= matches!(spacing_edit, NumericEvent::Preview(_) | NumericEvent::Cancel);
                edit_blend_number(&mut changes, &mut preview, "spacing", layout.spacing, spacing_edit);
                if layout.path_id.is_none()
                    && dtool_design::button(ui, dtool_strings::text("ctx.blend.straighten")).clicked()
                    && layout.spine != [0.0; 2]
                {
                    changes["spine"] = json!([0, 0]);
                }
                let picking = self.picking_path == Some(blend.id);
                let label = if picking { "ctx.blend.cancel_pick" } else { "ctx.blend.pick_path" };
                if dtool_design::button(ui, dtool_strings::text(label))
                    .on_hover_text(dtool_strings::text("ctx.blend.pick_path_help")).clicked()
                {
                    self.picking_path = (!picking).then_some(blend.id);
                }
                if layout.path_id.is_some() && dtool_design::button(ui, dtool_strings::text("ctx.blend.unlink_path")).clicked() {
                    self.picking_path = None;
                    changes["path_id"] = serde_json::Value::Null;
                }
            }
            ui.separator();
            if dtool_design::button(ui, dtool_strings::text("ctx.blend.expand")).clicked() {
                Some(Command::with_params("blend.expand", json!({"id":blend.id})))
            } else if changes.as_object().is_some_and(|fields| fields.len() > 1) {
                Some(Command::with_params("blend.update", changes))
            } else {
                if preview.as_object().is_some_and(|fields| fields.len() > 1) {
                    crate::app_canvas::stage_geometry_preview(ui, session, Command::with_params("blend.update", preview));
                } else if scrubbing {
                    crate::app_canvas::clear_geometry_preview(ui);
                }
                None
            }
        }).inner).inner
    }
}

fn blend_number(ui: &mut Ui, key: &str, current: u32, spec: &NumericSpec<'_>) -> NumericEvent {
    let id = ui.make_persistent_id(key);
    let (field, event) = dtool_design::numeric_field(ui, id, NumericValue::Value(f64::from(current)), spec);
    if key == "spacing" { field.on_hover_text(dtool_strings::text("ctx.blend.spacing.help")); }
    if event != NumericEvent::None { return event; }
    let state = dtool_design::numeric_state(ui.ctx(), id);
    if state.scrubbing && state.previewed { NumericEvent::Preview(state.scrub) } else { NumericEvent::None }
}

fn edit_blend_number(changes: &mut serde_json::Value, preview: &mut serde_json::Value, key: &str, current: u32, event: NumericEvent) {
    let (target, value) = match event {
        NumericEvent::Commit(value) => (changes, value),
        NumericEvent::Preview(value) => (preview, value),
        _ => return,
    };
    let value = value.round() as u32;
    if value != current { target[key] = json!(value); }
}

fn midpoint(from: [f32; 2], to: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|i| from[i] + (to[i] - from[i]) * 0.5)
}
