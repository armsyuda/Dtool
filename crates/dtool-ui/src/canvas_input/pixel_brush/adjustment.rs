use dtool_core::Session;
use dtool_core::brush::{BrushSettings, TipKind};
use dtool_core::commands::pixel_presets::{TipAdjustment, adjust_tip};
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui, Vec2};

#[derive(Clone, Copy, Debug, Default)]
enum Mode {
    #[default]
    Size,
    Shape,
    Rotation,
}

impl Mode {
    fn next(self) -> Self {
        match self {
            Self::Size => Self::Shape,
            Self::Shape => Self::Rotation,
            Self::Rotation => Self::Size,
        }
    }

    fn values(self, brush: &BrushSettings) -> TipAdjustment {
        match self {
            Self::Size => TipAdjustment::SizeHardness { size_px: brush.size_px, hardness: brush.hardness },
            Self::Shape => TipAdjustment::ShapeSpacing { roundness: brush.roundness, spacing: brush.spacing },
            Self::Rotation => TipAdjustment::Rotation(brush.rotation),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    start: Pos2,
    initial: TipAdjustment,
    current: TipAdjustment,
}

impl Drag {
    fn update(&mut self, centre: Pos2, pointer: Pos2, zoom: f32, view_angle: f32) {
        let delta = pointer - self.start;
        self.current = match self.initial {
            TipAdjustment::SizeHardness { size_px, hardness } => TipAdjustment::SizeHardness {
                size_px: (size_px + 2.0 * delta.x / zoom.max(0.01)).clamp(1.0, 2048.0),
                hardness: (hardness - delta.y * 0.005).clamp(0.0, 1.0),
            },
            TipAdjustment::ShapeSpacing { roundness, spacing } => TipAdjustment::ShapeSpacing {
                roundness: (roundness + delta.x * 0.005).clamp(0.01, 1.0),
                spacing: (spacing - delta.y * 0.005).clamp(0.01, 9.99),
            },
            TipAdjustment::Rotation(_) => {
                let radial = pointer - centre;
                if delta.length_sq() < 4.0 || radial.length_sq() < 4.0 { return; }
                let angle = radial.y.atan2(radial.x).to_degrees() - view_angle;
                TipAdjustment::Rotation((angle + 180.0).rem_euclid(360.0) - 180.0)
            }
        };
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Adjustment {
    centre: Option<Pos2>,
    next: Mode,
    drag: Option<Drag>,
    shown: Option<TipAdjustment>,
    suppress_until_up: bool,
}

fn chord(modifiers: egui::Modifiers) -> bool {
    modifiers.ctrl && modifiers.alt
}

impl Adjustment {
    pub(super) fn active(&self) -> bool {
        self.centre.is_some() || self.suppress_until_up
    }

    pub(super) fn input(&mut self, ui: &Ui, response: &Response, session: &mut Session, brush: &BrushSettings) -> bool {
        if !self.active() && !ui.input(|input| chord(input.modifiers) || input.events.iter().any(|event| {
            matches!(event, egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, modifiers, .. } if chord(*modifiers))
        })) {
            return false;
        }
        let (held, primary, escape, focused, events) = ui.input(|i| (
            chord(i.modifiers), i.pointer.primary_down(), i.key_pressed(egui::Key::Escape), i.focused, i.events.clone(),
        ));
        if !focused || events.iter().any(|event| matches!(event, egui::Event::PointerGone)) {
            let active = self.active();
            *self = Self::default();
            return active;
        }
        if escape && self.active() {
            *self = Self { suppress_until_up: primary || held, ..Default::default() };
            ui.ctx().request_repaint();
            return true;
        }
        if self.suppress_until_up {
            self.suppress_until_up = primary || held;
            return true;
        }
        let zoom = dtool_core::units::from_px(1.0, session.document.units, session.document.dpi)
            .unwrap_or(1.0) * session.view.zoom;
        let mut consumed = self.active() || (held && response.hovered());
        for event in events {
            match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers }
                    if chord(modifiers) && response.enabled() && response.interact_rect.contains(pos)
                        && ui.ctx().layer_id_at(pos) == Some(response.layer_id) && !egui::Popup::is_any_open(ui.ctx()) =>
                {
                    self.centre.get_or_insert(pos);
                    let value = self.next.values(brush);
                    self.drag = Some(Drag { start: pos, initial: value, current: value });
                    self.shown = Some(value);
                    self.next = self.next.next();
                    consumed = true;
                    response.request_focus();
                }
                egui::Event::PointerMoved(pos) if self.drag.is_some() => {
                    if let (Some(centre), Some(drag)) = (self.centre, &mut self.drag) {
                        drag.update(centre, pos, zoom, session.view.rotation_deg);
                        self.shown = Some(drag.current);
                    }
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => {
                    if let (Some(centre), Some(mut drag)) = (self.centre, self.drag.take()) {
                        drag.update(centre, pos, zoom, session.view.rotation_deg);
                        self.shown = Some(drag.current);
                        if drag.current != drag.initial { adjust_tip(session, drag.current); }
                    }
                }
                _ => {}
            }
        }
        if !held {
            if let Some(drag) = self.drag.take().filter(|drag| drag.current != drag.initial) {
                adjust_tip(session, drag.current);
            }
            *self = Self { suppress_until_up: consumed && primary, ..Default::default() };
        }
        if consumed {
            let cursor = if self.shown.is_some() { egui::CursorIcon::None } else { egui::CursorIcon::Crosshair };
            ui.ctx().set_cursor_icon(cursor);
            if ui.input(|input| !input.events.is_empty()) { ui.ctx().request_repaint(); }
        }
        consumed
    }

    pub(super) fn paint(&self, painter: &egui::Painter, rect: Rect, session: &Session, mut brush: BrushSettings) -> bool {
        let (Some(centre), Some(shown)) = (self.centre, self.shown) else { return false; };
        let labels = match shown {
            TipAdjustment::SizeHardness { size_px, hardness } => {
                brush.size_px = size_px;
                brush.hardness = hardness;
                format!("{} {:.1} px\n{} {:.0} %", dtool_strings::text("brush.adjust.size"), size_px,
                    dtool_strings::text("context.brush.hardness"), hardness * 100.0)
            }
            TipAdjustment::ShapeSpacing { roundness, spacing } => {
                brush.roundness = roundness;
                brush.spacing = spacing;
                format!("{} {:.0} %\n{} {:.0} %", dtool_strings::text("brush_editor.shape"), roundness * 100.0,
                    dtool_strings::text("brush_editor.spacing"), spacing * 100.0)
            }
            TipAdjustment::Rotation(rotation) => {
                brush.rotation = rotation;
                format!("{} {:.0}°", dtool_strings::text("brush_editor.rotation"), rotation)
            }
        };
        let size = dtool_core::units::from_px(brush.size_px, session.document.units, session.document.dpi)
            .unwrap_or(brush.size_px) * session.view.zoom;
        let radius = (size * 0.5).max(2.0);
        let angle = (brush.rotation + session.view.rotation_deg).to_radians();
        let rotate = egui::emath::Rot2::from_angle(angle);
        let stroke = Stroke::new(1.0, crate::canvas_input::SELECTION_BLUE);
        let aspect = super::tip_aspect(&brush);
        let outline = |centre, scale| draw_tip(painter, centre, radius * scale, aspect, rotate, brush.tip_kind, stroke);
        match shown {
            TipAdjustment::SizeHardness { .. } => {
                outline(centre, 1.0);
                if brush.stamp.is_none() && (0.0..1.0).contains(&brush.hardness) && brush.hardness > 0.0 {
                    outline(centre, brush.hardness);
                }
            }
            TipAdjustment::ShapeSpacing { .. } => {
                let spacing = rotate * egui::vec2(size * brush.spacing, 0.0);
                for offset in [-spacing, Vec2::ZERO, spacing] { outline(centre + offset, 1.0); }
            }
            TipAdjustment::Rotation(_) => {
                outline(centre, 1.0);
                painter.line_segment([centre, centre + rotate * egui::vec2(radius * aspect[0], 0.0)], stroke);
            }
        }

        let font = egui::TextStyle::Small.resolve(painter.ctx().global_style().as_ref());
        let galley = painter.layout_no_wrap(labels, font, Color32::from_gray(228));
        let pad = egui::vec2(8.0, 4.0);
        let bounds = galley.size() + pad * 2.0;
        let preferred = centre + egui::vec2(16.0, -bounds.y);
        let min = egui::pos2(
            preferred.x.clamp(rect.left(), (rect.right() - bounds.x).max(rect.left())),
            preferred.y.clamp(rect.top(), (rect.bottom() - bounds.y).max(rect.top())),
        );
        painter.rect_filled(Rect::from_min_size(min, bounds), 4.0, Color32::from_gray(33));
        painter.galley(min + pad, galley, Color32::from_gray(228));
        true
    }
}

pub(super) fn draw_tip(
    painter: &egui::Painter, centre: Pos2, radius: f32, aspect: [f32; 2],
    rotate: egui::emath::Rot2, kind: TipKind, stroke: Stroke,
) {
    let [rx, ry] = aspect.map(|side| side * radius);
    if kind == TipKind::Round && rx == ry {
        painter.circle_stroke(centre, rx, stroke);
        return;
    }
    let points = if kind == TipKind::Round {
        (0..64).map(|step| {
            let (sin, cos) = (std::f32::consts::TAU * step as f32 / 64.0).sin_cos();
            centre + rotate * egui::vec2(cos * rx, sin * ry)
        }).collect()
    } else {
        [[-rx, -ry], [rx, -ry], [rx, ry], [-rx, ry]].into_iter()
            .map(|[x, y]| centre + rotate * egui::vec2(x, y)).collect()
    };
    painter.add(egui::Shape::closed_line(points, stroke));
}
