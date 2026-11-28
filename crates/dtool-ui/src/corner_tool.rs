use dtool_core::commands::{contour, curve_nodes::corner::{self, Anchor}};
use dtool_core::curve::{CornerKind, Curve, corner_axis, effective_radius, max_corner_radius};
use dtool_core::{Command, Object, ObjectId, Session};
use dtool_design::{ControlState, NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;
use egui::{Pos2, Rect, Response, Stroke, Ui, Vec2};
use serde_json::json;

const KINDS: [(CornerKind, &str); 5] = [
    (CornerKind::None, "corner.style.none"),
    (CornerKind::Rounded, "corner.style.rounded"),
    (CornerKind::Straight, "corner.style.straight"),
    (CornerKind::Concave, "corner.style.concave"),
    (CornerKind::Cutout, "corner.style.cutout"),
];

#[derive(Debug, Clone, Default)]
pub(crate) struct CornerTool {
    owner: Option<(dtool_core::document::DocInstance, ObjectId)>,
    selected: Vec<Anchor>,
    gesture: Option<Gesture>,
    relative: bool,
}

#[derive(Debug, Clone)]
enum Gesture {
    Radius { start: Pos2, generation: u64, anchor: Anchor, original: f64, axis: Vec2 },
    Marquee { start: Pos2, current: Pos2, generation: u64, extend: bool },
}

struct Marker {
    key: Anchor,
    anchor: Pos2,
    handle: Pos2,
    axis: Vec2,
    radius: f64,
}

fn markers(session: &Session, object: &Object, curve: &Curve, rect: Rect) -> Vec<Marker> {
    let matrix = dtool_core::geometry::object_matrix(object);
    let screen = |p: [f64; 2]| {
        let p = dtool_core::geometry::apply_matrix(&matrix, std::array::from_fn(|axis| object.pos[axis] + p[axis] as f32));
        let p = dtool_core::doc_to_view(&session.view, p);
        rect.min + egui::vec2(p[0], p[1])
    };
    curve.contours().enumerate().flat_map(|(contour, path)| {
        path.points.iter().enumerate().filter_map(move |(index, point)| {
            let direction = corner_axis(path, index)?;
            let anchor = screen(point.pos);
            let axis = screen(std::array::from_fn(|axis| point.pos[axis] + direction[axis])) - anchor;
            if !anchor.is_finite() || !axis.is_finite() { return None; }
            let radius = effective_radius(path, index);
            Some(Marker { key: Anchor { contour, id: point.id }, anchor, axis, radius,
                handle: anchor + axis * radius as f32 })
        })
    }).collect()
}

fn marker_at(markers: &[Marker], point: Pos2) -> Option<&Marker> {
    let distance = |marker: &Marker| marker.anchor.distance_sq(point).min(marker.handle.distance_sq(point));
    markers.iter().filter(|marker| distance(marker) <= 81.0)
        .min_by(|left, right| distance(left).total_cmp(&distance(right)))
}

fn target_value(radius: f64, maximum: f64, relative: bool) -> f64 {
    if relative && maximum > 0.0 { (radius * 100.0 / maximum).clamp(0.0, 100.0) } else { radius }
}

fn limits(curve: &Curve, points: &[Anchor]) -> Vec<(f64, f64, CornerKind)> {
    let contours: Vec<_> = curve.contours().collect();
    points.iter().filter_map(|target| {
        let path = contours.get(target.contour)?;
        let index = path.points.iter().position(|point| point.id == target.id)?;
        let siblings: Vec<_> = points.iter().filter(|point| point.contour == target.contour).map(|point| point.id).collect();
        let spec = path.points[index].corner;
        Some((effective_radius(path, index), max_corner_radius(path, index, &siblings),
            spec.map_or(CornerKind::None, |spec| spec.kind)))
    }).collect()
}

impl CornerTool {
    pub(crate) fn active(&self) -> bool { self.gesture.is_some() }

    pub(crate) fn cancel(&mut self) { self.gesture = None; }

    fn sync(&mut self, session: &Session, object: &Object, markers: &[Marker]) {
        let owner = Some((session.document.instance, object.id));
        if self.owner != owner {
            self.owner = owner;
            self.selected.clear();
            self.cancel();
        }
        self.selected.retain(|key| markers.iter().any(|marker| marker.key == *key));
    }

    fn effective_anchors(&self, markers: &[Marker]) -> Vec<Anchor> {
        if self.selected.is_empty() { markers.iter().map(|marker| marker.key).collect() } else { self.selected.clone() }
    }

    pub(crate) fn toolbar(&mut self, ui: &mut Ui, session: &Session) -> Option<Command> {
        let Some(object) = corner::selected(session) else {
            ui.label(text("corner.select_curve"));
            return None;
        };
        let Some(curve) = corner::source(object) else {
            ui.label(text("corner.select_curve"));
            return None;
        };
        let markers = markers(session, object, &curve, ui.max_rect());
        self.sync(session, object, &markers);
        let points = self.effective_anchors(&markers);
        let limits = limits(&curve, &points);
        let editable = corner::editable(session, object);
        let mut output = None;
        ui.push_id(("corner_tool", object.id), |ui| {
            ui.add_enabled_ui(editable && !limits.is_empty(), |ui| {
                let mut kind = limits.first().map(|(_, _, kind)| *kind)
                    .filter(|first| limits.iter().all(|(_, _, kind)| kind == first));
                let old_kind = kind;
                let max_radius = limits.iter().map(|&(_, maximum, _)| maximum).fold(0.0_f64, f64::max);
                let value = limits.first().map(|&(radius, maximum, _)| target_value(radius, maximum, self.relative));
                let mixed = value.is_some_and(|value| limits.iter().any(|&(radius, maximum, _)|
                    (value - target_value(radius, maximum, self.relative)).abs() > 0.005));
                let compact = ui.available_width() < 420.0;
                let mut kinds = |ui: &mut Ui| {
                    for (choice, label) in KINDS {
                        if dtool_design::choice_value(ui, &mut kind, Some(choice), text(label)).changed() && compact { ui.close(); }
                    }
                };
                if compact {
                    ui.menu_button(text("corner.tool_kind"), &mut kinds);
                } else {
                    ui.label(text("corner.tool_kind"));
                    kinds(ui);
                }
                for (relative, label) in [(false, "corner.tool_absolute"), (true, "corner.tool_relative")] {
                    dtool_design::choice_value(ui, &mut self.relative, relative, text(label));
                }
                let value = limits.first().map_or(0.0, |&(radius, maximum, _)| target_value(radius, maximum, self.relative));
                let maximum = if self.relative { 100.0 } else { max_radius };
                let shown = if mixed { NumericValue::Mixed } else { NumericValue::Value(value) };
                let spec = NumericSpec::new(text("corner.tool_radius")).width(70.0).reset_to(0.0)
                    .range(0.0..=maximum).step(0.25).mixed_origin(value)
                    .suffix(if self.relative { "%" } else { dtool_core::units::suffix(session.document.units) });
                let id = ui.make_persistent_id(("radius", self.relative));
                match dtool_design::numeric_field(ui, id, shown, &spec).1 {
                    NumericEvent::Commit(radius) => output = Some(corner::command(object.id, &points, None, radius, self.relative)),
                    NumericEvent::Preview(radius) => crate::app_canvas::stage_geometry_preview(ui, session,
                        corner::command(object.id, &points, None, radius, self.relative)),
                    NumericEvent::Cancel => crate::app_canvas::clear_geometry_preview(ui),
                    NumericEvent::None => {}
                }
                if kind != old_kind {
                    let radius = if value > 0.0 { value } else { (maximum * 0.2).min(if self.relative { 20.0 } else { 10.0 }) };
                    output = Some(corner::command(object.id, &points, kind, radius, self.relative));
                }
            });
            if dtool_design::button_with(ui, text("contour.bake"),
                &ControlState::enabled(editable && corner::has_corners(object))).clicked()
            {
                self.selected.clear();
                output = Some(Command::with_params(contour::BAKE, json!({"id":object.id})));
            }
        });
        output
    }

    pub(crate) fn input(&mut self, ui: &Ui, response: &Response, session: &Session) -> (bool, Option<Command>) {
        let Some(object) = corner::selected(session).filter(|object| corner::editable(session, object)) else {
            self.cancel();
            return (false, None);
        };
        let Some(curve) = corner::source(object) else { self.cancel(); return (false, None); };
        let markers = markers(session, object, &curve, response.rect);
        self.sync(session, object, &markers);
        let generation = session.generation;
        if ui.input(|input| input.key_pressed(egui::Key::Escape))
            || self.gesture.as_ref().is_some_and(|gesture| match gesture {
                Gesture::Radius { generation: started, .. } | Gesture::Marquee { generation: started, .. } => *started != generation,
            })
        {
            self.cancel();
            crate::app_canvas::clear_geometry_preview(ui);
            return (true, None);
        }
        let hover = response.hover_pos().and_then(|point| marker_at(&markers, point));
        if hover.is_some() { ui.ctx().set_cursor_icon(egui::CursorIcon::Grab); }
        let (pressed, released) = crate::input::primary_button_positions(ui);
        if let Some(start) = pressed.filter(|_| response.contains_pointer()) {
            let extend = ui.input(|input| input.modifiers.shift || input.modifiers.ctrl);
            if let Some(marker) = marker_at(&markers, start) {
                if extend {
                    if let Some(index) = self.selected.iter().position(|key| *key == marker.key) {
                        self.selected.remove(index);
                    } else {
                        self.selected.push(marker.key);
                    }
                    return (true, None);
                }
                if !self.selected.contains(&marker.key) { self.selected = vec![marker.key]; }
                self.gesture = Some(Gesture::Radius { start, anchor: marker.key, original: marker.radius, axis: marker.axis, generation });
            } else {
                self.gesture = Some(Gesture::Marquee { start, current: start, generation, extend });
            }
        }
        let (down, latest) = ui.input(|input| (input.pointer.primary_down(), input.pointer.latest_pos()));
        let (Some(mut gesture), Some(pointer)) = (self.gesture.take(), released.or(latest).filter(|_| down || released.is_some())) else {
            return (hover.is_some(), None);
        };
        match &mut gesture {
            Gesture::Marquee { start, current, extend, .. } => {
                let moved = start.distance(pointer) > 4.0;
                if released.is_some() {
                    if moved {
                        if !*extend { self.selected.clear(); }
                        let box_rect = Rect::from_two_pos(*start, pointer);
                        for marker in markers.iter().filter(|marker| box_rect.contains(marker.anchor)) {
                            if !self.selected.contains(&marker.key) { self.selected.push(marker.key); }
                        }
                    }
                    return (moved, None);
                }
                *current = pointer;
                self.gesture = Some(gesture);
                (true, None)
            }
            Gesture::Radius { start, original, axis, anchor, .. } => {
                let moved = start.distance(pointer) >= 2.0 && axis.length_sq() > 0.000001;
                let points = self.effective_anchors(&markers);
                let command = if response.double_clicked() {
                    Some(corner::command(object.id, &points, Some(CornerKind::None), 0.0, false))
                } else if moved {
                    let delta = f64::from((pointer - *start).dot(*axis) / axis.length_sq());
                    let radius = (*original + delta).max(0.0);
                    let radius = if self.relative {
                        let maximum = limits(&curve, &points).into_iter().zip(&points)
                            .find_map(|((_, maximum, _), key)| (key == anchor).then_some(maximum)).unwrap_or(0.0);
                        target_value(radius, maximum, true)
                    } else { radius };
                    Some(corner::command(object.id, &points, None, radius, self.relative))
                } else { None };
                if released.is_some() {
                    crate::app_canvas::clear_geometry_preview(ui);
                    return (true, command);
                }
                if let Some(command) = command {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    crate::app_canvas::stage_geometry_preview(ui, session, command);
                }
                self.gesture = Some(gesture);
                (true, None)
            }
        }
    }

    pub(crate) fn paint(&self, ui: &Ui, rect: Rect, session: &Session) {
        let Some(object) = corner::selected(session).filter(|object| corner::editable(session, object)) else { return; };
        let Some(curve) = corner::source(object) else { return; };
        let painter = ui.painter_at(rect);
        let accent = crate::canvas_input::SELECTION_BLUE;
        let surface = egui::Color32::WHITE;
        let stroke = Stroke::new(1.0, accent);
        for marker in markers(session, object, &curve, rect) {
            let selected = self.owner == Some((session.document.instance, object.id)) && self.selected.contains(&marker.key);
            painter.circle_filled(marker.anchor, 4.0, if selected { accent } else { surface });
            painter.circle_stroke(marker.anchor, 4.0, stroke);
            if marker.radius > 0.0 {
                painter.line_segment([marker.anchor, marker.handle], stroke);
                painter.circle_filled(marker.handle, 4.5, surface);
                painter.circle_stroke(marker.handle, 4.5, stroke);
            }
        }
        if let Some(Gesture::Marquee { start, current, .. }) = self.gesture {
            let bounds = Rect::from_two_pos(start, current);
            painter.rect_filled(bounds, 0.0, accent.gamma_multiply(0.1));
            painter.rect_stroke(bounds, 0.0, stroke, egui::StrokeKind::Inside);
        }
    }
}
