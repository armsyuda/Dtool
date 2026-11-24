use dtool_core::commands::warp::{MESH_EDIT, STORE_KEY, group_to_json, warp_group};
use dtool_core::warp::{HANDLE_DOWN, HANDLE_LEFT, HANDLE_RIGHT, HANDLE_UP, MeshPoint, WarpGroup, WarpMesh, edge_control};
use dtool_core::{ObjectId, Session};
use dtool_protocol::Command;
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui};
use serde_json::{Value, json};

use super::{ToolKind, screen, tool_kind};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MeshPreview {
    group: ObjectId,
    points: Vec<MeshPoint>,
}

impl MeshPreview {
    pub(crate) fn install(&self, session: &mut Session) -> Option<Value> {
        let (mut group, bounds) = warp_group(session, self.group)?;
        self.apply(&mut group.mesh);
        let slot = session.document.export_defaults.get_mut(STORE_KEY)?.get_mut(self.group.0.to_string())?;
        Some(std::mem::replace(slot, group_to_json(&group, bounds)))
    }

    pub(crate) fn restore(&self, session: &mut Session, original: Value) {
        session.document.export_defaults[STORE_KEY][self.group.0.to_string()] = original;
    }

    fn command(self) -> Command {
        let ops: Vec<_> = self.points.into_iter().map(|point| {
            json!({"point": point.id, "pos": point.pos, "handles": point.handles})
        }).collect();
        Command::with_params(MESH_EDIT, json!({
            "group": self.group.0,
            "ops": ops,
        }))
    }

    fn apply(&self, mesh: &mut WarpMesh) {
        for point in &self.points {
            if let Some(target) = mesh.points.iter_mut().find(|target| target.id == point.id) {
                *target = point.clone();
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Drag {
    group: ObjectId,
    points: Vec<MeshPoint>,
    handle: Option<(usize, [f64; 2])>,
    press: [f32; 2],
    generation: u64,
    version: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WarpMeshEditor {
    pub enabled: bool,
    pub multiple: bool,
    selected_group: Option<ObjectId>,
    selected: Vec<u64>,
    drag: Option<Drag>,
    preview: Option<MeshPreview>,
}

impl WarpMeshEditor {
    fn group(&self, session: &Session) -> Option<WarpGroup> {
        if !self.enabled || session.view.canvas_flags.preview_mode
            || !matches!(tool_kind(&session.active_tool), ToolKind::Move | ToolKind::Node)
        {
            return None;
        }
        let [id] = session.selection.ids.as_slice() else { return None };
        if session.document.get_object(*id)?.locked { return None; }
        warp_group(session, *id).map(|(group, _)| group)
    }

    pub(crate) fn visible(&self, session: &Session) -> bool {
        self.group(session).is_some()
    }

    pub(crate) fn active(&self) -> bool {
        self.drag.is_some()
    }

    pub(crate) fn cancel(&mut self) {
        self.drag = None;
        self.preview = None;
    }

    pub(crate) fn junction_command(&self, group: &WarpGroup) -> Option<Command> {
        if self.selected_group != Some(group.id) || self.active() {
            return None;
        }
        let [a, b] = self.selected.as_slice() else { return None };
        let index = group.mesh.points.iter().position(|point| point.id == *a)?;
        [HANDLE_LEFT, HANDLE_RIGHT, HANDLE_UP, HANDLE_DOWN].into_iter()
            .filter_map(|handle| neighbour(&group.mesh, index, handle))
            .any(|point| point.id == *b)
            .then(|| Command::with_params(MESH_EDIT, json!({
                "group": group.id.0, "ops": [{"add_junction": {"a": a, "b": b}}],
            })))
    }

    pub(crate) fn preview(&self, session: &Session) -> Option<&MeshPreview> {
        let drag = self.drag.as_ref()?;
        let current = self.enabled && session.selection.ids.as_slice() == [drag.group]
            && drag.generation == session.generation && drag.version == session.document.version
            && !session.view.canvas_flags.preview_mode
            && matches!(tool_kind(&session.active_tool), ToolKind::Move | ToolKind::Node);
        current.then_some(self.preview.as_ref()).flatten()
    }

    pub(crate) fn input(&mut self, ui: &Ui, response: &Response, session: &Session, escape: bool) -> (bool, Option<Command>) {
        let Some(group) = self.group(session) else {
            let changed = self.active();
            self.cancel();
            self.selected_group = None;
            self.selected.clear();
            if changed { ui.ctx().request_repaint(); }
            return (false, None);
        };
        if self.selected_group != Some(group.id) {
            self.selected_group = Some(group.id);
            self.selected.clear();
        }
        self.selected.retain(|id| group.mesh.points.iter().any(|point| point.id == *id));
        if self.drag.as_ref().is_some_and(|drag| {
            drag.group != group.id || drag.generation != session.generation || drag.version != session.document.version
        }) {
            self.cancel();
            ui.ctx().request_repaint();
        }
        if escape && (self.active() || !self.selected.is_empty()) {
            if self.active() { self.cancel(); } else { self.selected.clear(); }
            ui.ctx().request_repaint();
            return (true, None);
        }
        let rect = response.rect;
        let to_doc = |pos: Pos2| dtool_core::view_to_doc(&session.view, [pos.x - rect.min.x, pos.y - rect.min.y]);
        let to_screen = |point: [f64; 2]| screen(rect, &session.view, point.map(|value| value as f32));
        let (press, release) = crate::input::primary_button_positions(ui);
        if self.drag.is_none() {
            let pointer = press.or_else(|| response.hover_pos());
            let hit = pointer.filter(|pos| rect.contains(*pos)).and_then(|pos| self.hit(&group, pos, to_screen));
            if let Some((point, handle)) = hit {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                if let Some(press) = press {
                    response.request_focus();
                    if self.multiple || ui.input(|input| input.modifiers.shift) {
                        if handle.is_none() {
                            if let Some(index) = self.selected.iter().position(|id| *id == point.id) {
                                self.selected.remove(index);
                            } else {
                                self.selected.push(point.id);
                            }
                        }
                        ui.ctx().request_repaint();
                        return (true, None);
                    }
                    if !self.selected.contains(&point.id) {
                        self.selected = vec![point.id];
                    }
                    let points = group.mesh.points.iter().filter(|point| self.selected.contains(&point.id)).cloned().collect();
                    self.drag = Some(Drag { group: group.id, points, handle, press: to_doc(press),
                        generation: session.generation, version: session.document.version });
                }
            }
        }
        let Some(drag) = &self.drag else { return (false, None) };
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let pointer = release.or_else(|| ui.input(|input| input.pointer.latest_pos()));
        if let Some(pointer) = pointer {
            let at = to_doc(pointer);
            let delta = [f64::from(at[0] - drag.press[0]), f64::from(at[1] - drag.press[1])];
            let move_by = |point: [f64; 2]| [point[0] + delta[0], point[1] + delta[1]];
            let moved = delta.iter().all(|value| value.is_finite())
                && delta[0].hypot(delta[1]) * f64::from(session.view.zoom) >= 0.5;
            self.preview = moved.then(|| {
                let mut points = drag.points.clone();
                for point in &mut points {
                    if let Some((handle, start)) = drag.handle {
                        point.handles[handle] = move_by(start);
                    } else {
                        point.pos = move_by(point.pos);
                        point.handles = point.handles.map(move_by);
                    }
                }
                MeshPreview { group: drag.group, points }
            });
        }
        let command = if release.is_some() {
            self.drag = None;
            self.preview.take().map(MeshPreview::command)
        } else {
            if !ui.input(|input| input.pointer.primary_down()) { self.cancel(); }
            None
        };
        ui.ctx().request_repaint();
        (true, command)
    }

    fn hit<'a>(&self, group: &'a WarpGroup, pos: Pos2, to_screen: impl Fn([f64; 2]) -> Pos2) -> Option<(&'a MeshPoint, Option<(usize, [f64; 2])>)> {
        const HIT_RADIUS: f32 = 7.0;
        let points = &group.mesh.points;
        if let Some(point) = points.iter().filter(|point| to_screen(point.pos).distance(pos) <= HIT_RADIUS)
            .min_by(|a, b| to_screen(a.pos).distance_sq(pos).total_cmp(&to_screen(b.pos).distance_sq(pos)))
        {
            return Some((point, None));
        }
        let index = self.single_point_index(group)?;
        let selected = &points[index];
        handles(&group.mesh, index).filter(|(_, at)| to_screen(*at).distance(pos) <= HIT_RADIUS)
            .min_by(|(_, a), (_, b)| to_screen(*a).distance_sq(pos).total_cmp(&to_screen(*b).distance_sq(pos)))
            .map(|handle| (selected, Some(handle)))
    }

    fn single_point_index(&self, group: &WarpGroup) -> Option<usize> {
        let [id] = self.selected.as_slice() else { return None };
        if self.selected_group != Some(group.id) { return None; }
        group.mesh.points.iter().position(|point| point.id == *id)
    }

    pub(crate) fn paint(&self, ui: &Ui, rect: Rect, session: &Session) {
        let Some(mut group) = self.group(session) else { return };
        if let Some(preview) = self.preview(session) {
            preview.apply(&mut group.mesh);
        }
        let painter = ui.painter_at(rect);
        let accent = dtool_design::color("accent");
        let grid = Stroke::new(1.0, accent.gamma_multiply(0.85));
        let at = |point: [f64; 2]| screen(rect, &session.view, point.map(|value| value as f32));
        for (index, point) in group.mesh.points.iter().enumerate() {
            for (handle, opposite) in [(HANDLE_RIGHT, HANDLE_LEFT), (HANDLE_DOWN, HANDLE_UP)] {
                let Some(next) = neighbour(&group.mesh, index, handle) else { continue };
                let control = [point.pos, edge_control(point, handle, next), edge_control(next, opposite, point), next.pos];
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(control.map(at), false, Color32::TRANSPARENT, grid));
            }
        }
        if let Some(index) = self.single_point_index(&group) {
            let anchor = at(group.mesh.points[index].pos);
            for (_, control) in handles(&group.mesh, index) {
                let control = at(control);
                painter.line_segment([anchor, control], grid);
                painter.circle(control, 3.5, Color32::WHITE, grid);
            }
        }
        for point in &group.mesh.points {
            let selected = self.selected_group == Some(group.id) && self.selected.contains(&point.id);
            let marker = Rect::from_center_size(at(point.pos), egui::Vec2::splat(if selected { 8.0 } else { 6.0 }));
            painter.rect(marker, 0.0, if selected { accent } else { Color32::WHITE }, grid, egui::StrokeKind::Inside);
        }
    }
}

fn neighbour(mesh: &WarpMesh, index: usize, handle: usize) -> Option<&MeshPoint> {
    let cols = mesh.cols as usize;
    if cols == 0 { return None; }
    let col = index % cols;
    let other = match handle {
        HANDLE_LEFT if col > 0 => index - 1,
        HANDLE_RIGHT if col + 1 < cols => index + 1,
        HANDLE_UP => index.checked_sub(cols)?,
        HANDLE_DOWN => index.checked_add(cols)?,
        _ => return None,
    };
    (other / cols < mesh.rows as usize).then(|| mesh.points.get(other)).flatten()
}

fn handles(mesh: &WarpMesh, index: usize) -> impl Iterator<Item = (usize, [f64; 2])> + '_ {
    [HANDLE_LEFT, HANDLE_RIGHT, HANDLE_UP, HANDLE_DOWN].into_iter().filter_map(move |handle| {
        let other = neighbour(mesh, index, handle)?;
        Some((handle, edge_control(&mesh.points[index], handle, other)))
    })
}
