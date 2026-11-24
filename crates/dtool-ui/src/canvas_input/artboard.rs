use super::{CanvasInput, Mods, click_select_command, screen};
use dtool_core::artboard::edit::selected_artboard;
use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix, object_world_quad};
use dtool_core::{Command, Object, ObjectId, Session};
use egui::{Pos2, Rect, Response, Ui};
use serde_json::json;

const GRIPS: [[f32; 2]; 8] = [
    [0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 0.5],
    [1.0, 1.0], [0.5, 1.0], [0.0, 1.0], [0.0, 0.5],
];

#[derive(Debug, Clone, Copy)]
pub(super) struct ArtboardDrag {
    board: ObjectId,
    start: Pos2,
    current: Pos2,
    grip: Option<[f32; 2]>,
}

impl CanvasInput {
    /// Returns true while an insertion button owns the pointer.
    pub(super) fn artboard_neighbours(&mut self, ui: &mut Ui, response: &Response, session: &mut Session) -> bool {
        if self.busy() || !response.enabled() || session.view.canvas_flags.preview_mode { return false; }
        let board = selected_artboard(session).or_else(|| {
            if !session.selection.ids.is_empty() { return None; }
            let mut boards = dtool_core::view::ViewTarget::artboards(&session.document);
            let first = boards.next()?;
            boards.next().is_none().then_some(first)
        });
        let Some(board) = board else { return false; };
        let quad = object_world_quad(board).map(|p| screen(response.rect, &session.view, p));
        let bounds = Rect::from_points(&quad);
        let buttons = [
            ("left", egui::pos2(bounds.left() - 19.0, bounds.center().y), "artboard.add_left"),
            ("right", egui::pos2(bounds.right() + 19.0, bounds.center().y), "artboard.add_right"),
            ("top", egui::pos2(bounds.center().x, bounds.top() - 33.0), "artboard.add_top"),
            ("bottom", egui::pos2(bounds.center().x, bounds.bottom() + 19.0), "artboard.add_bottom"),
        ];
        let id = board.id.0;
        let (down, up) = crate::input::primary_button_positions(ui);
        let mut owns = false;
        for (direction, pos, key) in buttons {
            let rect = Rect::from_center_size(pos, egui::vec2(20.0, 20.0));
            if !response.interact_rect.contains_rect(rect) { continue; }
            let label = dtool_strings::text(key);
            let button = ui.put(rect, egui::Button::new("+")).on_hover_text(label);
            button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            let pointer = button.contains_pointer() || down.or(up).is_some_and(|p| rect.contains(p));
            owns |= pointer || button.is_pointer_button_down_on();
            if button.clicked() {
                self.run(session, Some(Command::with_params("artboard.create", json!({
                    "source": "adjacent", "id": id, "direction": direction
                }))));
                return true;
            }
        }
        owns
    }

    pub(super) fn artboard_gesture(
        &mut self, ui: &Ui, response: &Response, session: &mut Session,
        pointer: Option<Pos2>, primary: bool, mods: Mods,
    ) -> bool {
        let (down, up) = crate::input::primary_button_positions(ui);
        let rect = response.rect;
        if self.artboard_drag.is_none() {
            if self.busy() || !response.enabled() || egui::Popup::is_any_open(ui.ctx()) {
                return false;
            }
            let Some(at) = down.or(pointer).filter(|point| response.interact_rect.contains(*point)
                && ui.ctx().layer_id_at(*point) == Some(response.layer_id)) else { return false };
            let Some((board, grip)) = hit(ui, rect, session, at) else { return false };
            ui.ctx().set_cursor_icon(cursor(grip));
            if down.is_none() {
                return false;
            }
            self.artboard_drag = Some(ArtboardDrag { board, start: at, current: at, grip });
            response.request_focus();
            self.claim(session);
        }

        let Some(drag) = self.artboard_drag.as_mut() else { return false };
        if let Some(at) = up.or(pointer) {
            drag.current = at;
        }
        ui.ctx().set_cursor_icon(cursor(drag.grip));
        if up.is_some() || !primary {
            let drag = self.artboard_drag.take().unwrap();
            let distance = drag.start.distance(drag.current);
            let command = if up.is_none() {
                None
            } else if distance < ui.ctx().options(|options| options.input_options.max_click_dist) {
                click_select_command(Some(drag.board), &session.selection.ids, false)
            } else {
                draft(session, rect, drag, mods.shift || crate::artboard_bar::aspect_locked(session)).map(|board| Command::with_params("artboard.update", json!({
                    "id": drag.board.0, "resize": drag.grip.is_some(),
                    "rect": [board.pos[0], board.pos[1], board.size[0], board.size[1]],
                })))
            };
            self.run(session, command);
            self.owner = None;
        }
        ui.ctx().request_repaint();
        true
    }

    pub(super) fn paint_artboard_drag(&self, ui: &Ui, rect: Rect, session: &Session, stroke: egui::Stroke) {
        let Some(drag) = self.artboard_drag else { return };
        let preserve = ui.input(|input| input.modifiers.shift) || crate::artboard_bar::aspect_locked(session);
        let Some(board) = draft(session, rect, drag, preserve) else { return };
        let points = object_world_quad(&board).map(|point| screen(rect, &session.view, point));
        ui.painter_at(rect).add(egui::Shape::closed_line(points.to_vec(), stroke));
    }

    pub(crate) fn dragging_artboard(&self) -> bool {
        self.artboard_drag.is_some()
    }
}

fn grip_point(board: &Object, [u, v]: [f32; 2]) -> [f32; 2] {
    [board.pos[0] + u * board.size[0].abs(), board.pos[1] + v * board.size[1].abs()]
}

fn hit(ui: &Ui, rect: Rect, session: &Session, at: Pos2) -> Option<(ObjectId, Option<[f32; 2]>)> {
    if session.view.canvas_flags.preview_mode {
        return None;
    }
    if let Some(board) = selected_artboard(session).filter(|board| !board.locked) {
        let matrix = object_matrix(board);
        let grip = GRIPS.into_iter().map(|grip| {
            let point = apply_matrix(&matrix, grip_point(board, grip));
            (screen(rect, &session.view, point).distance(at), grip)
        }).filter(|(distance, _)| *distance <= super::HANDLE_PX)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, grip)) = grip {
            return Some((board.id, Some(grip)));
        }
    }
    dtool_core::view::ViewTarget::artboards(&session.document)
        .filter(|board| !board.locked && board.pickable)
        .filter(|board| {
            let at_corner = screen(rect, &session.view, object_world_quad(board)[0]);
            if at.x < at_corner.x || at.y > at_corner.y - 3.0 || at.y < at_corner.y - 19.0 {
                return false;
            }
            if session.document.layout_artboard_origins.is_some()
                && session.document.pages.iter().any(|page| page.artboard_id == Some(board.id))
            {
                return false;
            }
            let width = ui.painter().layout_no_wrap(board.name.clone(), egui::FontId::proportional(11.0),
                ui.visuals().text_color()).size().x;
            at.x <= at_corner.x + width
        }).last().map(|board| (board.id, None))
}

fn cursor(grip: Option<[f32; 2]>) -> egui::CursorIcon {
    match grip {
        Some([0.5, _]) => egui::CursorIcon::ResizeVertical,
        Some([_, 0.5]) => egui::CursorIcon::ResizeHorizontal,
        Some([u, v]) if u == v => egui::CursorIcon::ResizeNwSe,
        Some(_) => egui::CursorIcon::ResizeNeSw,
        None => egui::CursorIcon::Move,
    }
}

fn draft(session: &Session, canvas: Rect, drag: ArtboardDrag, preserve: bool) -> Option<Object> {
    let mut board = session.document.get_object(drag.board)?.clone();
    if board.kind != dtool_core::ObjectKind::Artboard {
        return None;
    }
    let to_doc = |point: Pos2| dtool_core::view_to_doc(&session.view, [point.x - canvas.min.x, point.y - canvas.min.y]);
    let start = to_doc(drag.start);
    let end = to_doc(drag.current);
    let delta = [end[0] - start[0], end[1] - start[1]];
    let Some(grip) = drag.grip else {
        board.pos = [board.pos[0] + delta[0], board.pos[1] + delta[1]];
        return Some(board);
    };
    let matrix = object_matrix(&board);
    let inverse = invert_matrix(&matrix)?;
    let start_local = apply_matrix(&inverse, start);
    let end_local = apply_matrix(&inverse, end);
    let old_size = board.size.map(f32::abs);
    let minimum = (2.0 / session.view.zoom.max(0.0001)).min(old_size[0].min(old_size[1]));
    let candidate = std::array::from_fn::<_, 2, _>(|i| {
        (old_size[i] + (grip[i] * 2.0 - 1.0) * (end_local[i] - start_local[i])).max(minimum)
    });
    let size = if preserve {
        let scale = if grip[0] == 0.5 { candidate[1] / old_size[1] }
            else if grip[1] == 0.5 { candidate[0] / old_size[0] }
            else {
                let [sx, sy] = std::array::from_fn(|i| candidate[i] / old_size[i]);
                if (sx - 1.0).abs() >= (sy - 1.0).abs() { sx } else { sy }
            };
        old_size.map(|side| side * scale.max(minimum / old_size[0].min(old_size[1])))
    } else {
        candidate
    };
    let new_center = std::array::from_fn(|i| {
        board.pos[i] + old_size[i] * 0.5 + (grip[i] - 0.5) * (size[i] - old_size[i])
    });
    let world_center = apply_matrix(&matrix, new_center);
    board.pos = std::array::from_fn(|i| world_center[i] - size[i] * 0.5);
    board.size = size;
    Some(board)
}
