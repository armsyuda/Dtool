mod adjustment;

use std::sync::Arc;
use std::time::{Duration, Instant};

use dtool_core::brush::{BrushOp, BrushSettings, TipKind};
use dtool_core::commands::pixel_brush::{CloneTransform, LiveStroke, MAX_POINTS, STROKE, clone_layer, preview_with_layer, settings, symmetry_frame};
use dtool_core::context_toolbar::{CLONE_ALIGNED, CLONE_PICK_SOURCE, CLONE_SCOPE, HEAL_TOOL, OPTIONS_SET, uses_brush_source};
use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};
use dtool_core::{Command, Document, Session};
use egui::{Color32, Pos2, Rect, Response, TouchId, TouchPhase, Ui};
use serde_json::json;

const PREVIEW_INTERVAL: Duration = Duration::from_millis(16);
const SYMMETRY_HANDLE_RADIUS: f32 = 8.0;

fn using_global(session: &Session) -> bool {
    uses_brush_source(&session.active_tool)
        && session.tool_options.choice(&session.active_tool, CLONE_SCOPE).as_deref() == Some("global")
}

fn tip_aspect(settings: &BrushSettings) -> [f32; 2] {
    let mut aspect = settings.stamp.as_ref().map_or([1.0; 2], |image| image.aspect());
    let minor = usize::from(aspect[0] >= aspect[1]);
    aspect[minor] *= settings.roundness;
    aspect
}

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
    pub layer: Option<dtool_core::ObjectId>,
}

#[derive(Debug, Clone)]
struct CloneSource {
    context: Option<dtool_core::ContextId>,
    document: dtool_core::document::DocInstance,
    layer: dtool_core::ObjectId,
    fraction: [f32; 2],
    aligned_destination: Option<[f32; 2]>,
}

fn layer_fraction(layer: &dtool_core::Object, point: [f32; 2]) -> Option<[f32; 2]> {
    let local = apply_matrix(&invert_matrix(&object_matrix(layer))?, point);
    let fraction = std::array::from_fn(|axis| (local[axis] - layer.pos[axis]) / layer.size[axis].abs());
    fraction.iter().all(|value| value.is_finite()).then_some(fraction)
}

fn layer_world(layer: &dtool_core::Object, fraction: [f32; 2]) -> [f32; 2] {
    apply_matrix(&object_matrix(layer), std::array::from_fn(|axis| {
        layer.pos[axis] + fraction[axis] * layer.size[axis].abs()
    }))
}

impl CloneSource {
    fn layer<'a>(&self, session: &'a Session) -> Option<&'a dtool_core::Object> {
        // A history restore replaces the document instance within the same workspace tab.
        let same_document = match self.context {
            Some(context) => session.context == Some(context),
            None => session.context.is_none() && self.document == session.document.instance,
        };
        if !same_document || clone_layer(session) != Some(self.layer) {
            return None;
        }
        session.document.get_object(self.layer)
    }
}

#[derive(Debug, Clone)]
struct GlobalAlignment {
    source: (u64, u64),
    target: CloneSource,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Gesture {
    points: Vec<Sample>,
    outline: Option<(Pos2, f32, TipKind, f32, [f32; 2])>,
    owner: Option<(u64, String)>,
    adjustment: adjustment::Adjustment,
    contact: Option<TouchId>,
    draft: Option<Draft>,
    raster_preview: LiveStroke,
    revision: u64,
    rendered_points: usize,
    last_preview: Option<Instant>,
    invert: bool,
    symmetry_drag: Option<[f32; 2]>,
    clone_source: Option<CloneSource>,
    clone_stroke_start: Option<[f32; 2]>,
    clone_stroke_destination: Option<[f32; 2]>,
    global_alignment: Option<GlobalAlignment>,
    global_stroke_anchor: Option<GlobalAlignment>,
    global_stroke_offset: Option<[f32; 2]>,
}

impl Gesture {
    pub fn active(&self) -> bool {
        !self.points.is_empty() || self.symmetry_drag.is_some() || self.adjustment.active()
    }

    pub fn adjusting(&self) -> bool {
        self.adjustment.active()
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
        self.adjustment = adjustment::Adjustment::default();
        self.contact = None;
        self.draft = None;
        self.raster_preview = LiveStroke::default();
        self.rendered_points = 0;
        self.last_preview = None;
        self.symmetry_drag = None;
        self.clone_stroke_start = None;
        self.clone_stroke_destination = None;
        self.global_stroke_anchor = None;
        self.global_stroke_offset = None;
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
        let mut params = json!({
            "points": points, "pressure": pressure, "tilt": tilt, "velocity": velocity, "invert": self.invert,
        });
        if let Some(source) = self.clone_stroke_start {
            params["clone_source"] = json!(source);
        }
        if let Some(offset) = self.global_stroke_offset {
            params["clone_global_offset"] = json!(offset);
        }
        Command::with_params(STROKE, params)
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
        self.last_preview = Some(Instant::now());
        self.revision = self.revision.wrapping_add(1);
        let command = self.command();
        self.draft = preview_with_layer(session, &command, Some(&mut self.raster_preview)).map(|(document, layer)| Draft {
            revision: self.revision,
            document: Arc::new(document),
            layer,
        });
        self.rendered_points = self.points.len();
        ui.ctx().request_repaint();
    }

    pub fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        session: &mut Session,
        op: BrushOp,
        to_doc: impl Fn(Pos2) -> [f32; 2],
    ) -> Option<Command> {
        if self.active() && !self.valid_for(session) {
            self.cancel();
        }
        let settings = settings(session, op);
        if op != BrushOp::Pixel && self.points.is_empty() && self.symmetry_drag.is_none()
            && self.adjustment.input(ui, response, session, &settings)
        {
            self.owner = Some((session.generation, session.active_tool.clone()));
            self.outline = None;
            return None;
        }
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
        let aspect = tip_aspect(&settings);
        self.outline = response.hover_pos().map(|point| {
            (
                point,
                (size * 0.5).max(2.0),
                settings.tip_kind,
                settings.rotation,
                aspect,
            )
        });
        let sampled = op.uses_sample_source();
        let global = using_global(session);
        let pick_source = sampled
            && session.tool_options.flag(&session.active_tool, CLONE_PICK_SOURCE) == Some(true);
        let aligned = session.tool_options.flag(&session.active_tool, CLONE_ALIGNED).unwrap_or(true);
        if op == BrushOp::Undo && (session.history.brush_source.is_none() || clone_layer(session).is_none()) {
            self.cancel();
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
            }
            return None;
        }
        if self.outline.is_some() {
            let needs_source = sampled && if global {
                session.clone_sources.selected().is_none()
            } else {
                self.clone_source.as_ref().and_then(|source| source.layer(session)).is_none()
            };
            let picking = sampled && (pick_source || ui.input(|input| input.modifiers.alt));
            if needs_source || picking {
                self.outline = None;
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            }
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
                    let point = to_doc(pos);
                    if sampled {
                        if pick_source || modifiers.alt {
                            if self.pick_clone_source(session, point) {
                                response.request_focus();
                                ui.ctx().request_repaint();
                                if pick_source || global {
                                    let mut options = json!({(CLONE_PICK_SOURCE): false});
                                    if global {
                                        let local = if session.active_tool == HEAL_TOOL { "current" } else { "current_and_below" };
                                        options[CLONE_SCOPE] = json!(local);
                                    }
                                    return Some(Command::with_params(OPTIONS_SET, json!({
                                        "tool": session.active_tool, "options": options,
                                    })));
                                }
                            }
                            continue;
                        }
                        let started = if global { self.start_global_stroke(session, point, aligned) }
                            else { self.start_clone_stroke(session, point, aligned) };
                        if !started {
                            return Some(Command::with_params(STROKE, json!({"points": [point]})));
                        }
                    }
                    self.contact = touch.map(|touch| touch.id);
                    self.owner = Some((session.generation, session.active_tool.clone()));
                    self.sample(
                        point,
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
                    if global {
                        self.global_alignment = self.global_stroke_anchor.take();
                    } else if sampled && let Some(source) = self.clone_source.as_mut() {
                        source.aligned_destination = self.clone_stroke_destination;
                    }
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

    pub fn picked_source_params(&self, session: &Session) -> Option<serde_json::Value> {
        let source = self.clone_source.as_ref()?;
        let layer = source.layer(session)?;
        Some(json!({ "layer": layer.id, "point": layer_world(layer, source.fraction) }))
    }

    fn start_global_stroke(&mut self, session: &Session, point: [f32; 2], aligned: bool) -> bool {
        let Some(source) = session.clone_sources.selected() else { return false };
        let Some(layer) = clone_layer(session).and_then(|id| session.document.get_object(id)) else { return false };
        let Some(fraction) = layer_fraction(layer, point) else { return false };
        let source_id = (source.id, source.revision);
        let cached = self.global_alignment.as_ref().filter(|anchor| aligned
            && anchor.source == source_id && anchor.target.layer(session).is_some());
        let anchor = cached.cloned().unwrap_or(GlobalAlignment {
            source: source_id,
            target: CloneSource {
                context: session.context, document: session.document.instance, layer: layer.id, fraction,
                aligned_destination: None,
            },
        });
        let start = layer_world(layer, anchor.target.fraction);
        self.global_stroke_offset = Some([point[0] - start[0], point[1] - start[1]]);
        self.global_stroke_anchor = aligned.then_some(anchor);
        true
    }

    fn pick_clone_source(&mut self, session: &Session, point: [f32; 2]) -> bool {
        let Some(object) = clone_layer(session).and_then(|id| session.document.get_object(id)) else {
            return false;
        };
        let Some(fraction) = layer_fraction(object, point) else { return false };
        if !fraction.iter().all(|value| (0.0..1.0).contains(value)) {
            return false;
        }
        self.clone_source = Some(CloneSource {
            context: session.context, document: session.document.instance, layer: object.id, fraction, aligned_destination: None,
        });
        true
    }

    fn start_clone_stroke(&mut self, session: &Session, point: [f32; 2], aligned: bool) -> bool {
        let Some(source) = self.clone_source.as_ref() else { return false };
        let Some(layer) = source.layer(session) else { return false };
        let Some(destination) = layer_fraction(layer, point) else { return false };
        let anchor = if aligned { source.aligned_destination.unwrap_or(destination) } else { destination };
        let anchor = layer_world(layer, anchor);
        let offset = CloneTransform::current(session).sample_delta([point[0] - anchor[0], point[1] - anchor[1]]);
        let world = layer_world(layer, source.fraction);
        self.clone_stroke_start = Some([world[0] + offset[0], world[1] + offset[1]]);
        self.clone_stroke_destination = aligned.then(|| source.aligned_destination.unwrap_or(destination));
        true
    }

    fn paint_clone_source(&self, painter: &egui::Painter, rect: Rect, session: &Session) {
        if session.view.canvas_flags.preview_mode || using_global(session) { return; }
        let Some(source) = self.clone_source.as_ref() else { return };
        let Some(layer) = source.layer(session) else { return };
        let transform = CloneTransform::current(session);
        let offset_from = |sample: [f32; 2], start: [f32; 2], end: [f32; 2]| {
            let offset = transform.sample_delta([end[0] - start[0], end[1] - start[1]]);
            [sample[0] + offset[0], sample[1] + offset[1]]
        };
        let hover_source = || {
            let original = layer_world(layer, source.fraction);
            if session.tool_options.flag(&session.active_tool, CLONE_ALIGNED).unwrap_or(true)
                && let Some(anchor) = source.aligned_destination
                && let Some((point, ..)) = self.outline
            {
                let destination = dtool_core::view::canvas_to_doc(
                    &session.view, [rect.min.x, rect.min.y], [point.x, point.y],
                );
                offset_from(original, layer_world(layer, anchor), destination)
            } else {
                original
            }
        };
        let world = self.clone_stroke_start.zip(self.points.first().zip(self.points.last()))
            .map_or_else(hover_source, |(start, (first, last))| offset_from(start, first.point, last.point));
        let at = super::screen(rect, &session.view, world);
        if rect.contains(at) {
            for (width, colour) in [(3.5, super::CURSOR_EDGE), (1.5, super::CURSOR_FACE)] {
                let stroke = egui::Stroke::new(width, colour);
                for direction in [egui::vec2(6.0, 0.0), egui::vec2(0.0, 6.0)] {
                    painter.line_segment([at - direction, at + direction], stroke);
                }
            }
        }
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
        let Some(op) = dtool_core::commands::pixel_brush::operation(&session.active_tool) else { return };
        let painter = ui.painter_at(rect);
        if self.adjustment.active() && self.adjustment.paint(&painter, rect, session, settings(session, op)) {
            return;
        }
        if op.uses_sample_source() {
            self.paint_clone_source(&painter, rect, session);
        }
        self.paint_symmetry(&painter, rect, session);
        if let Some((point, radius, kind, rotation, aspect)) = self
            .outline
            .filter(|(point, ..)| rect.contains(*point) && ui.rect_contains_pointer(rect))
        {
            for (width, colour) in [(3.2, super::CURSOR_EDGE), (2.2, super::CURSOR_FACE)] {
                let stroke = egui::Stroke::new(width, colour);
                let rotate = egui::emath::Rot2::from_angle((rotation + session.view.rotation_deg).to_radians());
                adjustment::draw_tip(&painter, point, radius, aspect, rotate, kind, stroke);
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
