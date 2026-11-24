use super::{screen, ToolKind};
use dtool_core::commands::appearance::{Kind, PaintSlot};
use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};
use dtool_core::object::ObjectId;
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint, Transparency, interpolate_stops};
use dtool_core::{Object, Session};
use dtool_protocol::Command;
use egui::{Color32, Pos2, Rect, Stroke, Ui};
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Fill,
    Stroke,
    Appearance(PaintSlot),
    Transparency,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    Start,
    End,
    Stop(usize),
    Axis,
}

#[derive(Debug, Clone)]
struct Gradient {
    id: ObjectId,
    role: Role,
    kind: GradientKind,
    stops: Vec<GradientStop>,
    geom: GradientGeom,
}

impl Gradient {
    fn selected(session: &Session) -> Option<(&Object, Self)> {
        let [id] = session.selection.ids.as_slice() else { return None };
        let role = match super::tool_kind(&session.active_tool) {
            ToolKind::Transparency => Role::Transparency,
            ToolKind::Gradient(_) => {
                if let Some(focus) = session.appearance_focus.filter(|focus| focus.node == *id) {
                    if !focus.active(session) { return None; }
                    Role::Appearance(focus.slot)
                } else if session.tool_options.choice(&session.active_tool, "gradient.target").as_deref() == Some("stroke") {
                    Role::Stroke
                } else {
                    Role::Fill
                }
            }
            _ => return None,
        };
        let object = session.document.get_object(*id).filter(|object| !object.locked)?;
        let (kind, stops, geom) = match role {
            Role::Transparency => {
                let Transparency::Gradient { kind, stops, geom } = &object.transparency else { return None };
                (kind, stops, geom)
            }
            Role::Fill | Role::Stroke => {
                let paint = if role == Role::Fill { &object.fill } else { &object.stroke.paint };
                let Paint::Gradient { kind, stops, geom } = paint else { return None };
                (kind, stops, geom)
            }
            Role::Appearance(slot) => {
                let Paint::Gradient { kind, stops, geom } = slot.paint(object)? else { return None; };
                (kind, stops, geom)
            }
        };
        if stops.len() < 2 {
            return None;
        }
        Some((object, Self { id: *id, role, kind: *kind, stops: stops.clone(), geom: geom.clone() }))
    }

    fn command(self, object: &Object) -> Command {
        let Self { id, role, kind, stops, geom } = self;
        let params = match role {
            Role::Appearance(slot) => return slot.paint_command(id, Paint::Gradient { kind, stops, geom }),
            Role::Transparency => json!({"id": id, "transparency": Transparency::Gradient { kind, stops, geom }}),
            Role::Fill => json!({"id": id, "fill": Paint::Gradient { kind, stops, geom }}),
            Role::Stroke => {
                let mut stroke = object.stroke.clone();
                stroke.paint = Paint::Gradient { kind, stops, geom };
                json!({"id": id, "stroke": stroke})
            }
        };
        Command::with_params("object.update", params)
    }

    fn point(&self, offset: f64) -> [f64; 2] {
        std::array::from_fn(|i| self.geom.start[i] + (self.geom.end[i] - self.geom.start[i]) * offset)
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

    fn handles(&self) -> impl Iterator<Item = (Handle, [f64; 2])> + '_ {
        [(Handle::Start, self.geom.start), (Handle::End, self.geom.end)]
            .into_iter()
            .chain(self.axis().map(|point| (Handle::Axis, point)))
            .chain(self.stops.iter().enumerate().skip(1).take(self.stops.len() - 2)
                .map(|(index, stop)| (Handle::Stop(index), self.point(f64::from(stop.offset)))))
    }

    fn move_handle(&mut self, handle: Handle, point: [f64; 2]) {
        match handle {
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
                if let Some((offset, _)) = self.projection(point) {
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
    entry: Option<(ObjectId, Role, usize, Colour, String)>,
}

impl GradientEditor {
    pub(super) fn active(&self) -> bool {
        self.drag.is_some()
    }

    pub(super) fn cancel(&mut self) {
        self.drag = None;
    }

    fn selected_index(&self, gradient: &Gradient) -> usize {
        self.selected.filter(|(id, role, _)| (*id, *role) == (gradient.id, gradient.role))
            .map_or(0, |(_, _, index)| index.min(gradient.stops.len() - 1))
    }

    fn select(&mut self, gradient: &Gradient, index: usize) {
        self.selected = Some((gradient.id, gradient.role, index));
    }

    pub(super) fn press(&mut self, session: &Session, canvas: Rect, at: Pos2, doc: [f32; 2]) -> bool {
        let Some((object, gradient)) = Gradient::selected(session) else { return false };
        let Some((handle, point)) = gradient.handles().filter_map(|(handle, point)| {
            let distance = at.distance(screen(canvas, &session.view, to_world(object, point)));
            (distance <= 8.0).then_some((handle, point, distance))
        }).min_by(|a, b| a.2.total_cmp(&b.2)).map(|(handle, point, _)| (handle, point)) else { return false };
        let Some(local) = to_local(object, doc) else { return false };
        if let Some(index) = match handle {
            Handle::Start => Some(0),
            Handle::End => Some(gradient.stops.len() - 1),
            Handle::Stop(index) => Some(index),
            Handle::Axis => None,
        } {
            self.select(&gradient, index);
        }
        let grab = std::array::from_fn(|i| point[i] - local[i]);
        self.drag = Some(Edit { gradient, handle, grab });
        true
    }

    pub(super) fn motion(&mut self, session: &Session, point: [f32; 2], release: bool) -> Option<Command> {
        let edit = self.drag.as_mut()?;
        let (object, original) = Gradient::selected(session)
            .filter(|(_, gradient)| (gradient.id, gradient.role) == (edit.gradient.id, edit.gradient.role))?;
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
        let (offset, _) = gradient.projection(to_local(object, doc)?)?;
        if at.distance(screen(canvas, &session.view, to_world(object, gradient.point(offset)))) > 8.0 {
            return None;
        }
        let index = gradient.add(offset as f32)?;
        self.select(&gradient, index);
        Some(gradient.command(object))
    }

    pub(super) fn paint(&self, ui: &Ui, canvas: Rect, session: &Session, stroke: Stroke) {
        let Some((object, saved)) = Gradient::selected(session) else { return };
        let gradient = self.drag.as_ref()
            .filter(|edit| (edit.gradient.id, edit.gradient.role) == (saved.id, saved.role))
            .map_or(&saved, |edit| &edit.gradient);
        let painter = ui.painter_at(canvas);
        let project = |pt| screen(canvas, &session.view, to_world(object, pt));
        painter.line_segment([project(gradient.geom.start), project(gradient.geom.end)], stroke);
        if let Some(axis) = gradient.axis() {
            painter.extend(egui::Shape::dashed_line(&[project(gradient.geom.start), project(axis)], stroke, 4.0, 4.0));
        }
        let selected = self.selected_index(gradient);
        for (handle, position) in gradient.handles() {
            let at = project(position);
            let index = match handle {
                Handle::Start => 0,
                Handle::End => gradient.stops.len() - 1,
                Handle::Stop(index) => index,
                Handle::Axis => {
                    painter.circle(at, 4.0, ui.visuals().panel_fill, stroke);
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
            let square = Rect::from_center_size(at, egui::Vec2::splat(size));
            painter.rect_filled(square, 1.0, fill);
            painter.rect_stroke(square, 1.0, stroke, egui::StrokeKind::Outside);
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
        let (object, mut gradient) = Gradient::selected(session)?;
        let mut output = None;
        let button = dtool_design::button(ui, dtool_strings::text("ctx.gradient.edit"))
            .on_hover_text(dtool_strings::text("ctx.gradient.add_hint"));
        egui::Popup::menu(&button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
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
