use dtool_core::command::TransformSelectionBoxArgs;
use dtool_core::curve::Curve;
use dtool_core::object::ObjectId;
use dtool_core::scene::Scene;
use dtool_core::scene_hit::selection_box;
use dtool_core::Session;
use dtool_protocol::Command;
use egui::{pos2, Pos2, Rect, Response, Stroke, Ui};

use super::{primary_button_positions, screen, HANDLE_PX};

#[derive(Clone, Copy, Debug)]
enum Drag {
    Origin { before: Option<Pos2> },
    Point { press: Pos2, anchor: Pos2 },
}

#[derive(Clone, Debug, Default)]
pub(super) struct PointTransform {
    context: Option<u64>,
    ids: Vec<ObjectId>,
    origin: Option<Pos2>,
    drag: Option<Drag>,
}

impl PointTransform {
    pub(super) fn active(&self) -> bool {
        self.drag.is_some()
    }

    pub(super) fn cancel(&mut self) {
        if let Some(Drag::Origin { before }) = self.drag.take() {
            self.origin = before;
        }
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        scene: &Scene,
        session: &Session,
        context: Option<u64>,
        to_doc: impl Fn(Pos2) -> [f32; 2],
    ) -> (Option<TransformSelectionBoxArgs>, Option<Command>) {
        if self.context != context || self.ids != session.selection.ids {
            self.context = context;
            self.ids.clone_from(&session.selection.ids);
            self.cancel();
            self.origin = None;
        }
        let Some(origin) = self.origin.or_else(|| center(scene, session)) else {
            return (None, None);
        };
        if self
            .ids
            .iter()
            .any(|id| session.document.get_object(*id).is_none_or(|o| o.locked))
        {
            return (None, None);
        }
        let to_doc = |point| {
            let [x, y] = to_doc(point);
            pos2(x, y)
        };
        let reach = (HANDLE_PX + 2.0) / session.view.zoom.max(1e-4);
        let snapping = session
            .tool_options
            .flag("tool.point_transform", "point_transform.snap_to_points")
            .unwrap_or(true);
        let polar = session
            .tool_options
            .flag("tool.point_transform", "point_transform.rotate_scale")
            .unwrap_or(true);
        let snap = |target, selected_only| {
            nearest(scene, session, target, reach, selected_only).unwrap_or(target)
        };
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let (press, release) = primary_button_positions(ui);
        if let Some(at) = press
            .filter(|p| {
                response.interact_rect.contains(*p)
                    && (response.contains_pointer() || response.is_pointer_button_down_on())
            })
            .map(to_doc)
        {
            self.drag = match nearest(scene, session, at, reach, true) {
                Some(anchor)
                    if origin.distance(at) > reach && origin.distance(anchor) > f32::EPSILON =>
                {
                    Some(Drag::Point { press: at, anchor })
                }
                _ => Some(Drag::Origin {
                    before: self.origin,
                }),
            };
        }
        let pointer = release
            .or_else(|| response.interact_pointer_pos())
            .or_else(|| ui.input(|i| i.pointer.latest_pos()));
        let mut preview = None;
        if let Some((drag, point)) = self.drag.zip(pointer.map(to_doc)) {
            match drag {
                Drag::Origin { .. } => {
                    self.origin = Some(if snapping { snap(point, false) } else { point })
                }
                Drag::Point { press, anchor } if point.distance(press) > reach * 0.25 => {
                    let target = anchor + (point - press);
                    let target = if snapping {
                        snap(target, false)
                    } else {
                        target
                    };
                    preview = transform(&self.ids, origin, anchor, target, polar);
                }
                Drag::Point { .. } => {}
            }
            ui.ctx().request_repaint();
        }
        if release.is_some() {
            self.drag = None;
            let command = preview
                .and_then(|args| serde_json::to_value(args).ok())
                .map(|args| Command::with_params("transform.selection_box", args));
            return (None, command);
        }
        (preview, None)
    }

    pub(super) fn paint(
        &self,
        ui: &Ui,
        rect: Rect,
        session: &Session,
        scene: &Scene,
        stroke: Stroke,
        preview: Option<&TransformSelectionBoxArgs>,
    ) {
        let Some(centre) = center(scene, session) else {
            return;
        };
        let origin = self.origin.unwrap_or(centre);
        let painter = ui.painter_at(rect);
        let to_screen = |p: Pos2| screen(rect, &session.view, [p.x, p.y]);
        for id in &self.ids {
            for point in points(scene, session, *id) {
                let at = preview.map_or(point, |args| {
                    let [x, y] = dtool_core::command::box_transform_point(args, [point.x, point.y]);
                    pos2(x, y)
                });
                painter.circle_stroke(to_screen(at), 2.5, stroke);
            }
        }
        let marker = to_screen(origin);
        painter.circle(marker, 5.0, ui.visuals().panel_fill, stroke);
        painter.line_segment(
            [marker - egui::vec2(7.0, 0.0), marker + egui::vec2(7.0, 0.0)],
            stroke,
        );
        painter.line_segment(
            [marker - egui::vec2(0.0, 7.0), marker + egui::vec2(0.0, 7.0)],
            stroke,
        );
        if let (Some(Drag::Point { anchor, .. }), Some(args)) = (self.drag, preview) {
            let [x, y] = dtool_core::command::box_transform_point(args, [anchor.x, anchor.y]);
            let at = to_screen(pos2(x, y));
            painter.extend(egui::Shape::dashed_line(&[marker, at], stroke, 4.0, 3.0));
            painter.text(
                at + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.0}% · {:.1}°", args.scale[0] * 100.0, args.rotation_deg),
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().text_color(),
            );
        }
    }
}

fn center(scene: &Scene, session: &Session) -> Option<Pos2> {
    let [x, y, w, h] = selection_box(scene, &session.document, &session.selection.ids)?;
    Some(pos2(x + w * 0.5, y + h * 0.5))
}

fn points(scene: &Scene, session: &Session, id: ObjectId) -> Vec<Pos2> {
    let Some(quad) = scene.quad(id) else {
        return Vec::new();
    };
    let quad = quad.map(|[x, y]| pos2(x, y));
    let Some(object) = session.document.get_object(id) else {
        return quad.to_vec();
    };
    let outline;
    let curve = if let Some(curve) = object.curve() {
        curve
    } else {
        let Some(path) = dtool_core::outline::object_outline(object) else {
            return quad.to_vec();
        };
        outline = Curve::from_kurbo(&path);
        &outline
    };
    let [w, h] = object.size.map(|v| v.abs().max(1e-4));
    let x_axis = (quad[1] - quad[0]) / w;
    let y_axis = (quad[3] - quad[0]) / h;
    curve
        .contours()
        .flat_map(|contour| &contour.points)
        .map(|point| quad[0] + x_axis * point.pos[0] as f32 + y_axis * point.pos[1] as f32)
        .collect()
}

fn nearest(
    scene: &Scene,
    session: &Session,
    at: Pos2,
    reach: f32,
    selected_only: bool,
) -> Option<Pos2> {
    session
        .document
        .objects
        .iter()
        .filter(|o| !selected_only || session.selection.ids.contains(&o.id))
        .flat_map(|object| points(scene, session, object.id))
        .filter(|p| p.distance(at) <= reach)
        .min_by(|a, b| a.distance_sq(at).total_cmp(&b.distance_sq(at)))
}

fn transform(
    ids: &[ObjectId],
    origin: Pos2,
    anchor: Pos2,
    target: Pos2,
    polar: bool,
) -> Option<TransformSelectionBoxArgs> {
    let (start, end) = (anchor - origin, target - origin);
    let scale = if polar {
        (end.length() / start.length()).max(0.001)
    } else {
        1.0
    };
    let angle = if polar {
        start
            .x
            .mul_add(end.y, -start.y * end.x)
            .atan2(start.dot(end))
            .to_degrees()
    } else {
        0.0
    };
    let translate = if polar {
        [0.0, 0.0]
    } else {
        [target.x - anchor.x, target.y - anchor.y]
    };
    if (scale - 1.0).abs() < 1e-5 && angle.abs() < 1e-5 && translate == [0.0, 0.0] {
        return None;
    }
    Some(TransformSelectionBoxArgs {
        ids: Some(ids.iter().map(|id| id.0).collect()),
        origin: [origin.x, origin.y],
        translate,
        scale: [scale, scale],
        rotation_deg: angle,
        shear_deg: 0.0,
        ..Default::default()
    })
}
