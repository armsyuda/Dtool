use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::Paint;
use dtool_core::{Session, scene::Scene};
use dtool_protocol::Command;
use egui::{Pos2, Rect, Response, Stroke, Ui, Vec2};
use serde_json::json;

use crate::input::primary_button_positions;
use super::{CURSOR_EDGE, CURSOR_FACE, click_select_command, screen};

fn paint_width_cursor(ui: &Ui, rect: Rect, at: Pos2, normal: Vec2) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("stroke_width_cursor")))
        .with_clip_rect(rect);
    let tangent = Vec2::new(-normal.y, normal.x);
    let p = |x, y| at + normal * x + tangent * y;
    let segments = [
        [p(-7.0, 0.0), p(7.0, 0.0)],
        [p(-7.0, 0.0), p(-4.0, -3.0)],
        [p(-7.0, 0.0), p(-4.0, 3.0)],
        [p(7.0, 0.0), p(4.0, -3.0)],
        [p(7.0, 0.0), p(4.0, 3.0)],
    ];
    for stroke in [Stroke::new(3.4, CURSOR_EDGE), Stroke::new(2.3, CURSOR_FACE)] {
        for segment in segments {
            painter.line_segment(segment, stroke);
        }
    }
    painter.circle_filled(at, 2.5, CURSOR_EDGE);
    painter.circle_filled(at, 1.8, CURSOR_FACE);
}

#[derive(Debug, Clone, Default)]
pub(super) struct WidthTool {
    drag: Option<WidthDrag>,
    selected: Option<(ObjectId, f32)>,
}

#[derive(Debug, Clone)]
struct WidthDrag {
    id: ObjectId,
    path: Path,
    profile: Vec<[f32; 2]>,
    index: usize,
    at: f32,
    start: Vec2,
    width: f32,
    weight: f32,
    lock_width: bool,
    lock_order: bool,
    snap_ends: bool,
}

#[derive(Debug, Clone)]
struct Path {
    positions: Vec<Vec2>,
    distances: Vec<f32>,
    length: f32,
}

#[derive(Clone, Copy)]
struct Sample {
    pos: Vec2,
    tangent: Vec2,
    at: f32,
}

impl Sample {
    fn normal(self) -> Vec2 {
        egui::vec2(-self.tangent.y, self.tangent.x)
    }
}

impl Path {
    fn of(object: &Object) -> Option<Self> {
        let matrix = dtool_core::geometry::object_matrix(object);
        let (mut points, closed) = dtool_core::scene::doc_contours(object, &matrix).into_iter().next()?;
        points.dedup();
        if closed && points.first() != points.last() {
            points.push(*points.first()?);
        }
        let positions: Vec<_> = points.into_iter().map(|p| egui::vec2(p[0], p[1])).collect();
        let mut distances = vec![0.0];
        for pair in positions.windows(2) {
            distances.push(distances.last()? + (pair[1] - pair[0]).length());
        }
        let length = *distances.last()?;
        (length > f32::EPSILON).then_some(Self { positions, distances, length })
    }

    fn sample(&self, at: f32) -> Sample {
        let distance = at * self.length;
        let end = self.distances.partition_point(|d| *d <= distance).clamp(1, self.distances.len() - 1);
        let start = self.positions[end - 1];
        let tangent = (self.positions[end] - start).normalized();
        Sample { pos: start + tangent * (distance - self.distances[end - 1]), tangent, at }
    }

    fn nearest(&self, point: Vec2) -> Sample {
        let mut best = (f32::INFINITY, self.sample(0.0));
        for (i, pair) in self.positions.windows(2).enumerate() {
            let segment = pair[1] - pair[0];
            let fraction = ((point - pair[0]).dot(segment) / segment.length_sq()).clamp(0.0, 1.0);
            let pos = pair[0] + segment * fraction;
            let distance = (point - pos).length_sq();
            if distance < best.0 {
                let at = (self.distances[i] + segment.length() * fraction) / self.length;
                best = (distance, Sample { pos, tangent: segment.normalized(), at });
            }
        }
        best.1
    }
}

fn weight_at(profile: &[[f32; 2]], at: f32) -> f32 {
    let next = profile.partition_point(|p| p[0] <= at);
    match (next.checked_sub(1).and_then(|i| profile.get(i)), profile.get(next)) {
        (Some(&[a, x]), Some(&[b, y])) => x + (y - x) * (at - a) / (b - a),
        (Some(&[_, x]), _) | (_, Some(&[_, x])) => x,
        _ => 1.0,
    }
}

fn patch(id: ObjectId, width: Option<f32>, profile: Vec<[f32; 2]>) -> Command {
    Command::with_params("stroke.set", json!({ "id": id.0, "width": width, "pressure": profile }))
}

fn eligible(object: &Object) -> bool {
    !object.locked && object.pickable && object.stroke.width > 0.0 && object.stroke.paint != Paint::None
}

fn handle_at(path: &Path, profile: &[[f32; 2]], width: f32, point: Vec2, radius: f32) -> Option<usize> {
    profile.iter().enumerate().filter_map(|(i, &[at, weight])| {
        let sample = path.sample(at);
        let offset = sample.normal() * (width * weight * 0.5).max(radius * 0.7);
        let distance = [sample.pos, sample.pos + offset, sample.pos - offset].iter()
            .map(|p| (*p - point).length_sq()).fold(f32::INFINITY, f32::min);
        (distance <= radius * radius).then_some((i, distance))
    }).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
}

impl WidthDrag {
    fn edit(&self, pointer: Vec2, radius: f32) -> (f32, Vec<[f32; 2]>, f32) {
        let original = self.path.sample(self.at);
        let start_offset = (self.start - original.pos).dot(original.normal());
        let sign = if start_offset.abs() >= radius * 0.45 {
            start_offset.signum()
        } else {
            (pointer - self.start).dot(original.normal()).signum()
        };
        let sample = self.path.nearest(pointer);
        let mut at = if (pointer - self.start).dot(original.tangent).abs() >= radius * 0.5 {
            sample.at
        } else {
            self.at
        };
        if self.snap_ends {
            let threshold = radius / self.path.length;
            if at <= threshold { at = 0.0; }
            if at >= 1.0 - threshold { at = 1.0; }
        }
        if self.lock_order {
            let min = self.index.checked_sub(1).map_or(0.0, |i| (self.profile[i][0] + 0.0001).min(self.at));
            let max = self.profile.get(self.index + 1).map_or(1.0, |p| (p[0] - 0.0001).max(self.at));
            at = at.clamp(min, max);
        }
        let normal = self.path.sample(at).normal();
        let centre = self.path.sample(at).pos;
        let offset = (pointer - centre).dot(normal);
        let half_width = self.width * self.weight * 0.5;
        let target = (half_width + (offset - start_offset) * sign).max(0.0) * 2.0;
        let width = if self.lock_width { self.width } else { self.width.max(target) };
        let mut profile = self.profile.clone();
        profile.remove(self.index);
        for entry in &mut profile {
            entry[1] *= self.width / width;
        }
        Self::insert(&mut profile, [at, target / width]);
        (width, profile, at)
    }

    fn insert(points: &mut Vec<[f32; 2]>, entry: [f32; 2]) -> usize {
        let at = points.partition_point(|p| p[0] < entry[0]);
        if points.get(at).is_some_and(|p| p[0] == entry[0]) {
            points[at] = entry;
        } else {
            points.insert(at, entry);
        }
        at
    }
}

impl WidthTool {
    pub(super) fn active(&self) -> bool {
        self.drag.is_some()
    }

    pub(super) fn cancel(&mut self) {
        *self = Self::default();
    }

    pub(super) fn input(
        &mut self, ui: &Ui, response: &Response, session: &mut Session, scene: &Scene,
        to_doc: impl Fn(Pos2) -> [f32; 2], keyboard: bool,
    ) -> Option<Command> {
        let (down, up) = primary_button_positions(ui);
        let point = |pos| { let [x, y] = to_doc(pos); egui::vec2(x, y) };
        let radius = 7.0 / session.view.zoom.max(1e-4);
        let screen_pointer = ui.input(|input| input.pointer.latest_pos());
        let pointer = screen_pointer.map(point);
        let selected = match session.selection.ids.as_slice() {
            [id] => session.document.get_object(*id).filter(|object| eligible(object) && scene.quad(*id).is_some()),
            _ => None,
        };
        if let Some(at) = screen_pointer.filter(|at| response.hovered()
            || (self.drag.is_some() && response.rect.contains(*at)))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            let normal = self.drag.as_ref().map(|drag| drag.path.sample(drag.at).normal())
                .or_else(|| {
                    let object = selected?;
                    let path = Path::of(object)?;
                    let point = point(at);
                    let profile = &object.stroke.pressure;
                    if let Some(index) = handle_at(&path, profile, object.stroke.width, point, radius) {
                        return Some(path.sample(profile[index][0]).normal());
                    }
                    let sample = path.nearest(point);
                    ((sample.pos - point).length() <= radius).then(|| sample.normal())
                })
                .map(|normal| egui::emath::Rot2::from_angle(session.view.rotation_deg.to_radians()) * normal)
                .unwrap_or(Vec2::Y);
            paint_width_cursor(ui, response.rect, at, normal);
        }
        if let Some(object) = selected
            && keyboard && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
            && let Some(index) = self.selected.filter(|(id, _)| *id == object.id)
                .and_then(|(_, at)| object.stroke.pressure.iter().position(|point| point[0] == at))
                .or_else(|| handle_at(&Path::of(object)?, &object.stroke.pressure, object.stroke.width, pointer?, radius))
        {
            self.selected = None;
            let mut profile = object.stroke.pressure.clone();
            profile.remove(index);
            return Some(patch(object.id, None, profile));
        }
        if let Some(at) = down.filter(|pos| response.interact_rect.contains(*pos)
            && ui.ctx().layer_id_at(*pos) == Some(response.layer_id)
            && !egui::Popup::is_any_open(ui.ctx())).map(point)
        {
            response.request_focus();
            let hit = |object: &Object| {
                let path = Path::of(object)?;
                let index = handle_at(&path, &object.stroke.pressure, object.stroke.width, at, radius);
                let sample = path.nearest(at);
                (index.is_some() || (sample.pos - at).length() <= radius).then_some((path, index, sample.at))
            };
            let candidate = selected.and_then(|object| hit(object).map(|info| (object, info))).or_else(|| {
                session.document.objects.iter().rev().filter(|object| eligible(object) && scene.quad(object.id).is_some())
                    .find_map(|object| hit(object).map(|info| (object, info)))
            });
            if let Some((object, (path, index, t))) = candidate {
                let id = object.id;
                let width = object.stroke.width;
                let mut profile = object.stroke.pressure.clone();
                let index = index.unwrap_or_else(|| {
                    let weight = weight_at(&profile, t);
                    if profile.is_empty() {
                        profile.extend([[0.0, 1.0], [1.0, 1.0]]);
                    }
                    WidthDrag::insert(&mut profile, [t, weight])
                });
                let flag = |id| session.tool_options.flag("tool.stroke_width", id).unwrap_or(false);
                self.drag = Some(WidthDrag {
                    id, path, at: profile[index][0], start: at, width, weight: profile[index][1],
                    profile, index, lock_width: flag("stroke_width.lock_width"),
                    lock_order: flag("stroke_width.lock_order"), snap_ends: flag("stroke_width.snap_ends"),
                });
                if session.selection.ids.as_slice() != [id]
                    && let Some(command) = click_select_command(Some(id), &session.selection.ids, false)
                {
                    if session.run(&command).is_err() {
                        self.cancel();
                        return None;
                    }
                }
            }
        }
        if let Some(up) = up {
            let drag = self.drag.take()?;
            let pointer = point(up);
            if (pointer - drag.start).length() < radius * 0.25 {
                self.selected = Some((drag.id, drag.at));
                return Some(patch(drag.id, None, drag.profile));
            }
            let (width, profile, at) = drag.edit(pointer, radius);
            self.selected = Some((drag.id, at));
            return Some(patch(drag.id, Some(width), profile));
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            self.drag = None;
        }
        None
    }

    pub(super) fn paint(&self, ui: &Ui, rect: Rect, session: &Session, stroke: Stroke) {
        let [id] = session.selection.ids.as_slice() else { return };
        let Some(object) = session.document.get_object(*id).filter(|object| eligible(object)) else { return };
        let view = &session.view;
        let radius = 7.0 / view.zoom.max(1e-4);
        let display = |point: Vec2| screen(rect, view, [point.x, point.y]);
        let current;
        let (path, width, profile, active) = if let Some(drag) = &self.drag {
            let pointer = ui.input(|input| input.pointer.latest_pos()).map(|p| {
                let [x, y] = dtool_core::view_to_doc(view, [p.x - rect.min.x, p.y - rect.min.y]);
                egui::vec2(x, y)
            }).unwrap_or(drag.start);
            current = drag.edit(pointer, radius);
            (&drag.path, current.0, current.1.as_slice(), Some(current.2))
        } else {
            let Some(path) = Path::of(object) else { return };
            let painter = ui.painter_at(rect);
            let active = self.selected.filter(|(target, _)| target == id).map(|(_, at)| at);
            paint_profile(&painter, &path, object.stroke.width, &object.stroke.pressure, active, radius, stroke, display);
            return;
        };
        paint_profile(&ui.painter_at(rect), path, width, profile, active, radius, stroke, display);
    }
}

fn paint_profile(
    painter: &egui::Painter, path: &Path, width: f32, profile: &[[f32; 2]],
    active: Option<f32>, radius: f32, stroke: Stroke, display: impl Fn(Vec2) -> Pos2,
) {
    painter.add(egui::Shape::line(path.positions.iter().copied().map(&display).collect(), Stroke::new(1.0, stroke.color.gamma_multiply(0.65))));
    for &[at, weight] in profile {
        let sample = path.sample(at);
        let delta = sample.normal() * (width * weight * 0.5).max(radius * 0.7);
        let ends = [display(sample.pos - delta), display(sample.pos + delta)];
        painter.line_segment(ends, stroke);
        for end in ends {
            let fill = if active == Some(at) { stroke.color } else { egui::Color32::WHITE };
            painter.circle(end, 3.0, fill, stroke);
        }
        painter.circle_filled(display(sample.pos), 2.4, stroke.color);
    }
}
