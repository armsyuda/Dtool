use super::{CanvasInput, Mods, click_select_command, screen};
use dtool_core::artboard::edit::selected_artboard;
use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix, object_world_bounds, object_world_quad};
use dtool_core::scene::Scene;
use dtool_core::{Command, Object, ObjectId, Session};
use egui::{Pos2, Rect, Response, Ui};
use serde_json::json;

const GRIPS: [[f32; 2]; 8] = [
    [0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 0.5],
    [1.0, 1.0], [0.5, 1.0], [0.0, 1.0], [0.0, 0.5],
];

#[derive(Debug, Clone)]
pub(super) struct ArtboardDrag {
    board: ObjectId,
    start: Pos2,
    current: Pos2,
    grip: Option<[f32; 2]>,
    correction: [f32; 2],
    duplicate: bool,
    preview: Option<ArtboardPreview>,
}

#[derive(Clone)]
struct ArtboardPreview {
    bounds: [f32; 4],
    texture: egui::TextureHandle,
}

impl std::fmt::Debug for ArtboardPreview {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ArtboardPreview").field("bounds", &self.bounds).finish_non_exhaustive()
    }
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
        &mut self, ui: &Ui, response: &Response, session: &mut Session, scene: &Scene,
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
            ui.ctx().set_cursor_icon(cursor(grip, mods.alt));
            if down.is_none() {
                return false;
            }
            self.artboard_drag = Some(ArtboardDrag {
                board, start: at, current: at, grip, correction: [0.0; 2], duplicate: mods.alt, preview: None,
            });
            response.request_focus();
            self.claim(session);
        }

        let Some(mut drag) = self.artboard_drag.take() else { return false };
        if let Some(at) = up.or(pointer) {
            drag.current = at;
        }
        drag.duplicate |= mods.alt;
        let moved = drag.start.distance(drag.current) >= ui.ctx().options(|options| options.input_options.max_click_dist);
        if drag.grip.is_none() && moved {
            drag.correction = [0.0; 2];
            if let Some(board) = draft(session, rect, &drag, false) {
                drag.correction = self.snap_artboard(session, scene, drag.board, drag.duplicate, object_world_bounds(&board));
            }
            if primary && up.is_none() && drag.preview.is_none() {
                drag.preview = ArtboardPreview::capture(ui, session, scene, drag.board);
            }
        } else {
            self.clear_artboard_snap();
        }
        ui.ctx().set_cursor_icon(cursor(drag.grip, drag.duplicate));
        if up.is_some() || !primary {
            self.clear_artboard_snap();
            let command = if up.is_none() {
                None
            } else if !moved {
                click_select_command(Some(drag.board), &session.selection.ids, false)
            } else {
                draft(session, rect, &drag, mods.shift || crate::artboard_bar::aspect_locked(session)).and_then(|board| {
                    if drag.duplicate && drag.grip.is_none() {
                        let source = session.document.get_object(drag.board)?;
                        let offset = std::array::from_fn::<_, 2, _>(|i| board.pos[i] - source.pos[i]);
                        (offset != [0.0; 2]).then(|| Command::with_params(
                            dtool_tools::move_tool::ALT_DRAG_DUPLICATE_COMMAND,
                            json!({ "artboard": drag.board.0, "offset": offset }),
                        ))
                    } else {
                        Some(Command::with_params("artboard.update", json!({
                            "id": drag.board.0, "resize": drag.grip.is_some(),
                            "rect": [board.pos[0], board.pos[1], board.size[0], board.size[1]],
                        })))
                    }
                })
            };
            self.run(session, command);
            self.owner = None;
        } else {
            self.artboard_drag = Some(drag);
        }
        ui.ctx().request_repaint();
        true
    }

    fn clear_artboard_snap(&mut self) {
        self.snap_axes = [None; 2];
        self.guide_lines.clear();
        self.gap_guides.clear();
    }

    fn snap_artboard(&mut self, session: &Session, scene: &Scene, id: ObjectId, duplicate: bool, bounds: [f32; 4]) -> [f32; 2] {
        use crate::canvas_tools::{axis_snaps, box_alignment_guides, box_guide_offset, box_points, gap_guides, gap_snap, object_boxes};
        use dtool_core::snap_candidates::CandidateKind;

        self.clear_artboard_snap();
        let prefs = session.snap_prefs.query();
        if !prefs.enabled { return [0.0; 2]; }
        let exclude: Vec<_> = session.document.subtree_ids(id).into_iter()
            .filter(|object| !duplicate || *object != id).collect();
        let tolerance = prefs.screen_tolerance as f32 / session.view.zoom.max(1e-4);
        let axes = axis_snaps(session, &box_points(bounds, [0.0; 2]), &exclude);
        let gaps = prefs.object_snapping && prefs.snap_to_gaps_and_sizes;
        let edges = prefs.object_snapping && prefs.snap_to_object_bounds;
        let boxes = if edges || gaps {
            let mut boxes = object_boxes(session, scene, &[id]);
            boxes.extend(dtool_core::view::ViewTarget::artboards(&session.document)
                .filter(|board| (duplicate || board.id != id) && board.pickable && (!prefs.only_visible_objects || board.opacity > 0.0))
                .map(object_world_bounds));
            boxes
        } else { Vec::new() };
        let guides = if edges { box_guide_offset(&boxes, bounds, prefs.include_bbox_midpoints, tolerance) } else { [None; 2] };
        let correction = std::array::from_fn(|axis| {
            let mut chosen = axes[axis];
            if let Some(edge) = guides[axis]
                && chosen.is_none_or(|snap| edge.abs() <= snap.delta.abs() || snap.kind == CandidateKind::Pixel)
            {
                chosen = None;
            }
            let edge = chosen.map(|snap| snap.delta).or(guides[axis]);
            if gaps && let Some(gap) = gap_snap(&boxes, bounds, axis, tolerance)
                && edge.is_none_or(|delta| gap.delta.abs() <= delta.abs() || chosen.is_some_and(|snap| snap.kind == CandidateKind::Pixel))
            {
                return gap.delta;
            }
            self.snap_axes[axis] = chosen.filter(|snap| snap.kind != CandidateKind::Pixel).map(|snap| snap.point[axis]);
            edge.unwrap_or(0.0)
        });
        if prefs.dynamic_guides {
            let snapped = [bounds[0] + correction[0], bounds[1] + correction[1], bounds[2], bounds[3]];
            if edges { self.guide_lines = box_alignment_guides(&boxes, snapped, prefs.include_bbox_midpoints); }
            if gaps { self.gap_guides = gap_guides(&boxes, snapped, true); }
        }
        correction
    }

    pub(super) fn paint_artboard_drag(&self, ui: &Ui, rect: Rect, session: &Session, stroke: egui::Stroke) {
        let Some(drag) = &self.artboard_drag else { return };
        let preserve = ui.input(|input| input.modifiers.shift) || crate::artboard_bar::aspect_locked(session);
        let Some(board) = draft(session, rect, drag, preserve) else { return };
        let points = object_world_quad(&board).map(|point| screen(rect, &session.view, point));
        let painter = ui.painter_at(rect);
        if let Some(preview) = &drag.preview
            && let Some(source) = session.document.get_object(drag.board)
            && source.pos != board.pos
        {
            painter.add(egui::Shape::convex_polygon(points.to_vec(), egui::Color32::from_white_alpha(180), egui::Stroke::NONE));
            let [x, y, w, h] = preview.bounds;
            let x = x + board.pos[0] - source.pos[0];
            let y = y + board.pos[1] - source.pos[1];
            let mut mesh = egui::Mesh::with_texture(preview.texture.id());
            for (point, uv) in [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].into_iter()
                .zip([Pos2::ZERO, egui::pos2(1.0, 0.0), egui::pos2(1.0, 1.0), egui::pos2(0.0, 1.0)])
            {
                mesh.vertices.push(egui::epaint::Vertex { pos: screen(rect, &session.view, point), uv,
                    color: egui::Color32::from_white_alpha(210) });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(egui::Shape::mesh(mesh));
        }
        painter.add(egui::Shape::closed_line(points.to_vec(), stroke));
    }

    pub(crate) fn dragging_artboard(&self) -> bool {
        self.artboard_drag.is_some()
    }
}

impl ArtboardPreview {
    fn capture(ui: &Ui, session: &Session, scene: &Scene, id: ObjectId) -> Option<Self> {
        use dtool_core::layer_style::raster::{prim_object, render_scene_region};

        let doc = &session.document;
        let bounds @ [_, _, width, height] = object_world_bounds(doc.get_object(id)?);
        if !bounds.iter().all(|value| value.is_finite()) || width <= 0.0 || height <= 0.0 {
            return None;
        }
        let ids: std::collections::HashSet<_> = doc.subtree_ids(id).into_iter().collect();
        let prims = scene.prims.iter().filter(|prim| prim_object(prim).is_some_and(|object| ids.contains(&object)))
            .cloned().collect();
        let isolated = Scene { version: scene.version, prims, ..Scene::default() };
        // Rasterize the drag preview once at a bounded resolution. Pointer
        // updates translate the texture using the same snapped bounds as drop.
        let scale = (session.view.zoom * ui.ctx().pixels_per_point()).min(512.0 / width.max(height));
        let size = [width, height].map(|side| (side * scale).ceil().clamp(1.0, 512.0) as usize);
        let effect_scale = dtool_core::effect_raster::effect_scale(doc, size[0] as f32 / width);
        let pixels = render_scene_region(doc, &session.view.canvas_flags, &[], &isolated, bounds,
            size.map(|side| side as u32), effect_scale);
        let image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        let texture = ui.ctx().load_texture("artboard drag preview", image, egui::TextureOptions::LINEAR);
        Some(Self { bounds, texture })
    }
}

fn grip_point(board: &Object, [u, v]: [f32; 2]) -> [f32; 2] {
    [board.pos[0] + u * board.size[0].abs(), board.pos[1] + v * board.size[1].abs()]
}

fn hit(ui: &Ui, rect: Rect, session: &Session, at: Pos2) -> Option<(ObjectId, Option<[f32; 2]>)> {
    if session.view.canvas_flags.preview_mode {
        return None;
    }
    let labelled = dtool_core::view::ViewTarget::artboards(&session.document)
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
        }).last();
    if let Some(board) = labelled {
        return Some((board.id, None));
    }
    let board = selected_artboard(session).filter(|board| !board.locked)?;
    let matrix = object_matrix(board);
    GRIPS.into_iter().map(|grip| {
        let point = apply_matrix(&matrix, grip_point(board, grip));
        (screen(rect, &session.view, point).distance(at), grip)
    }).filter(|(distance, _)| *distance <= super::HANDLE_PX)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, grip)| (board.id, Some(grip)))
}

fn cursor(grip: Option<[f32; 2]>, duplicate: bool) -> egui::CursorIcon {
    match grip {
        Some([0.5, _]) => egui::CursorIcon::ResizeVertical,
        Some([_, 0.5]) => egui::CursorIcon::ResizeHorizontal,
        Some([u, v]) if u == v => egui::CursorIcon::ResizeNwSe,
        Some(_) => egui::CursorIcon::ResizeNeSw,
        None if duplicate => egui::CursorIcon::Copy,
        None => egui::CursorIcon::Move,
    }
}

fn draft(session: &Session, canvas: Rect, drag: &ArtboardDrag, preserve: bool) -> Option<Object> {
    let mut board = session.document.get_object(drag.board)?.clone();
    if board.kind != dtool_core::ObjectKind::Artboard {
        return None;
    }
    let to_doc = |point: Pos2| dtool_core::view_to_doc(&session.view, [point.x - canvas.min.x, point.y - canvas.min.y]);
    let start = to_doc(drag.start);
    let end = to_doc(drag.current);
    let delta = [end[0] - start[0], end[1] - start[1]];
    let Some(grip) = drag.grip else {
        board.pos = std::array::from_fn(|i| board.pos[i] + delta[i] + drag.correction[i]);
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
