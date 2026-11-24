use super::{click_select_command, preview_outlines, screen, Mods, MIN_DRAG_DOC};
use crate::input::primary_button_positions;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::scene::Scene;
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

#[derive(Clone, Debug, Default)]
pub(super) struct Crop {
    drag: Option<Resize>,
}

#[derive(Clone, Debug)]
struct Resize {
    id: ObjectId,
    grip: [i8; 2],
    pointer: [f32; 2],
    original: [f32; 4],
    current: [f32; 4],
    offset: [f32; 2],
}

impl Crop {
    pub(super) fn active(&self) -> bool {
        self.drag.is_some()
    }

    pub(super) fn cancel(&mut self) {
        self.drag = None;
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
        let (pressed, released) = primary_button_positions(ui);
        if response.contains_pointer() || response.is_pointer_button_down_on() {
            if let Some((point, (id, rect))) = pressed
                .filter(|p| response.interact_rect.contains(*p))
                .map(&to_doc)
                .zip(selected)
            {
                let grip = hit_grip(rect, point, session.view.zoom)
                    .or_else(|| (hit(point) == Some(id)).then_some([0, 0]));
                if let Some(grip) = grip {
                    self.drag = Some(Resize {
                        id,
                        grip,
                        pointer: point,
                        original: rect,
                        current: rect,
                        offset: [0.0; 2],
                    });
                }
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
                    crate::canvas_tools::snap_point(session, point, &[drag.id]).0
                };
                if drag.grip == [0, 0] {
                    let mut offset = [snapped[0] - drag.pointer[0], snapped[1] - drag.pointer[1]];
                    if mods.shift {
                        let minor_axis = usize::from(offset[0].abs() >= offset[1].abs());
                        offset[minor_axis] = 0.0;
                    }
                    drag.offset = offset;
                } else {
                    drag.current = resize(drag, snapped);
                }
            } else {
                drag.offset = [0.0; 2];
                drag.current = drag.original;
            }
            ui.ctx().set_cursor_icon(cursor(drag.grip));
        } else if response.hovered() {
            if let Some((rect, point)) = selected.map(|(_, r)| r).zip(pointer) {
                if let Some(grip) = hit_grip(rect, point, session.view.zoom) {
                    ui.ctx().set_cursor_icon(cursor(grip));
                }
            }
        }
        if released.is_some() {
            if let Some(drag) = self.drag.take() {
                let [x, y, right, bottom] = drag.current;
                if drag.offset != [0.0; 2] {
                    let object = session.document.get_object(drag.id)?;
                    let [dx, dy] = drag.offset;
                    let pos = [object.pos[0] + dx, object.pos[1] + dy];
                    return Some(Command::with_params(
                        "edit.batch",
                        json!({"commands":[
                            {"name":"object.update","params":{"id":drag.id,"pos":pos}},
                            {"name":"crop.vector.set","params":{"id":drag.id,"rect":[x - pos[0], y - pos[1], right - x, bottom - y]}}
                        ]}),
                    ));
                }
                if drag.original != drag.current {
                    return crate::canvas_tools::crop_command(
                        session,
                        drag.id,
                        [x, y],
                        [right, bottom],
                    );
                }
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
        let Some(bounds) = self
            .drag
            .as_ref()
            .map(|drag| drag.current)
            .or_else(|| bounds(session, scene).map(|(_, rect)| rect))
        else {
            return;
        };
        let [x, y, right, bottom] = bounds;
        let painter = ui.painter_at(canvas);
        let to_screen = |point| screen(canvas, &session.view, point);
        let crop = Rect::from_two_pos(to_screen([x, y]), to_screen([right, bottom]));
        if let Some(drag) = self.drag.as_ref().filter(|drag| drag.offset != [0.0; 2]) {
            let preview = painter.with_clip_rect(crop.intersect(canvas));
            let [dx, dy] = drag.offset;
            for (points, closed) in preview_outlines(scene, drag.id) {
                let points = points
                    .into_iter()
                    .map(|[x, y]| to_screen([x + dx, y + dy]))
                    .collect();
                preview.add(if closed {
                    egui::Shape::closed_line(points, stroke)
                } else {
                    egui::Shape::line(points, stroke)
                });
            }
        }
        painter.rect_stroke(crop, 0.0, stroke, egui::StrokeKind::Middle);
        for [a, b] in GRIPS {
            let at = to_screen(grip_at(bounds, [a, b]));
            let dx = if a == 0 {
                egui::vec2(5.0, 0.0)
            } else {
                egui::vec2(-(a as f32) * 9.0, 0.0)
            };
            let dy = if b == 0 {
                egui::vec2(0.0, 5.0)
            } else {
                egui::vec2(0.0, -(b as f32) * 9.0)
            };
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

fn bounds(session: &Session, scene: &Scene) -> Option<(ObjectId, [f32; 4])> {
    let [id] = session.selection.ids.as_slice() else {
        return None;
    };
    let object = session.document.get_object(*id)?;
    if object.locked
        || matches!(object.kind, ObjectKind::Group | ObjectKind::Artboard)
        || scene.quad(*id).is_none()
    {
        return None;
    }
    let [x, y, w, h] = match object.vector_crop {
        Some(crop) => [
            object.pos[0] + crop.rect[0],
            object.pos[1] + crop.rect[1],
            crop.rect[2],
            crop.rect[3],
        ],
        None => dtool_core::scene_hit::selection_box(scene, &session.document, &[*id])?,
    };
    let rect = [x, y, x + w, y + h];
    (rect.iter().all(|v| v.is_finite()) && w >= MIN_DRAG_DOC && h >= MIN_DRAG_DOC)
        .then_some((*id, rect))
}

fn grip_at([x, y, right, bottom]: [f32; 4], [a, b]: [i8; 2]) -> [f32; 2] {
    [
        x + (right - x) * (f32::from(a) + 1.0) / 2.0,
        y + (bottom - y) * (f32::from(b) + 1.0) / 2.0,
    ]
}

fn hit_grip(rect: [f32; 4], point: [f32; 2], zoom: f32) -> Option<[i8; 2]> {
    GRIPS
        .into_iter()
        .filter_map(|grip| {
            let at = grip_at(rect, grip);
            let distance = (at[0] - point[0]).hypot(at[1] - point[1]) * zoom;
            (distance <= GRIP_RADIUS).then_some((distance, grip))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, grip)| grip)
}

fn resize(drag: &Resize, point: [f32; 2]) -> [f32; 4] {
    let mut rect = drag.original;
    for (axis, direction) in drag.grip.into_iter().enumerate() {
        let delta = point[axis] - drag.pointer[axis];
        match direction {
            -1 => rect[axis] = (rect[axis] + delta).min(rect[axis + 2] - MIN_DRAG_DOC),
            1 => rect[axis + 2] = (rect[axis + 2] + delta).max(rect[axis] + MIN_DRAG_DOC),
            _ => {}
        }
    }
    rect
}

fn cursor([a, b]: [i8; 2]) -> CursorIcon {
    match (a, b) {
        (0, 0) => CursorIcon::Move,
        (0, _) => CursorIcon::ResizeVertical,
        (_, 0) => CursorIcon::ResizeHorizontal,
        _ if a == b => CursorIcon::ResizeNwSe,
        _ => CursorIcon::ResizeNeSw,
    }
}
