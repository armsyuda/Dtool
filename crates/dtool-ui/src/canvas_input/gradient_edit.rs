use super::{screen, ToolKind};
use dtool_core::commands::appearance::Kind;
use dtool_core::commands::context_toolbar::gradient::Target as Role;
use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};
use dtool_core::object::ObjectId;
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientSpread, GradientStop, Paint, Transparency, gradient_parameter, interpolate_stops};
use dtool_core::{Object, Session};
use dtool_protocol::Command;
use egui::{Color32, Pos2, Rect, Stroke, Ui};
use serde_json::json;

mod bitmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    Start,
    End,
    Stop(usize),
    Axis,
    Center,
    FirstColour,
    Focus,
    FocusRadius,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gradient {
    pub(crate) id: ObjectId,
    role: Role,
    kind: GradientKind,
    stops: Vec<GradientStop>,
    geom: GradientGeom,
}

impl Gradient {
    pub(crate) fn changes_scene_paint(&self) -> bool {
        self.role != Role::Transparency
    }

    pub(crate) fn swap_with_object(&mut self, object: &mut Object) -> bool {
        let (kind, stops, geom) = if self.role == Role::Transparency {
            let Transparency::Gradient { kind, stops, geom } = &mut object.transparency else { return false };
            (kind, stops, geom)
        } else {
            let paint = match self.role {
                Role::Fill => Some(&mut object.fill),
                Role::Stroke => Some(&mut object.stroke.paint),
                Role::Appearance(slot) => match (slot.kind, slot.index) {
                    (Kind::Fill, 0) => Some(&mut object.fill),
                    (Kind::Stroke, 0) => Some(&mut object.stroke.paint),
                    (Kind::Fill, index) => object.additional_fills.get_mut(index - 1),
                    (Kind::Stroke, index) => object.additional_strokes.get_mut(index - 1).map(|stroke| &mut stroke.paint),
                },
                Role::Transparency => None,
            };
            let Some(Paint::Gradient { kind, stops, geom }) = paint else { return false };
            (kind, stops, geom)
        };
        if *kind != self.kind {
            return false;
        }
        std::mem::swap(stops, &mut self.stops);
        std::mem::swap(geom, &mut self.geom);
        true
    }

    fn selected(session: &Session) -> Option<(&Object, Self)> {
        let (object, role) = Role::selected(session, &session.active_tool)?;
        let (kind, stops, geom) = role.gradient(object)?;
        if stops.len() < 2 {
            return None;
        }
        Some((object, Self { id: object.id, role, kind, stops: stops.to_vec(), geom: *geom }))
    }

    fn command(self, object: &Object) -> Command {
        self.role.command(object, self.kind, self.stops, self.geom)
    }

    fn point(&self, offset: f64) -> [f64; 2] {
        let [x, y] = self.geom.start;
        let [ex, ey] = self.geom.end;
        let (dx, dy) = (ex - x, ey - y);
        if self.kind == GradientKind::Conical {
            let (sin, cos) = (offset * std::f64::consts::TAU).sin_cos();
            let [dx, dy, sx, sy, ..] = self.geom.gradient_matrix(self.kind);
            [x + dx * cos + sx * sin, y + dy * cos + sy * sin]
        } else {
            [x + dx * offset, y + dy * offset]
        }
    }

    fn stop_offset(&self, point: [f64; 2]) -> Option<f64> {
        if self.kind == GradientKind::Conical {
            let [x, y] = self.geom.start;
            if (point[0] - x).hypot(point[1] - y) <= 1e-10 {
                return None;
            }
            Some(f64::from(gradient_parameter(self.kind, point[0], point[1], &self.geom)))
        } else {
            self.projection(point).map(|(offset, _)| offset)
        }
    }

    fn projection(&self, point: [f64; 2]) -> Option<(f64, f64)> {
        let dir: [f64; 2] = std::array::from_fn(|i| self.geom.end[i] - self.geom.start[i]);
        let rel: [f64; 2] = std::array::from_fn(|i| point[i] - self.geom.start[i]);
        let length_squared = dir[0].mul_add(dir[0], dir[1] * dir[1]);
        if length_squared <= 1e-10 || !length_squared.is_finite() {
            return None;
        }
        Some(((rel[0] * dir[0] + rel[1] * dir[1]) / length_squared,
            (rel[1] * dir[0] - rel[0] * dir[1]) / length_squared))
    }

    fn axis(&self) -> Option<[f64; 2]> {
        (self.kind == GradientKind::Elliptical).then(|| {
            let [x, y] = self.geom.start;
            let [ex, ey] = self.geom.end;
            [x - (ey - y) * self.geom.ratio, y + (ex - x) * self.geom.ratio]
        })
    }

    fn focus_point(&self) -> Option<[f64; 2]> {
        let [x, y] = self.geom.focus?;
        if !matches!(self.kind, GradientKind::Radial | GradientKind::Elliptical) {
            return None;
        }
        let geom = if self.kind == GradientKind::Radial { GradientGeom { ratio: 1.0, ..self.geom } } else { self.geom };
        let [a, b, c, d, e, f] = geom.to_matrix();
        Some([a * x + c * y + e, b * x + d * y + f])
    }

    fn focal_point_at(&self, angle: f64) -> [f64; 2] {
        let [fx, fy] = self.geom.focus.unwrap_or([0.0; 2]);
        let (sin, cos) = angle.sin_cos();
        let geom = if self.kind == GradientKind::Radial { GradientGeom { ratio: 1.0, ..self.geom } } else { self.geom };
        let [a, b, c, d, x, y] = geom.to_matrix();
        let u = fx + self.geom.focus_radius * cos;
        let v = fy + self.geom.focus_radius * sin;
        [x + a * u + c * v, y + b * u + d * v]
    }

    fn has_focal_circle(&self) -> bool {
        matches!(self.kind, GradientKind::Radial | GradientKind::Elliptical) && self.geom.focus_radius > 0.0
    }

    fn handles(&self, project: impl Fn([f64; 2]) -> Pos2) -> impl Iterator<Item = (Handle, [f64; 2])> + '_ {
        let (start, first_colour) = if self.kind == GradientKind::Conical {
            let radius = project(self.geom.start).distance(project(self.geom.end));
            let inward = f64::from((16.0 / radius).min(0.5));
            let colour = std::array::from_fn(|i| self.geom.end[i] + (self.geom.start[i] - self.geom.end[i]) * inward);
            (Handle::Center, Some((Handle::FirstColour, colour)))
        } else {
            (Handle::Start, None)
        };
        [(start, self.geom.start), (Handle::End, self.geom.end)]
            .into_iter()
            .chain(first_colour)
            .chain(self.axis().map(|point| (Handle::Axis, point)))
            .chain(self.focus_point().map(|point| (Handle::Focus, point)))
            .chain(self.has_focal_circle().then(|| (Handle::FocusRadius, self.focal_point_at(-std::f64::consts::FRAC_PI_2))))
            .chain(self.stops.iter().enumerate().skip(1).take(self.stops.len() - 2)
                .map(|(index, stop)| (Handle::Stop(index), self.point(f64::from(stop.offset)))))
    }

    fn move_handle(&mut self, handle: Handle, point: [f64; 2]) {
        match handle {
            Handle::Focus => {
                if let Some((x, mut y)) = self.projection(point) {
                    if self.kind == GradientKind::Elliptical { y /= self.geom.ratio.max(1e-8); }
                    let limit = (0.999 - self.geom.focus_radius).max(0.0);
                    let scale = limit / x.hypot(y).max(limit).max(1e-8);
                    self.geom.focus = Some([x * scale, y * scale]);
                }
            }
            Handle::FocusRadius => {
                if let Some((u, v)) = self.projection(point) {
                    let [fx, fy] = self.geom.focus.unwrap_or([0.0; 2]);
                    let ratio = if self.kind == GradientKind::Elliptical { self.geom.ratio } else { 1.0 };
                    let radius = (u - fx).hypot(v / ratio - fy);
                    self.geom.focus_radius = radius.min((0.999 - fx.hypot(fy)).max(0.0));
                }
            }
            Handle::Center => {
                for (i, value) in point.into_iter().enumerate() {
                    self.geom.end[i] += value - self.geom.start[i];
                }
                self.geom.start = point;
            }
            Handle::FirstColour => {}
            Handle::Start | Handle::End => {
                let other = if handle == Handle::Start { self.geom.end } else { self.geom.start };
                if (point[0] - other[0]).hypot(point[1] - other[1]) >= 0.1 {
                    if handle == Handle::Start { self.geom.start = point; } else { self.geom.end = point; }
                }
            }
            Handle::Axis => {
                if let Some((_, side)) = self.projection(point) {
                    self.geom.ratio = side.abs().clamp(0.05, 20.0);
                }
            }
            Handle::Stop(index) => {
                if let Some(mut offset) = self.stop_offset(point) {
                    if self.kind == GradientKind::Conical {
                        // Stay on the same side of the angular seam while dragging a stop.
                        let delta = offset - f64::from(self.stops[index].offset);
                        offset -= delta.round();
                    }
                    let left = self.stops[index - 1].offset.clamp(0.0, 1.0);
                    let right = self.stops[index + 1].offset.clamp(left, 1.0);
                    let gap = ((right - left) * 0.5).min(0.001);
                    self.stops[index].offset = if gap > 0.0 {
                        (offset as f32).clamp(left + gap, right - gap)
                    } else {
                        left
                    };
                }
            }
        }
    }

    fn add(&mut self, offset: f32) -> Option<usize> {
        if !(0.005..0.995).contains(&offset) || self.stops.len() >= 256
            || self.stops.iter().any(|stop| (stop.offset - offset).abs() < 0.005)
        {
            return None;
        }
        let colour = interpolate_stops(&self.stops, offset);
        let index = self.stops.partition_point(|stop| stop.offset <= offset);
        self.stops.insert(index, GradientStop { offset, colour });
        Some(index)
    }
}

#[derive(Debug, Clone)]
struct Edit {
    gradient: Gradient,
    handle: Handle,
    grab: [f64; 2],
}

#[derive(Debug, Clone, Default)]
pub(super) struct GradientEditor {
    selected: Option<(ObjectId, Role, usize)>,
    drag: Option<Edit>,
    bitmap_drag: Option<bitmap::Edit>,
    entry: Option<(ObjectId, Role, usize, Colour, String)>,
}

impl GradientEditor {
    pub(super) fn preview(&self, session: &Session) -> Option<&Gradient> {
        let gradient = &self.drag.as_ref()?.gradient;
        let (object, role) = Role::selected(session, &session.active_tool)?;
        let (kind, stops, geom) = role.gradient(object)?;
        ((gradient.id, gradient.role, gradient.kind) == (object.id, role, kind)
            && (gradient.stops != stops || gradient.geom != *geom)).then_some(gradient)
    }

    pub(super) fn active(&self) -> bool {
        self.drag.is_some() || self.bitmap_drag.is_some()
    }

    pub(super) fn cancel(&mut self) {
        self.drag = None;
        self.bitmap_drag = None;
    }

    fn selected_index(&self, gradient: &Gradient) -> usize {
        self.selected.filter(|(id, role, _)| (*id, *role) == (gradient.id, gradient.role))
            .map_or(0, |(_, _, index)| index.min(gradient.stops.len() - 1))
    }

    fn select(&mut self, gradient: &Gradient, index: usize) {
        self.selected = Some((gradient.id, gradient.role, index));
    }

    fn hit_handle(session: &Session, canvas: Rect, at: Pos2, object: &Object, gradient: &Gradient) -> Option<(Handle, [f64; 2])> {
        let project = |point| screen(canvas, &session.view, to_world(object, point));
        gradient.handles(project).filter_map(|(handle, point)| {
            let distance = at.distance(project(point));
            (distance <= 8.0).then_some((handle, point, distance))
        }).min_by(|a, b| a.2.total_cmp(&b.2)).map(|(handle, point, _)| (handle, point))
    }

    pub(super) fn handle_cursor(session: &Session, canvas: Rect, at: Pos2) -> Option<egui::CursorIcon> {
        if bitmap::Edit::hovering(session, canvas, at) {
            return Some(egui::CursorIcon::Grab);
        }
        let (object, gradient) = Gradient::selected(session)?;
        let (handle, _) = Self::hit_handle(session, canvas, at, object, &gradient)?;
        Some(if handle == Handle::FirstColour { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Grab })
    }

    pub(super) fn press(&mut self, session: &Session, canvas: Rect, at: Pos2, doc: [f32; 2]) -> bool {
        if let Some(edit) = bitmap::Edit::begin(session, canvas, at, doc) {
            self.bitmap_drag = Some(edit);
            return true;
        }
        let Some((object, gradient)) = Gradient::selected(session) else { return false };
        let Some((handle, point)) = Self::hit_handle(session, canvas, at, object, &gradient) else { return false };
        let Some(local) = to_local(object, doc) else { return false };
        if let Some(index) = match handle {
            Handle::Start | Handle::FirstColour => Some(0),
            Handle::End => Some(gradient.stops.len() - 1),
            Handle::Stop(index) => Some(index),
            Handle::Axis | Handle::Center | Handle::Focus | Handle::FocusRadius => None,
        } {
            self.select(&gradient, index);
        }
        if handle == Handle::FirstColour {
            return true;
        }
        let grab = std::array::from_fn(|i| point[i] - local[i]);
        self.drag = Some(Edit { gradient, handle, grab });
        true
    }

    pub(super) fn motion(&mut self, session: &Session, point: [f32; 2], release: bool) -> Option<Command> {
        if let Some(edit) = &mut self.bitmap_drag {
            return edit.motion(session, point, release);
        }
        let edit = self.drag.as_mut()?;
        let (object, original) = Gradient::selected(session)
            .filter(|(_, gradient)| (gradient.id, gradient.role, gradient.kind) == (edit.gradient.id, edit.gradient.role, edit.gradient.kind))?;
        let local = to_local(object, point)?;
        edit.gradient.move_handle(edit.handle, std::array::from_fn(|i| local[i] + edit.grab[i]));
        if !release {
            return None;
        }
        let finished = self.drag.take()?.gradient;
        (finished.geom != original.geom || finished.stops != original.stops).then(|| finished.command(object))
    }

    pub(super) fn insert(&mut self, session: &Session, canvas: Rect, at: Pos2, doc: [f32; 2]) -> Option<Command> {
        let (object, mut gradient) = Gradient::selected(session)?;
        let offset = gradient.stop_offset(to_local(object, doc)?)?;
        if at.distance(screen(canvas, &session.view, to_world(object, gradient.point(offset)))) > 8.0 {
            return None;
        }
        let index = gradient.add(offset as f32)?;
        self.select(&gradient, index);
        Some(gradient.command(object))
    }

    pub(super) fn paint(&self, ui: &Ui, canvas: Rect, session: &Session, stroke: Stroke) {
        if bitmap::Edit::paint(self.bitmap_drag.as_ref(), ui, canvas, session, stroke) {
            return;
        }
        let Some((object, saved)) = Gradient::selected(session) else { return };
        let gradient = self.drag.as_ref()
            .filter(|edit| (edit.gradient.id, edit.gradient.role, edit.gradient.kind) == (saved.id, saved.role, saved.kind))
            .map_or(&saved, |edit| &edit.gradient);
        let painter = ui.painter_at(canvas);
        let project = |pt| screen(canvas, &session.view, to_world(object, pt));
        painter.line_segment([project(gradient.geom.start), project(gradient.geom.end)], stroke);
        if gradient.kind == GradientKind::Conical {
            let center = project(gradient.geom.start);
            let radius = center.distance(project(gradient.geom.end)).max(center.distance(project(gradient.point(0.25))));
            let segments = (radius * 0.5).clamp(48.0, 256.0) as u32;
            let points = (0..segments).map(|i| project(gradient.point(f64::from(i) / f64::from(segments)))).collect();
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        if let Some(axis) = gradient.axis() {
            painter.extend(egui::Shape::dashed_line(&[project(gradient.geom.start), project(axis)], stroke, 4.0, 4.0));
        }
        if gradient.has_focal_circle() {
            let points = (0..48).map(|i| {
                project(gradient.focal_point_at(f64::from(i) * std::f64::consts::TAU / 48.0))
            }).collect();
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        let selected = self.selected_index(gradient);
        for (handle, position) in gradient.handles(project) {
            let at = project(position);
            let index = match handle {
                Handle::Start | Handle::FirstColour => 0,
                Handle::End => gradient.stops.len() - 1,
                Handle::Stop(index) => index,
                Handle::Axis | Handle::Center | Handle::FocusRadius => {
                    painter.circle(at, 4.0, ui.visuals().panel_fill, stroke);
                    continue;
                }
                Handle::Focus => {
                    let points = [(0.0, -6.0), (6.0, 0.0), (0.0, 6.0), (-6.0, 0.0)]
                        .map(|(x, y)| at + egui::vec2(x, y)).to_vec();
                    painter.add(egui::Shape::convex_polygon(points, ui.visuals().panel_fill, stroke));
                    continue;
                }
            };
            let colour = gradient.stops[index].colour;
            let fill = if gradient.role == Role::Transparency {
                Color32::from_gray(colour.a)
            } else {
                Color32::from_rgb(colour.r, colour.g, colour.b)
            };
            let size = if index == selected { 10.0 } else { 8.0 };
            if gradient.kind == GradientKind::Conical {
                painter.circle(at, size * 0.5, fill, stroke);
            } else {
                let square = Rect::from_center_size(at, egui::Vec2::splat(size));
                painter.rect_filled(square, 1.0, fill);
                painter.rect_stroke(square, 1.0, stroke, egui::StrokeKind::Outside);
            }
        }
    }

    pub(super) fn toolbar(&mut self, ui: &mut Ui, session: &Session) -> Option<Command> {
        if matches!(super::tool_kind(&session.active_tool), ToolKind::Gradient(_))
            && let Some(focus) = session.appearance_focus.filter(|focus| session.selection.ids.as_slice() == [focus.node])
        {
            let label = if !focus.active(session) {
                dtool_strings::text("appearance.gradient.reselect").to_owned()
            } else {
                let kind = dtool_strings::text(match focus.slot.kind {
                    Kind::Fill => "context.gradient.target.fill",
                    Kind::Stroke => "context.gradient.target.stroke",
                });
                format!("{} {} {}", dtool_strings::text("panel.appearance"), kind, focus.slot.index + 1)
            };
            ui.label(label);
            if dtool_design::button(ui, dtool_strings::text("appearance.gradient.clear")).clicked() {
                return Some(Command::with_params("appearance.focus", json!({"clear": true})));
            }
        }
        if Role::selected_bitmap_target(session, &session.active_tool).is_some_and(|(object, target)| {
            matches!(target.paint(object), Some(dtool_core::paint::Paint::Bitmap(_)))
        }) {
            let replace = dtool_design::button(ui, dtool_strings::text("ctx.gradient.bitmap.replace"))
                .on_hover_text(dtool_strings::text("ctx.gradient.bitmap.hint"));
            return replace.clicked().then(|| Command::with_params(dtool_core::context_toolbar::OPTION_SET, json!({
                "id": "gradient.type", "value": "bitmap", "tool": session.active_tool,
            })));
        }
        let (object, mut gradient) = Gradient::selected(session)?;
        let mut output = None;
        let button = dtool_design::button(ui, dtool_strings::text("ctx.gradient.edit"))
            .on_hover_text(dtool_strings::text("ctx.gradient.add_hint"));
        egui::Popup::menu(&button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
            if gradient.role != Role::Transparency && gradient.kind != GradientKind::Conical {
                let original = gradient.geom.spread;
                ui.label(dtool_strings::text("ctx.gradient.spread"));
                ui.horizontal(|ui| {
                    for (spread, label) in [
                        (GradientSpread::Pad, "ctx.gradient.spread.pad"),
                        (GradientSpread::Repeat, "ctx.gradient.spread.repeat"),
                        (GradientSpread::Reflect, "ctx.gradient.spread.reflect"),
                    ] {
                        ui.selectable_value(&mut gradient.geom.spread, spread, dtool_strings::text(label));
                    }
                });
                if original != gradient.geom.spread {
                    output = Some(gradient.clone().command(object));
                }
                if matches!(gradient.kind, GradientKind::Radial | GradientKind::Elliptical) {
                    let label = if gradient.geom.focus.is_some() { "ctx.gradient.focus.reset" } else { "ctx.gradient.focus.add" };
                    if dtool_design::button(ui, dtool_strings::text(label)).clicked() {
                        gradient.geom.focus = gradient.geom.focus.is_none().then_some([(0.999 - gradient.geom.focus_radius).clamp(0.0, 0.25), 0.0]);
                        output = Some(gradient.clone().command(object));
                    }
                    let label = if gradient.has_focal_circle() { "ctx.gradient.focal_circle.remove" } else { "ctx.gradient.focal_circle.add" };
                    if dtool_design::button(ui, dtool_strings::text(label)).clicked() {
                        let [fx, fy] = gradient.geom.focus.unwrap_or([0.0; 2]);
                        gradient.geom.focus_radius = if gradient.has_focal_circle() { 0.0 }
                            else { (0.999 - fx.hypot(fy)).clamp(0.0, 0.25) };
                        output = Some(gradient.clone().command(object));
                    }
                }
                ui.separator();
            }
            let index = self.selected_index(&gradient);
            ui.label(format!("{} {}/{}", dtool_strings::text("ctx.gradient.stop"), index + 1, gradient.stops.len()));
            let entry_color = gradient.stops[index].colour;
            let key = (gradient.id, gradient.role, index, entry_color);
            if self.entry.as_ref().is_none_or(|entry| (entry.0, entry.1, entry.2, entry.3) != key) {
                let value = if gradient.role == Role::Transparency {
                    format!("{:.1}", f32::from(entry_color.a) * 100.0 / 255.0)
                } else {
                    format!("#{:02X}{:02X}{:02X}{:02X}", entry_color.r, entry_color.g, entry_color.b, entry_color.a)
                };
                self.entry = Some((key.0, key.1, key.2, key.3, value));
            }
            if let Some((_, _, _, _, value)) = &mut self.entry {
                let label = if gradient.role == Role::Transparency { "ctx.gradient.opacity" } else { "ctx.gradient.rgba" };
                ui.label(dtool_strings::text(label));
                let applied = ui.horizontal(|ui| {
                    let input = ui.add(egui::TextEdit::singleline(value).desired_width(90.0));
                    let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let apply = dtool_design::button(ui, dtool_strings::text("ctx.gradient.apply"));
                    (input.lost_focus() && enter) || apply.clicked()
                }).inner;
                if applied {
                    let colour = if gradient.role == Role::Transparency {
                        value.trim().parse::<f32>().ok()
                            .filter(|percent| percent.is_finite() && (0.0..=100.0).contains(percent))
                            .map(|percent| Colour::new(0, 0, 0, (percent * 255.0 / 100.0).round() as u8))
                    } else {
                        rgba_hex(value)
                    };
                    if let Some(colour) = colour.filter(|colour| *colour != entry_color) {
                        gradient.stops[index].colour = colour;
                        output = Some(gradient.clone().command(object));
                    }
                }
            }
            if dtool_design::button(ui, dtool_strings::text("ctx.gradient.reverse")).clicked() {
                for stop in &mut gradient.stops { stop.offset = 1.0 - stop.offset; }
                gradient.stops.reverse();
                self.select(&gradient, gradient.stops.len() - 1 - index);
                output = Some(gradient.clone().command(object));
            }
            if ui.add_enabled_ui(index > 0 && index + 1 < gradient.stops.len(), |ui| {
                dtool_design::button(ui, dtool_strings::text("ctx.gradient.delete_stop")).clicked()
            }).inner {
                gradient.stops.remove(index);
                self.select(&gradient, index - 1);
                output = Some(gradient.clone().command(object));
            }
            if gradient.role == Role::Transparency
                && dtool_design::button(ui, dtool_strings::text("ctx.gradient.clear_transparency")).clicked()
            {
                output = Some(Command::with_params("object.update", json!({"id": object.id, "transparency": Transparency::None})));
                ui.close();
            }
        });
        output
    }
}

fn to_world(object: &Object, point: [f64; 2]) -> [f32; 2] {
    apply_matrix(&object_matrix(object), std::array::from_fn(|i| object.pos[i] + point[i] as f32))
}

fn to_local(object: &Object, point: [f32; 2]) -> Option<[f64; 2]> {
    let matrix = invert_matrix(&object_matrix(object))?;
    let point = apply_matrix(&matrix, point);
    Some(std::array::from_fn(|i| f64::from(point[i] - object.pos[i])))
}

fn rgba_hex(value: &str) -> Option<Colour> {
    let value = value.trim().trim_start_matches('#');
    if !value.is_ascii() || !matches!(value.len(), 6 | 8) {
        return None;
    }
    let component = |index| u8::from_str_radix(&value[index..index + 2], 16).ok();
    Some(Colour::new(component(0)?, component(2)?, component(4)?, if value.len() == 8 { component(6)? } else { 255 }))
}
