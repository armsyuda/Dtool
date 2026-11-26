use std::sync::Arc;
use std::time::{Duration, Instant};

use dtool_core::brush::{BrushOp, BrushSettings, TipKind};
use dtool_core::commands::pixel_brush::{MAX_POINTS, STROKE, preview, settings, symmetry_frame};
use dtool_core::context_toolbar::OPTIONS_SET;
use dtool_core::{Command, Document, Session};
use egui::{Color32, Pos2, Rect, Response, TouchId, TouchPhase, Ui};
use serde_json::json;

const PREVIEW_INTERVAL: Duration = Duration::from_millis(32);
const SYMMETRY_HANDLE_RADIUS: f32 = 8.0;

struct SymmetryGuide {
    start: Pos2,
    x: egui::Vec2,
    y: egui::Vec2,
}

impl SymmetryGuide {
    fn new(session: &Session, rect: Rect) -> Option<Self> {
        let corners = symmetry_frame(session, [rect.width(), rect.height()])?;
        let [start, right, bottom] = corners.map(|point| super::screen(rect, &session.view, point));
        let x = right - start;
        let y = bottom - start;
        ((x.x * y.y - x.y * y.x).abs() > 0.001).then_some(Self { start, x, y })
    }

    fn position(&self, [u, v]: [f32; 2]) -> Pos2 {
        self.start + self.x * u + self.y * v
    }

    fn fraction(&self, point: Pos2) -> [f32; 2] {
        let offset = point - self.start;
        let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
        let area = cross(self.x, self.y);
        [cross(offset, self.y) / area, cross(self.x, offset) / area].map(|n| n.clamp(0.0, 1.0))
    }
}

#[derive(Debug, Clone, Copy)]
struct Sensors {
    pressure: Option<f32>,
    tilt: [f32; 2],
    time_ms: Option<u32>,
}

impl Default for Sensors {
    fn default() -> Self {
        Self {
            pressure: None,
            tilt: [0.0; 2],
            time_ms: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Contact {
    id: TouchId,
    phase: TouchPhase,
    pos: Pos2,
    sensors: Sensors,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    point: [f32; 2],
    screen: Pos2,
    time: f64,
    sensors: Sensors,
    velocity: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct Draft {
    pub revision: u64,
    pub document: Arc<Document>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Gesture {
    points: Vec<Sample>,
    outline: Option<(Pos2, f32, TipKind, f32, [f32; 2])>,
    owner: Option<(u64, String)>,
    contact: Option<TouchId>,
    draft: Option<Draft>,
    revision: u64,
    rendered_points: usize,
    last_preview: Option<Instant>,
    invert: bool,
    symmetry_drag: Option<[f32; 2]>,
}

impl Gesture {
    pub fn active(&self) -> bool {
        !self.points.is_empty() || self.symmetry_drag.is_some()
    }

    pub fn draft(&self, session: &Session) -> Option<Draft> {
        self.valid_for(session)
            .then(|| self.draft.clone())
            .flatten()
    }

    fn valid_for(&self, session: &Session) -> bool {
        self.owner.as_ref().is_some_and(|(generation, tool)| {
            *generation == session.generation && tool == &session.active_tool
        })
    }

    pub fn cancel(&mut self) {
        self.points.clear();
        self.outline = None;
        self.owner = None;
        self.contact = None;
        self.draft = None;
        self.rendered_points = 0;
        self.last_preview = None;
        self.symmetry_drag = None;
    }

    fn sample(&mut self, point: [f32; 2], screen: Pos2, time: f64, sensors: Sensors) {
        if self.points.last().is_some_and(|last| {
            last.point == point
                && last.sensors.pressure == sensors.pressure
                && last.sensors.tilt == sensors.tilt
        }) {
            return;
        }
        let velocity = self.points.last().map_or(0.0, |last| {
            let elapsed = match (sensors.time_ms, last.sensors.time_ms) {
                (Some(now), Some(before)) => f64::from(now.wrapping_sub(before)) / 1000.0,
                _ => time - last.time,
            }
            .max(1.0 / 240.0) as f32;
            (screen.distance(last.screen) / elapsed / 1500.0).clamp(0.0, 1.0)
        });
        self.points.push(Sample {
            point,
            screen,
            time,
            sensors,
            velocity,
        });
    }

    fn command(&self) -> Command {
        let points: Vec<_> = self.points.iter().map(|sample| sample.point).collect();
        let pressure = self.points.iter().any(|sample| sample.sensors.pressure.is_some())
            .then(|| self.points.iter().map(|sample| sample.sensors.pressure.unwrap_or(1.0))
                .collect::<Vec<_>>());
        let tilt: Vec<_> = self
            .points
            .iter()
            .map(|sample| sample.sensors.tilt)
            .collect();
        let velocity: Vec<_> = self.points.iter().map(|sample| sample.velocity).collect();
        Command::with_params(
            STROKE,
            json!({
                "points": points, "pressure": pressure, "tilt": tilt, "velocity": velocity, "invert": self.invert,
            }),
        )
    }

    fn update_draft(&mut self, ui: &Ui, session: &Session) {
        if self.points.len() == self.rendered_points {
            return;
        }
        if let Some(remaining) = self
            .last_preview
            .and_then(|last| PREVIEW_INTERVAL.checked_sub(last.elapsed()))
        {
            ui.ctx().request_repaint_after(remaining);
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        self.draft = preview(session, &self.command()).map(|document| Draft {
            revision: self.revision,
            document: Arc::new(document),
        });
        self.rendered_points = self.points.len();
        self.last_preview = Some(Instant::now());
        ui.ctx().request_repaint();
    }

    pub fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        session: &Session,
        op: BrushOp,
        to_doc: impl Fn(Pos2) -> [f32; 2],
    ) -> Option<Command> {
        if self.active() && !self.valid_for(session) {
            self.cancel();
        }
        let settings = settings(session, op);
        if let Some(command) = self.input_symmetry(ui, response, session, &settings) {
            return command;
        }
        let size = dtool_core::units::from_px(
            settings.size_px,
            session.document.units,
            session.document.dpi,
        )
        .unwrap_or(settings.size_px)
            * session.view.zoom;
        let aspect = settings
            .stamp
            .as_ref()
            .map_or([1.0; 2], |image| image.aspect());
        self.outline = response.hover_pos().map(|point| {
            (
                point,
                (size * 0.5).max(2.0),
                settings.tip_kind,
                settings.rotation,
                aspect,
            )
        });
        if self.outline.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }
        let (events, time) = ui.input(|input| (input.events.clone(), input.time));
        let mut pending = None::<Contact>;
        for event in events {
            match event {
                egui::Event::Touch {
                    id,
                    phase,
                    pos,
                    force,
                    ..
                } => {
                    let last = self.points.last().filter(|_| self.contact == Some(id));
                    let sensors = contact_sensors(
                        ui,
                        id,
                        phase,
                        pos,
                        force,
                        last.map(|sample| sample.sensors),
                    );
                    pending = Some(Contact {
                        id,
                        phase,
                        pos,
                        sensors,
                    });
                    if phase == TouchPhase::Cancel && self.contact == Some(id) {
                        self.cancel();
                        ui.ctx().request_repaint();
                    }
                }
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pos,
                    pressed: true,
                    modifiers,
                } if response.enabled()
                    && response.interact_rect.contains(pos)
                    && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
                    && !egui::Popup::is_any_open(ui.ctx()) =>
                {
                    let touch = pending.take().filter(|touch| {
                        touch.phase == TouchPhase::Start && touch.pos.distance(pos) < 0.5
                    });
                    self.cancel();
                    self.contact = touch.map(|touch| touch.id);
                    self.owner = Some((session.generation, session.active_tool.clone()));
                    self.sample(
                        to_doc(pos),
                        pos,
                        time,
                        touch.map_or_else(Sensors::default, |touch| touch.sensors),
                    );
                    self.invert = op == BrushOp::Tone && modifiers.alt;
                    response.request_focus();
                }
                egui::Event::PointerMoved(pos)
                    if self.active() && self.points.len() < MAX_POINTS =>
                {
                    let touch = pending.filter(|touch| {
                        Some(touch.id) == self.contact
                            && touch.phase == TouchPhase::Move
                            && touch.pos.distance(pos) < 0.5
                    });
                    if let Some(touch) = touch {
                        self.sample(to_doc(pos), pos, time, touch.sensors);
                        pending = None;
                    } else if self.contact.is_none() {
                        self.sample(to_doc(pos), pos, time, Sensors::default());
                    }
                }
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pos,
                    pressed: false,
                    ..
                } if self.active() => {
                    let touch = pending.take().filter(|touch| {
                        Some(touch.id) == self.contact && touch.phase == TouchPhase::End
                    });
                    let (pos, sensors) = match (touch, self.contact) {
                        (Some(touch), _) => (touch.pos, touch.sensors),
                        (None, Some(_)) => self
                            .points
                            .last()
                            .map_or((pos, Sensors::default()), |last| {
                                (last.screen, last.sensors)
                            }),
                        _ => (pos, Sensors::default()),
                    };
                    if self.points.len() == MAX_POINTS {
                        self.points.pop();
                    }
                    self.sample(to_doc(pos), pos, time, sensors);
                    let command = self.command();
                    self.cancel();
                    ui.ctx().request_repaint();
                    return Some(command);
                }
                egui::Event::PointerGone => {
                    self.cancel();
                    ui.ctx().request_repaint();
                }
                _ => {}
            }
        }
        if ui.input(|input| input.pointer.primary_down()) {
            self.update_draft(ui, session);
        } else if self.active() {
            self.cancel();
            ui.ctx().request_repaint();
        }
        None
    }

    fn input_symmetry(
        &mut self, ui: &Ui, response: &Response, session: &Session, settings: &BrushSettings,
    ) -> Option<Option<Command>> {
        if settings.symmetry_lock || (settings.symmetry == 1 && !settings.mirror) {
            if self.symmetry_drag.is_some() { self.cancel(); }
            return None;
        }
        if !self.points.is_empty() { return None; }
        let guide = SymmetryGuide::new(session, response.rect)?;
        let handle = guide.position(settings.symmetry_origin);
        let (events, primary_down) = ui.input(|input| (input.events.clone(), input.pointer.primary_down()));
        let mut captured = self.symmetry_drag.is_some();
        for event in events {
            match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                    if response.enabled() && response.interact_rect.contains(pos)
                        && handle.distance(pos) <= SYMMETRY_HANDLE_RADIUS
                        && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
                        && !egui::Popup::is_any_open(ui.ctx()) =>
                {
                    self.cancel();
                    self.owner = Some((session.generation, session.active_tool.clone()));
                    self.symmetry_drag = Some(settings.symmetry_origin);
                    captured = true;
                    response.request_focus();
                }
                egui::Event::PointerMoved(pos) if captured => {
                    self.symmetry_drag = Some(guide.fraction(pos));
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } if captured => {
                    let [x, y] = guide.fraction(pos);
                    self.cancel();
                    ui.ctx().request_repaint();
                    return Some(Some(Command::with_params(OPTIONS_SET, json!({
                        "tool": session.active_tool,
                        "options": { "brush.symmetry_x": x * 100.0, "brush.symmetry_y": y * 100.0 },
                    }))));
                }
                egui::Event::PointerGone if captured => {
                    self.cancel();
                    ui.ctx().request_repaint();
                    return Some(None);
                }
                _ => {}
            }
        }
        if captured && !primary_down { self.cancel(); }
        if captured || response.hover_pos().is_some_and(|p| handle.distance(p) <= SYMMETRY_HANDLE_RADIUS) {
            self.outline = None;
            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            if captured { ui.ctx().request_repaint(); }
            return Some(None);
        }
        None
    }

    pub fn paint(&self, ui: &Ui, rect: Rect, session: &Session) {
        if dtool_core::commands::pixel_brush::operation(&session.active_tool).is_none() {
            return;
        }
        let painter = ui.painter_at(rect);
        self.paint_symmetry(&painter, rect, session);
        if let Some((point, radius, kind, rotation, aspect)) = self
            .outline
            .filter(|(point, ..)| rect.contains(*point) && ui.rect_contains_pointer(rect))
        {
            for (width, colour) in [(3.2, super::CURSOR_EDGE), (2.2, super::CURSOR_FACE)] {
                let stroke = egui::Stroke::new(width, colour);
                if kind != TipKind::Round {
                    let rot = egui::emath::Rot2::from_angle(
                        rotation.to_radians() + session.view.rotation_deg.to_radians(),
                    );
                    let vertices =
                        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[x, y]| {
                            point + rot * egui::vec2(x * radius * aspect[0], y * radius * aspect[1])
                        });
                    painter.add(egui::Shape::closed_line(vertices.to_vec(), stroke));
                } else {
                    painter.circle_stroke(point, radius, stroke);
                }
            }
        }
    }

    fn paint_symmetry(&self, painter: &egui::Painter, rect: Rect, session: &Session) {
        let tool = &session.active_tool;
        let number = |id, fallback| session.tool_options.number(tool, id).unwrap_or(fallback) as f32;
        let count = number("brush.symmetry", 1.0) as u32;
        let mirror = session.tool_options.flag(tool, "brush.mirror") == Some(true);
        if count <= 1 && !mirror || session.view.canvas_flags.preview_mode { return; }
        let Some(guide) = SymmetryGuide::new(session, rect) else { return; };
        let fraction = self.symmetry_drag.unwrap_or_else(|| {
            [number("brush.symmetry_x", 50.0) / 100.0, number("brush.symmetry_y", 50.0) / 100.0]
        });
        let at = guide.position(fraction);
        let color = Color32::from_rgb(110, 165, 211);
        let line = egui::Stroke::new(1.0, color.gamma_multiply(0.65));
        let reach = rect.size().length();
        if mirror {
            let dir = guide.y.normalized() * reach;
            painter.line_segment([at - dir, at + dir], line);
        }
        if count > 1 {
            for copy in 0..count {
                let (sin, cos) = (std::f32::consts::TAU * copy as f32 / count as f32).sin_cos();
                let dir = guide.x.normalized() * cos + guide.y.normalized() * sin;
                painter.line_segment([at, at + dir * reach], line);
            }
        }
        let locked = session.tool_options.flag(tool, "brush.symmetry_lock") == Some(true);
        painter.circle(at, if locked { 3.0 } else { 5.0 }, Color32::from_gray(38), egui::Stroke::new(1.25, color));
    }
}

fn contact_sensors(
    ui: &Ui,
    id: TouchId,
    phase: TouchPhase,
    pos: Pos2,
    force: Option<f32>,
    previous: Option<Sensors>,
) -> Sensors {
    let mut sensors = Sensors {
        pressure: force.map(|value| value.clamp(0.0, 1.0)),
        ..Default::default()
    };
    #[cfg(not(windows))]
    let _ = (ui, id, pos);
    #[cfg(windows)]
    if let Some(pen) = crate::tablet_input::consume(id, phase, pos, ui.ctx().pixels_per_point()) {
        sensors = Sensors {
            pressure: pen.pressure,
            tilt: pen.tilt,
            time_ms: Some(pen.time_ms),
        };
    }
    if phase == TouchPhase::End
        && let Some(previous) = previous
    {
        sensors.pressure = previous.pressure;
        sensors.tilt = previous.tilt;
    }
    sensors
}
