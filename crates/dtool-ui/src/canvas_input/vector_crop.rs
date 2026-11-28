use super::{click_select_command, preview_outlines, screen, Mods, MIN_DRAG_DOC};
use crate::input::primary_button_positions;
use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::scene::Scene;
use dtool_core::selection::{MarqueeMode, SelectOp};
use dtool_core::Session;
use dtool_protocol::Command;
use egui::{CursorIcon, Pos2, Rect, Response, Stroke, Ui};
use serde_json::json;

const GRIPS: [[i8; 2]; 8] = [
    [-1, -1],
    [0, -1],
    [1, -1],
    [-1, 0],
    [1, 0],
    [-1, 1],
    [0, 1],
    [1, 1],
];
const GRIP_RADIUS: f32 = 8.0;
const MARQUEE_THRESHOLD: f32 = 3.0;

#[derive(Clone, Debug, Default)]
pub(super) struct Crop {
    drag: Option<Resize>,
    marquee: Option<Marquee>,
}

#[derive(Clone, Debug)]
struct Marquee {
    start: [f32; 2],
    current: [f32; 2],
}

impl Marquee {
    fn rect(&self, zoom: f32) -> Option<[f32; 4]> {
        let [x, y] = self.start;
        let [end_x, end_y] = self.current;
        let width = (end_x - x).abs();
        let height = (end_y - y).abs();
        (width.hypot(height) * zoom >= MARQUEE_THRESHOLD)
            .then_some([x.min(end_x), y.min(end_y), width, height])
    }
}

#[derive(Clone, Debug)]
struct Resize {
    frame: Frame,
    grip: [i8; 2],
    pointer: [f32; 2],
    current: [f32; 4],
    offset: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    id: ObjectId,
    rect: [f32; 4],
    matrix: [f32; 6],
    inverse: [f32; 6],
}

impl Frame {
    fn to_world(self, point: [f32; 2]) -> [f32; 2] {
        apply_matrix(&self.matrix, point)
    }

    fn to_local(self, point: [f32; 2]) -> [f32; 2] {
        apply_matrix(&self.inverse, point)
    }

    fn local_offset(self, [dx, dy]: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d, _, _] = self.inverse;
        [a * dx + c * dy, b * dx + d * dy]
    }

    fn screen_direction(self, [x, y]: [f32; 2], view_rotation: f32) -> egui::Vec2 {
        let [a, b, c, d, _, _] = self.matrix;
        egui::emath::Rot2::from_angle(view_rotation.to_radians()) * egui::vec2(a * x + c * y, b * x + d * y)
    }
}

impl Crop {
    pub(super) fn active(&self) -> bool {
        self.drag.is_some() || self.marquee.is_some()
    }

    pub(super) fn cancel(&mut self) {
        *self = Self::default();
    }

    pub(super) fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        session: &Session,
        scene: &Scene,
        mods: Mods,
        to_doc: impl Fn(Pos2) -> [f32; 2],
        hit: impl Fn([f32; 2]) -> Option<ObjectId>,
    ) -> Option<Command> {
        let selected = bounds(session, scene);
        if self.drag.as_ref().is_some_and(|drag| {
            !selected.is_some_and(|frame| frame.id == drag.frame.id)
        }) {
            self.drag = None;
        }
        let (pressed, released) = primary_button_positions(ui);
        let zoom = session.view.zoom;
        let grip_at_point = |frame: Frame, point: [f32; 2]| {
            let [x, y] = frame.to_local(point);
            let [left, top, right, bottom] = frame.rect;
            hit_grip(frame, point, zoom).or_else(|| {
                (x >= left && y >= top && x <= right && y <= bottom && hit(point) == Some(frame.id))
                    .then_some([0, 0])
            })
        };
        if let Some(point) = pressed.filter(|p| response.enabled()
            && response.interact_rect.contains(*p)
            && ui.ctx().layer_id_at(*p) == Some(response.layer_id)
            && !egui::Popup::is_any_open(ui.ctx())).map(&to_doc)
        {
            if let Some(frame) = selected
                && let Some(grip) = grip_at_point(frame, point)
            {
                self.drag = Some(Resize {
                    frame,
                    grip,
                    pointer: point,
                    current: frame.rect,
                    offset: [0.0; 2],
                });
            } else if hit(point).is_none() {
                self.marquee = Some(Marquee { start: point, current: point });
            }
        }
        let pointer = released
            .or_else(|| ui.input(|input| input.pointer.latest_pos()))
            .map(&to_doc);
        if let (Some(drag), Some(point)) = (self.drag.as_mut(), pointer) {
            if (point[0] - drag.pointer[0]).hypot(point[1] - drag.pointer[1]) * session.view.zoom
                > 1.0
            {
                let snapped = if mods.alt {
                    point
                } else {
                    crate::canvas_tools::snap_point(session, point, &[drag.frame.id]).0
                };
                if drag.grip == [0, 0] {
                    let mut offset = [snapped[0] - drag.pointer[0], snapped[1] - drag.pointer[1]];
                    if mods.shift {
                        let minor_axis = usize::from(offset[0].abs() >= offset[1].abs());
                        offset[minor_axis] = 0.0;
                    }
                    drag.offset = offset;
                } else {
                    let offset = drag.frame.local_offset([snapped[0] - drag.pointer[0], snapped[1] - drag.pointer[1]]);
                    drag.current = resize(drag.frame.rect, drag.grip, offset);
                }
            } else {
                drag.offset = [0.0; 2];
                drag.current = drag.frame.rect;
            }
            ui.ctx().set_cursor_icon(cursor(drag.frame, drag.grip, session.view.rotation_deg));
        } else if let (Some(marquee), Some(point)) = (self.marquee.as_mut(), pointer) {
            marquee.current = point;
            if marquee.rect(zoom).is_some() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
        } else if response.hovered()
            && let Some((frame, point)) = selected.zip(pointer)
            && let Some(grip) = grip_at_point(frame, point)
        {
            ui.ctx().set_cursor_icon(cursor(frame, grip, session.view.rotation_deg));
        }
        if released.is_some() {
            if let Some(drag) = self.drag.take() {
                let id = drag.frame.id;
                let object = session.document.get_object(id)?;
                let [x, y, right, bottom] = drag.current;
                let [dx, dy] = drag.offset;
                let [local_dx, local_dy] = drag.frame.local_offset(drag.offset);
                let rect = [x - object.pos[0] - local_dx, y - object.pos[1] - local_dy, right - x, bottom - y];
                if drag.offset != [0.0; 2] {
                    let pos = [object.pos[0] + dx, object.pos[1] + dy];
                    return Some(Command::with_params(
                        "edit.batch",
                        json!({"commands":[
                            {"name":"object.update","params":{"id":id,"pos":pos}},
                            {"name":"crop.vector.set","params":{"id":id,"rect":rect}}
                        ]}),
                    ));
                }
                if drag.frame.rect != drag.current {
                    return Some(Command::with_params("crop.vector.set", json!({"id":id,"rect":rect})));
                }
            }
            if let Some(rect) = self.marquee.take().and_then(|marquee| marquee.rect(zoom)) {
                let mode = if mods.ctrl { MarqueeMode::Enclosed } else { MarqueeMode::Touch };
                let op = if mods.shift { SelectOp::Add } else { SelectOp::Replace };
                return Some(Command::with_params("selection.marquee", json!({ "rect": rect, "mode": mode, "op": op })));
            }
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            self.cancel();
        }
        if response.clicked_by(egui::PointerButton::Primary) {
            let id = pointer.and_then(hit);
            if id.is_some() && session.selection.ids.as_slice() == id.as_slice() && !mods.shift {
                return None;
            }
            return click_select_command(id, &session.selection.ids, mods.shift);
        }
        None
    }

    pub(super) fn paint(
        &self,
        ui: &Ui,
        canvas: Rect,
        session: &Session,
        scene: &Scene,
        stroke: Stroke,
    ) {
        let painter = ui.painter_at(canvas);
        let to_screen = |point| screen(canvas, &session.view, point);
        if let Some([x, y, width, height]) = self.marquee.as_ref().and_then(|marquee| marquee.rect(session.view.zoom)) {
            let polygon = [[x, y], [x + width, y], [x + width, y + height], [x, y + height]].map(to_screen);
            painter.add(egui::Shape::convex_polygon(polygon.to_vec(), stroke.color.gamma_multiply(0.15), stroke));
            return;
        }
        let Some((frame, bounds)) = self
            .drag
            .as_ref()
            .map(|drag| (drag.frame, drag.current))
            .or_else(|| bounds(session, scene).map(|frame| (frame, frame.rect)))
        else {
            return;
        };
        let to_screen = |local| to_screen(frame.to_world(local));
        let [x, y, right, bottom] = bounds;
        let crop = [[x, y], [right, y], [right, bottom], [x, bottom]].map(to_screen);
        if let Some(drag) = self.drag.as_ref().filter(|drag| drag.offset != [0.0; 2]) {
            let [dx, dy] = drag.offset;
            let extent = [right - x, bottom - y];
            for (points, closed) in preview_outlines(scene, frame.id) {
                let closing = points.last().copied().zip(points.first().copied()).filter(|_| closed);
                let edges = points.windows(2).map(|pair| (pair[0], pair[1])).chain(closing);
                for (a, b) in edges {
                    let a = frame.to_local([a[0] + dx, a[1] + dy]);
                    let b = frame.to_local([b[0] + dx, b[1] + dy]);
                    if let Some(interval) = crate::app_canvas::clip_frame_segment(a, b, [x, y], extent) {
                        let segment = interval.map(|t| to_screen([
                            a[0] + (b[0] - a[0]) * t,
                            a[1] + (b[1] - a[1]) * t,
                        ]));
                        painter.line_segment(segment, stroke);
                    }
                }
            }
        }
        painter.add(egui::Shape::closed_line(crop.to_vec(), stroke));
        let axis_x = frame.screen_direction([1.0, 0.0], session.view.rotation_deg).normalized();
        let axis_y = frame.screen_direction([0.0, 1.0], session.view.rotation_deg).normalized();
        for [a, b] in GRIPS {
            let at = to_screen(grip_at(bounds, [a, b]));
            let dx = axis_x * if a == 0 { 5.0 } else { -f32::from(a) * 9.0 };
            let dy = axis_y * if b == 0 { 5.0 } else { -f32::from(b) * 9.0 };
            let segments = [
                [if a == 0 { at - dx } else { at }, at + dx],
                [if b == 0 { at - dy } else { at }, at + dy],
            ];
            for line in segments {
                painter.line_segment(line, Stroke::new(5.0, ui.visuals().panel_fill));
                painter.line_segment(line, Stroke::new(2.8, stroke.color));
            }
        }
    }
}

fn bounds(session: &Session, scene: &Scene) -> Option<Frame> {
    let [id] = session.selection.ids.as_slice() else {
        return None;
    };
    let object = session.document.get_object(*id)?;
    if session.document.is_object_locked(object)
        || matches!(object.kind, ObjectKind::Group | ObjectKind::Artboard)
        || scene.quad(*id).is_none()
    {
        return None;
    }
    let matrix = object_matrix(object);
    let inverse = invert_matrix(&matrix)?;
    let rect = match object.vector_crop {
        Some(crop) => {
            let x = object.pos[0] + crop.rect[0];
            let y = object.pos[1] + crop.rect[1];
            [x, y, x + crop.rect[2], y + crop.rect[3]]
        }
        None => {
            let quad = dtool_core::scene_hit::selection_quad(scene, &session.document, *id)?;
            let local = quad.map(|point| apply_matrix(&inverse, point)).to_vec();
            dtool_core::scene::contours_bounds(&[local])?
        }
    };
    (rect.iter().all(|v| v.is_finite()) && rect[2] - rect[0] >= MIN_DRAG_DOC && rect[3] - rect[1] >= MIN_DRAG_DOC)
        .then_some(Frame { id: *id, rect, matrix, inverse })
}

fn grip_at([x, y, right, bottom]: [f32; 4], [a, b]: [i8; 2]) -> [f32; 2] {
    [
        x + (right - x) * (f32::from(a) + 1.0) / 2.0,
        y + (bottom - y) * (f32::from(b) + 1.0) / 2.0,
    ]
}

fn hit_grip(frame: Frame, point: [f32; 2], zoom: f32) -> Option<[i8; 2]> {
    GRIPS
        .into_iter()
        .filter_map(|grip| {
            let at = frame.to_world(grip_at(frame.rect, grip));
            let distance = (at[0] - point[0]).hypot(at[1] - point[1]) * zoom;
            (distance <= GRIP_RADIUS).then_some((distance, grip))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, grip)| grip)
}

fn resize(mut rect: [f32; 4], grip: [i8; 2], offset: [f32; 2]) -> [f32; 4] {
    for (axis, direction) in grip.into_iter().enumerate() {
        let delta = offset[axis];
        match direction {
            -1 => rect[axis] = (rect[axis] + delta).min(rect[axis + 2] - MIN_DRAG_DOC),
            1 => rect[axis + 2] = (rect[axis + 2] + delta).max(rect[axis] + MIN_DRAG_DOC),
            _ => {}
        }
    }
    rect
}

fn cursor(frame: Frame, [a, b]: [i8; 2], rotation_deg: f32) -> CursorIcon {
    if [a, b] == [0, 0] {
        return CursorIcon::Move;
    }
    let direction = frame.screen_direction([f32::from(a), f32::from(b)], rotation_deg);
    let angle = direction.y.atan2(direction.x).to_degrees();
    match ((angle.rem_euclid(180.0) + 22.5) / 45.0) as u8 % 4 {
        0 => CursorIcon::ResizeHorizontal,
        1 => CursorIcon::ResizeNwSe,
        2 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeNeSw,
    }
}
