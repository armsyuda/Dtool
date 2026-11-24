use dtool_core::boolean::{shape_builder_regions, SHAPE_BUILDER_APPLY};
use dtool_core::object::ObjectId;
use dtool_core::scene::Scene;
use dtool_core::scene_hit::{hit_test_in_view, HIT_TOLERANCE_PX};
use dtool_core::Session;
use dtool_protocol::Command;
use egui::{pos2, Event, PointerButton, Pos2, Rect, Response, Stroke, Ui};
use serde_json::json;

use super::{click_select_command, screen, GestureOwner};

const TOOL: &str = "tool.shape_builder";

#[derive(Debug, Clone, Copy)]
enum DragMethod {
    Freehand,
    Line,
    Marquee,
}

#[derive(Debug, Clone)]
struct Gesture {
    method: DragMethod,
    start: Pos2,
    end: Pos2,
    picked: Vec<usize>,
}

#[derive(Debug, Clone)]
struct Region {
    rings: Vec<Vec<Pos2>>,
    bounds: Rect,
}

impl Region {
    fn edges(&self) -> impl Iterator<Item = (Pos2, Pos2)> + '_ {
        self.rings.iter().flat_map(|ring| {
            ring.iter()
                .copied()
                .zip(ring.iter().copied().cycle().skip(1))
                .take(ring.len())
        })
    }

    fn contains(&self, p: Pos2) -> bool {
        self.bounds.contains(p)
            && self
                .edges()
                .filter(|(a, b)| {
                    (a.y > p.y) != (b.y > p.y)
                        && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x
                })
                .count()
                % 2
                == 1
    }

    fn crossing(&self, start: Pos2, end: Pos2) -> Option<f32> {
        if self.contains(start) {
            return Some(0.0);
        }
        if !self.bounds.intersects(Rect::from_two_pos(start, end)) {
            return None;
        }
        self.edges()
            .filter_map(|(a, b)| segment_crossing(start, end, a, b))
            .min_by(f32::total_cmp)
            .or_else(|| self.contains(end).then_some(1.0))
    }

    fn intersects(&self, rect: Rect) -> bool {
        if !self.bounds.intersects(rect) {
            return false;
        }
        let corners = [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
        corners
            .iter()
            .enumerate()
            .any(|(index, p)| self.crossing(*p, corners[(index + 1) % 4]).is_some())
            || self.rings.iter().flatten().any(|p| rect.contains(*p))
    }
}

fn segment_crossing(start: Pos2, end: Pos2, a: Pos2, b: Pos2) -> Option<f32> {
    let r = end - start;
    let s = b - a;
    let offset = a - start;
    let cross = r.x * s.y - r.y * s.x;
    if cross.abs() <= f32::EPSILON {
        return None;
    }
    let along = (offset.x * s.y - offset.y * s.x) / cross;
    let across = (offset.x * r.y - offset.y * r.x) / cross;
    ((0.0..=1.0).contains(&along) && (0.0..=1.0).contains(&across)).then_some(along)
}

#[derive(Debug, Clone, Default)]
pub(super) struct ShapeBuilder {
    stamp: Option<GestureOwner>,
    targets: Vec<ObjectId>,
    regions: Vec<Region>,
    gesture: Option<Gesture>,
    hover: Option<usize>,
}

impl ShapeBuilder {
    pub(super) fn active(&self) -> bool {
        self.gesture.is_some()
    }

    pub(super) fn cancel(&mut self) {
        self.gesture = None;
        self.hover = None;
    }

    fn sync(&mut self, session: &Session, stamp: GestureOwner) {
        if self.stamp.as_ref() == Some(&stamp) && self.targets == session.selection.ids {
            return;
        }
        self.cancel();
        self.stamp = Some(stamp);
        self.targets.clone_from(&session.selection.ids);
        self.regions = shape_builder_regions(session)
            .unwrap_or_default()
            .into_iter()
            .map(|shape| {
                let rings: Vec<Vec<Pos2>> = shape
                    .into_iter()
                    .map(|ring| {
                        ring.into_iter()
                            .map(|[x, y]| pos2(x as f32, y as f32))
                            .collect()
                    })
                    .collect();
                let bounds = rings.iter().flatten().fold(Rect::NOTHING, |bounds, p| {
                    bounds.union(Rect::from_min_max(*p, *p))
                });
                Region { rings, bounds }
            })
            .collect();
    }

    pub(super) fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        session: &Session,
        scene: &Scene,
        stamp: GestureOwner,
        to_doc: impl Fn(Pos2) -> [f32; 2],
    ) -> Option<Command> {
        self.sync(session, stamp);
        let selecting = self.regions.is_empty();
        if selecting {
            response
                .clone()
                .on_hover_text(dtool_strings::text("tool.shape_builder.select_hint"));
        }
        let doc_point = |p| {
            let [x, y] = to_doc(p);
            pos2(x, y)
        };
        let pointer = ui.input(|i| i.pointer.latest_pos());
        self.hover = pointer.filter(|_| response.hovered()).and_then(|p| {
            self.regions
                .iter()
                .position(|region| region.contains(doc_point(p)))
        });
        if response.hovered() || self.active() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let captures_press = response.contains_pointer() || response.is_pointer_button_down_on();
        let command = ui.input(|i| {
            for event in &i.events {
                match *event {
                    Event::PointerButton {
                        button: PointerButton::Primary,
                        pressed: true,
                        pos,
                        ..
                    } if captures_press && response.interact_rect.contains(pos) => {
                        let method = match (
                            selecting,
                            session
                                .tool_options
                                .choice(TOOL, "shape_builder.drag_method")
                                .as_deref(),
                        ) {
                            (true, _) | (_, Some("marquee")) => DragMethod::Marquee,
                            (_, Some("line")) => DragMethod::Line,
                            _ => DragMethod::Freehand,
                        };
                        let start = doc_point(pos);
                        let mut gesture = Gesture {
                            method,
                            start,
                            end: start,
                            picked: Vec::new(),
                        };
                        gesture.track(start, &self.regions);
                        self.gesture = Some(gesture);
                    }
                    Event::PointerMoved(p) => {
                        if let Some(gesture) = &mut self.gesture {
                            gesture.track(doc_point(p), &self.regions);
                        }
                    }
                    Event::PointerButton {
                        button: PointerButton::Primary,
                        pressed: false,
                        pos,
                        modifiers,
                    } => {
                        if let Some(mut gesture) = self.gesture.take() {
                            gesture.track(doc_point(pos), &self.regions);
                            if selecting {
                                return select_objects(session, scene, &gesture, modifiers.shift);
                            }
                            return self.command(session, gesture, modifiers.alt);
                        }
                    }
                    _ => {}
                }
            }
            if !i.pointer.primary_down() {
                self.gesture = None;
            }
            None
        });
        if command.is_some() {
            self.hover = None;
        }
        command
    }

    fn command(&self, session: &Session, gesture: Gesture, alt: bool) -> Option<Command> {
        let mut areas = gesture.picked;
        if areas.is_empty() {
            return None;
        }
        let choice = |key| session.tool_options.choice(TOOL, key);
        let action = if alt {
            "delete".into()
        } else {
            choice("shape_builder.action").unwrap_or_else(|| "add".into())
        };
        let cleanup = choice("shape_builder.cleanup").unwrap_or_else(|| "none".into());
        let first_area = areas.iter().position(|&index| self.regions[index].contains(gesture.start));
        let first_style = first_area.is_some()
            && session.tool_options.flag(TOOL, "shape_builder.first_style").unwrap_or(true);
        if let Some(start) = first_area.filter(|_| first_style) {
            areas[..=start].rotate_right(1);
        }
        Some(Command::with_params(
            SHAPE_BUILDER_APPLY,
            json!({
                "ids": self.targets, "areas": areas, "op": action, "cleanup": cleanup, "first_style": first_style,
            }),
        ))
    }

    pub(super) fn paint(&self, ui: &Ui, rect: Rect, session: &Session, stroke: Stroke) {
        let painter = ui.painter_at(rect);
        let to_screen = |p: Pos2| screen(rect, &session.view, [p.x, p.y]);
        let picked = self
            .gesture
            .as_ref()
            .map(|g| g.picked.as_slice())
            .unwrap_or(&[]);
        for &index in picked
            .iter()
            .chain(self.hover.iter().filter(|index| !picked.contains(index)))
        {
            if let Some(region) = self.regions.get(index) {
                let rings: Vec<Vec<Pos2>> = region
                    .rings
                    .iter()
                    .map(|ring| ring.iter().copied().map(to_screen).collect())
                    .collect();
                paint_region(&painter, rect, rings, stroke);
            }
        }
        if let Some(gesture) = &self.gesture {
            let start = to_screen(gesture.start);
            let end = to_screen(gesture.end);
            match gesture.method {
                DragMethod::Line => {
                    painter.line_segment([start, end], stroke);
                }
                DragMethod::Marquee => {
                    painter.rect_stroke(
                        Rect::from_two_pos(start, end),
                        0.0,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                DragMethod::Freehand => {}
            }
        }
    }
}

fn select_objects(
    session: &Session,
    scene: &Scene,
    gesture: &Gesture,
    shift: bool,
) -> Option<Command> {
    let tolerance = HIT_TOLERANCE_PX / session.view.zoom.max(1e-4);
    if gesture.start.distance(gesture.end) < tolerance {
        let hit = hit_test_in_view(
            scene,
            session,
            [gesture.end.x, gesture.end.y],
            tolerance,
            None,
        );
        return click_select_command(hit, &session.selection.ids, shift);
    }
    let area = Rect::from_two_pos(gesture.start, gesture.end);
    let mode = if gesture.end.x >= gesture.start.x {
        "Enclosed"
    } else {
        "Touch"
    };
    let op = if shift { "Add" } else { "Replace" };
    Some(Command::with_params(
        "selection.marquee",
        json!({
            "rect": [area.min.x, area.min.y, area.width(), area.height()], "mode": mode, "op": op,
        }),
    ))
}

impl Gesture {
    fn track(&mut self, point: Pos2, regions: &[Region]) {
        match self.method {
            DragMethod::Marquee => {
                let rect = Rect::from_two_pos(self.start, point);
                self.picked = regions
                    .iter()
                    .enumerate()
                    .filter(|(_, region)| region.intersects(rect))
                    .map(|(index, _)| index)
                    .collect();
            }
            DragMethod::Freehand | DragMethod::Line => {
                let start = match self.method {
                    DragMethod::Line => {
                        self.picked.clear();
                        self.start
                    }
                    _ => self.end,
                };
                let mut crossed: Vec<_> = regions
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !self.picked.contains(index))
                    .filter_map(|(index, region)| region.crossing(start, point).map(|t| (index, t)))
                    .collect();
                crossed.sort_by(|a, b| a.1.total_cmp(&b.1));
                self.picked
                    .extend(crossed.into_iter().map(|(index, _)| index));
            }
        }
        self.end = point;
    }
}

fn paint_region(painter: &egui::Painter, clip: Rect, rings: Vec<Vec<Pos2>>, stroke: Stroke) {
    let bounds = rings
        .iter()
        .flatten()
        .fold(Rect::NOTHING, |bounds, p| {
            bounds.union(Rect::from_min_max(*p, *p))
        })
        .intersect(clip);
    if bounds.is_positive() {
        let edges: Vec<_> = rings
            .iter()
            .flat_map(|ring| {
                ring.iter()
                    .copied()
                    .zip(ring.iter().copied().cycle().skip(1))
                    .take(ring.len())
            })
            .collect();
        let span = bounds.width() + bounds.height();
        let spacing = (span * edges.len() as f32 / 500_000.0).max(8.0);
        let mut intercept = ((bounds.min.x + bounds.min.y) / spacing).ceil() * spacing;
        let hatch = Stroke::new(1.0, stroke.color.gamma_multiply(0.45));
        while intercept <= bounds.max.x + bounds.max.y {
            let mut crossing: Vec<_> = edges
                .iter()
                .filter_map(|(a, b)| {
                    let a_sum = a.x + a.y;
                    let b_sum = b.x + b.y;
                    ((a_sum > intercept) != (b_sum > intercept))
                        .then(|| a.x + (b.x - a.x) * (intercept - a_sum) / (b_sum - a_sum))
                })
                .collect();
            crossing.sort_by(f32::total_cmp);
            for pair in crossing.chunks_exact(2) {
                painter.line_segment(
                    [
                        pos2(pair[0], intercept - pair[0]),
                        pos2(pair[1], intercept - pair[1]),
                    ],
                    hatch,
                );
            }
            intercept += spacing;
        }
    }
    for ring in rings {
        painter.add(egui::Shape::closed_line(ring, stroke));
    }
}
