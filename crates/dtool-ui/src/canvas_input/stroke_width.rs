use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::Paint;
use dtool_core::{Session, scene::Scene};
use crate::app_canvas::{clear_geometry_preview, stage_geometry_preview};
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
    snap_nodes: bool,
    snap_weights: bool,
    new_point: bool,
}

#[derive(Debug, Clone)]
struct Path {
    positions: Vec<Vec2>,
    local: Vec<Vec2>,
    matrix: [f32; 6],
    nodes: Vec<f32>,
    distances: Vec<f32>,
    length: f32,
}

#[derive(Clone, Copy)]
struct Sample {
    pos: Vec2,
    tangent: Vec2,
    width_axis: Vec2,
    at: f32,
}

impl Sample {
    fn normal(self) -> Vec2 {
        egui::vec2(-self.tangent.y, self.tangent.x)
    }

    fn handle_offset(self, half_width: f32, radius: f32) -> Vec2 {
        self.width_axis.normalized() * (self.width_axis.length() * half_width).max(radius * 0.7)
    }

    fn normal_offset(self, point: Vec2) -> f32 {
        (point - self.pos).dot(self.normal()) / self.width_axis.dot(self.normal())
    }
}

impl Path {
    fn of(object: &Object) -> Vec<Self> {
        let Some(outline) = dtool_core::outline::object_outline(object) else { return Vec::new() };
        let matrix = dtool_core::geometry::object_matrix(object);
        if dtool_core::geometry::invert_matrix(&matrix).is_none() { return Vec::new(); }
        let transform = |p: [f64; 2]| {
            let [x, y] = dtool_core::geometry::apply_matrix(&matrix, [
                p[0] as f32 + object.pos[0], p[1] as f32 + object.pos[1],
            ]);
            egui::vec2(x, y)
        };
        let point_key = |position: Vec2| [position.x.to_bits(), position.y.to_bits()];
        let node_positions: std::collections::HashSet<_> = outline.elements().iter()
            .filter_map(|element| element.end_point())
            .map(|p| point_key(egui::vec2(p.x as f32, p.y as f32))).collect();
        dtool_core::outline::flatten_subpaths(&outline, dtool_core::boolean::DEFAULT_TOLERANCE)
            .into_iter().filter_map(|(mut points, closed)| {
                points.dedup_by_key(|p| p.map(|v| v as f32));
                if closed && points.first() != points.last() {
                    points.push(*points.first()?);
                }
                let local: Vec<_> = points.iter().map(|p| egui::vec2(p[0] as f32, p[1] as f32)).collect();
                let positions = points.into_iter().map(transform).collect();
                let distances: Vec<_> = std::iter::once(0.0).chain(local.windows(2).scan(0.0, |length, pair| {
                    *length += (pair[1] - pair[0]).length();
                    Some(*length)
                })).collect();
                let length = *distances.last()?;
                if !length.is_finite() || length <= f32::EPSILON { return None; }
                let nodes = local.iter().zip(&distances).filter_map(|(position, distance)| {
                    node_positions.contains(&point_key(*position)).then_some(distance / length)
                }).collect();
                Some(Self { positions, local, matrix, nodes, distances, length })
            }).collect()
    }

    fn segment(&self, end: usize, fraction: f32) -> Sample {
        let start = self.positions[end - 1];
        let edge = self.positions[end] - start;
        let direction = (self.local[end] - self.local[end - 1]).normalized();
        let [a, b, c, d, _, _] = self.matrix;
        Sample {
            pos: start + edge * fraction,
            tangent: edge.normalized(),
            width_axis: egui::vec2(c * direction.x - a * direction.y, d * direction.x - b * direction.y),
            at: (self.distances[end - 1] + (self.distances[end] - self.distances[end - 1]) * fraction) / self.length,
        }
    }

    fn sample(&self, at: f32) -> Sample {
        let distance = at * self.length;
        let end = self.distances.partition_point(|d| *d <= distance).clamp(1, self.distances.len() - 1);
        self.segment(end, ((distance - self.distances[end - 1])
            / (self.distances[end] - self.distances[end - 1])).clamp(0.0, 1.0))
    }

    fn nearest(&self, point: Vec2) -> Sample {
        self.positions.windows(2).enumerate().filter_map(|(i, pair)| {
            let segment = pair[1] - pair[0];
            let length_sq = segment.length_sq();
            (length_sq > 0.0 && self.distances[i + 1] > self.distances[i]).then(|| {
                let fraction = ((point - pair[0]).dot(segment) / length_sq).clamp(0.0, 1.0);
                self.segment(i + 1, fraction)
            })
        }).min_by(|a, b| (a.pos - point).length_sq().total_cmp(&(b.pos - point).length_sq()))
            .unwrap_or_else(|| self.sample(0.0))
    }

    fn snap_node(&self, sample: Sample, radius: f32) -> f32 {
        self.nodes.iter().copied()
            .map(|at| (at, (self.sample(at).pos - sample.pos).length_sq()))
            .filter(|(_, distance)| *distance <= radius * radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(sample.at, |(at, _)| at)
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

fn eligible(session: &Session, object: &Object) -> bool {
    object.pickable && object.stroke.width > 0.0 && object.stroke.paint != Paint::None
        && !session.document.is_object_locked(object)
}

fn handle_at(path: &Path, profile: &[[f32; 2]], width: f32, point: Vec2, radius: f32) -> Option<(usize, f32)> {
    profile.iter().enumerate().filter_map(|(i, &[at, weight])| {
        let sample = path.sample(at);
        let offset = sample.handle_offset(width * weight * 0.5, radius);
        let distance = [sample.pos, sample.pos + offset, sample.pos - offset].iter()
            .map(|p| (*p - point).length_sq()).fold(f32::INFINITY, f32::min);
        (distance <= radius * radius).then_some((i, distance))
    }).min_by(|a, b| a.1.total_cmp(&b.1))
}

impl WidthDrag {
    fn edit(&self, pointer: Vec2, radius: f32, modifiers: egui::Modifiers) -> (f32, Vec<[f32; 2]>, f32) {
        let original = self.path.sample(self.at);
        let start_offset = original.normal_offset(self.start);
        let movement = pointer - self.start;
        let sign = if (self.start - original.pos).dot(original.normal()).abs() >= radius * 0.45 {
            start_offset.signum()
        } else {
            (movement.dot(original.normal()) / original.width_axis.dot(original.normal())).signum()
        };
        let mut at = self.at;
        if !modifiers.shift && movement.dot(original.tangent).abs() >= radius * 0.5 {
            let sample = self.path.nearest(pointer);
            at = if self.snap_nodes { self.path.snap_node(sample, radius) } else { sample.at };
        }
        if self.lock_order {
            let min = self.index.checked_sub(1).map_or(0.0, |i| (self.profile[i][0] + 0.0001).min(self.at));
            let max = self.profile.get(self.index + 1).map_or(1.0, |p| (p[0] - 0.0001).max(self.at));
            at = at.clamp(min, max);
        } else if at != self.at {
            while at != self.at && self.profile.iter().any(|p| p[0] == at) {
                at = if at > self.at { at.next_down() } else { at.next_up() };
            }
        }
        let sample = self.path.sample(at);
        let original_width = self.width * self.weight;
        let mut target = if modifiers.command {
            original_width
        } else {
            (original_width + (sample.normal_offset(pointer) - start_offset) * sign * 2.0).max(0.0)
        };
        if self.snap_weights && !modifiers.command {
            let threshold = radius * 0.7 / sample.width_axis.length();
            if let Some((width, _)) = self.profile.iter().enumerate()
                .filter(|(i, _)| *i != self.index)
                .map(|(_, point)| self.width * point[1])
                .map(|width| (width, (width - target).abs()))
                .filter(|(_, delta)| *delta <= threshold)
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                target = width;
            }
        }
        let width = if self.lock_width {
            target = target.min(self.width);
            self.width
        } else {
            self.width.max(target)
        };
        let mut profile = self.profile.clone();
        profile.remove(self.index);
        for entry in &mut profile {
            entry[1] *= self.width / width;
        }
        let index = if at == self.at { self.index } else { profile.partition_point(|p| p[0] < at) };
        profile.insert(index, [at, target / width]);
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

fn hit_path(object: &Object, point: Vec2, radius: f32) -> Option<(Path, Option<usize>, Sample)> {
    Path::of(object).into_iter().filter_map(|path| {
        let handle = handle_at(&path, &object.stroke.pressure, object.stroke.width, point, radius);
        let sample = path.nearest(point);
        let distance = (sample.pos - point).length_sq();
        (handle.is_some() || distance <= radius * radius)
            .then(|| ((handle.is_none(), handle.map_or(distance, |(_, d)| d)), (path, handle.map(|(i, _)| i), sample)))
    }).min_by(|(a, _), (b, _)| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))).map(|(_, hit)| hit)
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
        let modifiers = ui.input(|input| input.events.iter().rev().find_map(|event| match event {
            egui::Event::PointerButton { button: egui::PointerButton::Primary, modifiers, .. } if up.is_some() => Some(*modifiers),
            _ => None,
        }).unwrap_or(input.modifiers));
        let screen_pointer = ui.input(|input| input.pointer.latest_pos());
        let pointer = screen_pointer.map(point);
        let selected = match session.selection.ids.as_slice() {
            [id] => session.document.get_object(*id).filter(|object| eligible(session, object) && scene.quad(*id).is_some()),
            _ => None,
        };
        if let Some(at) = screen_pointer.filter(|at| response.hovered()
            || (self.drag.is_some() && response.rect.contains(*at)))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            let normal = self.drag.as_ref().map(|drag| drag.path.sample(drag.at).width_axis.normalized())
                .or_else(|| {
                    let object = selected?;
                    let (path, index, sample) = hit_path(object, point(at), radius)?;
                    Some(index.map_or(sample, |i| path.sample(object.stroke.pressure[i][0])).width_axis.normalized())
                })
                .map(|normal| egui::emath::Rot2::from_angle(session.view.rotation_deg.to_radians()) * normal)
                .unwrap_or(Vec2::Y);
            paint_width_cursor(ui, response.rect, at, normal);
        }
        if let Some(object) = selected
            && keyboard && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
            && let Some(index) = self.selected.filter(|(id, _)| *id == object.id)
                .and_then(|(_, at)| object.stroke.pressure.iter().position(|point| point[0] == at))
                .or_else(|| hit_path(object, pointer?, radius)?.1)
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
            let hit = |object: &Object| hit_path(object, at, radius);
            let candidate = selected.and_then(|object| hit(object).map(|info| (object, info))).or_else(|| {
                session.document.objects.iter().rev()
                    .filter(|object| eligible(session, object) && scene.quad(object.id).is_some())
                    .find_map(|object| hit(object).map(|info| (object, info)))
            });
            if let Some((object, (path, index, sample))) = candidate {
                let id = object.id;
                let width = object.stroke.width;
                let mut profile = object.stroke.pressure.clone();
                let flag = |id| session.tool_options.flag("tool.stroke_width", id).unwrap_or(false);
                let snap_nodes = flag("stroke_width.snap_ends");
                let t = if snap_nodes { path.snap_node(sample, radius) } else { sample.at };
                let index = index.or_else(|| profile.iter().position(|point| point[0] == t));
                let new_point = index.is_none();
                let index = index.unwrap_or_else(|| {
                    let weight = weight_at(&profile, t);
                    if profile.is_empty() {
                        profile.extend([[0.0, 1.0], [1.0, 1.0]]);
                    }
                    WidthDrag::insert(&mut profile, [t, weight])
                });
                self.drag = Some(WidthDrag {
                    id, path, at: profile[index][0], start: at, width, weight: profile[index][1],
                    profile, index, lock_width: flag("stroke_width.lock_width"),
                    lock_order: flag("stroke_width.lock_order"), snap_nodes,
                    snap_weights: flag("stroke_width.snap_weights"), new_point,
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
            clear_geometry_preview(ui);
            let pointer = point(up);
            if (pointer - drag.start).length() < radius * 0.25 {
                if !drag.new_point && response.double_clicked() {
                    self.selected = None;
                    let mut profile = drag.profile;
                    profile.remove(drag.index);
                    return Some(patch(drag.id, None, profile));
                }
                self.selected = Some((drag.id, drag.at));
                return drag.new_point.then(|| patch(drag.id, None, drag.profile));
            }
            let (width, profile, at) = drag.edit(pointer, radius, modifiers);
            self.selected = Some((drag.id, at));
            return (drag.new_point || width != drag.width || profile != drag.profile)
                .then(|| patch(drag.id, Some(width), profile));
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            if self.drag.take().is_some() { clear_geometry_preview(ui); }
        } else if let (Some(drag), Some(pointer)) = (&self.drag, pointer) {
            if (pointer - drag.start).length() >= radius * 0.25 {
                let (width, profile, _) = drag.edit(pointer, radius, modifiers);
                stage_geometry_preview(ui, session, patch(drag.id, Some(width), profile));
            } else {
                clear_geometry_preview(ui);
            }
        }
        None
    }

    pub(super) fn paint(&self, ui: &Ui, rect: Rect, session: &Session, stroke: Stroke) {
        let stroke = Stroke::new(stroke.width, super::SELECTION_BLUE);
        let [id] = session.selection.ids.as_slice() else { return };
        let Some(object) = session.document.get_object(*id).filter(|object| eligible(session, object)) else { return };
        let view = &session.view;
        let radius = 7.0 / view.zoom.max(1e-4);
        let display = |point: Vec2| screen(rect, view, [point.x, point.y]);
        let painter = ui.painter_at(rect);
        let (width, profile, active) = if let Some(drag) = self.drag.as_ref().filter(|drag| drag.id == *id) {
            (drag.width, drag.profile.as_slice(), Some(drag.at))
        } else {
            (object.stroke.width, object.stroke.pressure.as_slice(),
                self.selected.filter(|(target, _)| target == id).map(|(_, at)| at))
        };
        for path in Path::of(object) {
            paint_profile(&painter, &path, width, profile, active, radius, stroke, display);
        }
    }
}

fn paint_profile(
    painter: &egui::Painter, path: &Path, width: f32, profile: &[[f32; 2]],
    active: Option<f32>, radius: f32, stroke: Stroke, display: impl Fn(Vec2) -> Pos2,
) {
    painter.add(egui::Shape::line(path.positions.iter().copied().map(&display).collect(), Stroke::new(1.0, stroke.color.gamma_multiply(0.65))));
    for &[at, weight] in profile {
        let sample = path.sample(at);
        let delta = sample.handle_offset(width * weight * 0.5, radius);
        let ends = [display(sample.pos - delta), display(sample.pos + delta)];
        painter.line_segment(ends, stroke);
        for end in ends {
            let fill = if active == Some(at) { stroke.color } else { egui::Color32::WHITE };
            painter.circle(end, 3.0, fill, stroke);
        }
        painter.circle_filled(display(sample.pos), 2.4, stroke.color);
    }
}
