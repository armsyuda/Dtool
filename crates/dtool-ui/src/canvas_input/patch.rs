use dtool_core::commands::patch::{self, Area, Options, Prepared};
use dtool_core::{Command, Session};
use egui::{Pos2, Rect, Response, Stroke, Ui};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub(super) struct Gesture {
    points: Vec<[f32; 2]>,
    drawing: bool,
    ready: Option<Prepared>,
    start: Option<[f32; 2]>,
    owner: Option<(dtool_core::document::DocInstance, u64, dtool_core::ObjectId)>,
    delta: [f32; 2],
    offset: [i64; 2],
    options: Option<Options>,
    revision: u64,
    preview: Option<super::PixelBrushDraft>,
}

impl Gesture {
    pub fn active(&self) -> bool { self.ready.is_some() || self.drawing }

    pub fn cancel(&mut self) {
        let revision = self.revision;
        *self = Self { revision, ..Default::default() };
    }

    fn add_point(&mut self, point: [f32; 2]) {
        if self.points.last() == Some(&point) { return; }
        if self.points.len() < patch::MAX_POINTS {
            self.points.push(point);
        } else if let Some(last) = self.points.last_mut() {
            *last = point;
        }
    }

    fn owner(session: &Session) -> Option<(dtool_core::document::DocInstance, u64, dtool_core::ObjectId)> {
        Some((session.document.instance, session.generation, dtool_core::commands::pixel_brush::clone_layer(session)?))
    }

    pub fn draft(&self, session: &Session) -> Option<super::PixelBrushDraft> {
        if session.active_tool == patch::TOOL && self.owner == Self::owner(session) {
            return self.preview.clone();
        }
        None
    }

    pub fn input(&mut self, ui: &Ui, response: &Response, session: &Session, to_doc: impl Fn(Pos2) -> [f32; 2]) -> Option<Command> {
        let owner = Self::owner(session);
        if self.owner != owner { self.cancel(); }
        if response.hovered() {
            ui.ctx().set_cursor_icon(if owner.is_some() { egui::CursorIcon::Crosshair } else { egui::CursorIcon::NotAllowed });
        }
        owner?;
        let (down, up) = crate::input::primary_button_positions(ui);
        if let Some(pos) = down.filter(|pos| {
            response.enabled() && response.interact_rect.contains(*pos)
                && ui.ctx().layer_id_at(*pos) == Some(response.layer_id) && !egui::Popup::is_any_open(ui.ctx())
        }) {
            let point = to_doc(pos);
            response.request_focus();
            self.owner = owner;
            if self.ready.is_none() && session.document.pixel_selection.is_some() && self.points.is_empty() {
                self.ready = Prepared::new(session, None).ok();
            }
            if self.ready.as_ref().is_some_and(|ready| ready.contains(point)) {
                self.start = Some(point);
            } else {
                self.points.clear();
                self.points.push(point);
                self.ready = None;
                self.drawing = true;
            }
        }
        let point = up.or_else(|| ui.input(|input| input.pointer.latest_pos())).map(&to_doc);
        if self.drawing {
            ui.input(|input| {
                let mut drawing = down.is_none();
                for event in &input.events {
                    match event {
                        egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed, .. } => {
                            if !pressed { break; }
                            drawing = true;
                        }
                        egui::Event::PointerMoved(pos) if drawing => self.add_point(to_doc(*pos)),
                        _ => {}
                    }
                }
            });
            if let Some(release) = up {
                self.add_point(to_doc(release));
                self.drawing = false;
                self.ready = Prepared::new(session, Some(&Area::Polygon(self.points.clone()))).ok();
                if self.ready.is_none() { self.points.clear(); }
            }
            ui.ctx().request_repaint();
        }
        if let Some((start, point, ready)) = self.start.zip(point).zip(self.ready.as_ref()).map(|((a, b), c)| (a, b, c)) {
            self.delta = [point[0] - start[0], point[1] - start[1]];
            let offset = ready.offset(self.delta).unwrap_or_default();
            let options = Options::current(session);
            if up.is_some() {
                self.start = None;
                self.preview = None;
                self.offset = [0; 2];
                if offset != [0; 2] {
                    let area = (!self.points.is_empty()).then(|| Area::Polygon(std::mem::take(&mut self.points)));
                    let delta = self.delta;
                    self.cancel();
                    ui.ctx().request_repaint();
                    return Some(Command::with_params(patch::APPLY, json!({"area": area, "offset": delta})));
                }
            } else if self.offset != offset || self.options != Some(options) {
                self.offset = offset;
                self.options = Some(options);
                self.revision = self.revision.wrapping_add(1);
                self.preview = ready.document(session, offset, options, true).map(|document| super::PixelBrushDraft {
                    revision: self.revision,
                    document: Arc::new(document),
                    layer: Some(ready.layer()),
                });
                ui.ctx().request_repaint();
            }
        }
        if up.is_none() && (self.drawing || self.start.is_some()) && !ui.input(|input| input.pointer.primary_down()) {
            self.cancel();
            ui.ctx().request_repaint();
        }
        None
    }

    pub fn paint(&self, ui: &Ui, rect: Rect, session: &Session) {
        if self.owner.is_none() || self.owner != Self::owner(session) { return; }
        let painter = ui.painter_at(rect);
        let screen = |point| super::screen(rect, &session.view, point);
        let stroke = Stroke::new(1.4, dtool_design::color("selection.ants"));
        if self.drawing && self.points.len() > 1 {
            painter.add(egui::Shape::line(self.points.iter().copied().map(screen).collect(), stroke));
        } else if let Some(ready) = &self.ready {
            if !self.points.is_empty() {
                painter.add(egui::Shape::closed_line(self.points.iter().copied().map(screen).collect(), stroke));
            }
            if self.start.is_some() && self.offset != [0; 2] {
                let stroke = Stroke::new(1.2, dtool_design::color("dialog.choice_border"));
                let mut contour = if self.points.is_empty() {
                    ready.outline(self.offset).map(screen).to_vec()
                } else {
                    let offset = ready.world_offset(self.offset);
                    self.points.iter().map(|point| screen([point[0] + offset[0], point[1] + offset[1]])).collect()
                };
                contour.push(contour[0]);
                painter.extend(egui::Shape::dashed_line(&contour, stroke, 5.0, 3.0));
            }
        }
    }
}
