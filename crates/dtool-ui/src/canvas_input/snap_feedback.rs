use super::{screen, CanvasInput, GestureKind, Handle};
use dtool_core::{Session, document::Units};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, vec2};

fn guide_colour(axis: usize) -> Color32 {
    if axis == 0 { Color32::from_rgb(0, 152, 0) } else { Color32::from_rgb(212, 0, 0) }
}

fn distance_text(distance: f32, units: Units) -> String {
    let suffix = match units {
        Units::Px => "px",
        Units::Pt => "pt",
        Units::Mm => "mm",
        Units::Cm => "cm",
        Units::In => "in",
    };
    let value = format!("{distance:.2}");
    format!("{} {suffix}", value.trim_end_matches('0').trim_end_matches('.'))
}

impl CanvasInput {
    pub(super) fn paint_snap_feedback(&self, rect: Rect, session: &Session, painter: &Painter) {
        let prefs = session.snap_prefs.query();
        if !self.move_tool.dragging || !prefs.enabled || !prefs.dynamic_guides {
            return;
        }
        let width = 1.0 / painter.ctx().pixels_per_point();
        let project = |point| screen(rect, &session.view, point);
        for guide in &self.guide_lines {
            let axis = usize::from(guide.from[0] != guide.to[0]);
            painter.line_segment([project(guide.from), project(guide.to)], Stroke::new(width, guide_colour(axis)));
        }
        let extent = rect.size().length();
        for (axis, &position) in self.snap_axes.iter().enumerate() {
            let Some(position) = position else { continue };
            if self.guide_lines.iter().any(|guide| {
                guide.from[axis] == guide.to[axis]
                    && (guide.from[axis] - position).abs() <= crate::canvas_tools::GUIDE_EPSILON
            }) {
                continue;
            }
            let mut point = [0.0; 2];
            point[axis] = position;
            let a = project(point);
            point[1 - axis] = 1.0;
            let direction = (project(point) - a).normalized();
            let centre = a + direction * (rect.center() - a).dot(direction);
            let colour = guide_colour(axis);
            painter.line_segment([centre - direction * extent, centre + direction * extent], Stroke::new(width, colour));
            if matches!(self.move_tool.gesture, Some(GestureKind::Scale(handle)) if handle != Handle::FrameTextScale)
                && let Some(point) = self.snap_mark
            {
                painter.circle_filled(project(point), 2.4, colour);
            }
        }
        for guide in &self.gap_guides {
            let axis = usize::from(guide.line.from[0] != guide.line.to[0]);
            paint_gap(painter, project(guide.line.from), project(guide.line.to), Stroke::new(width, guide_colour(axis)),
                &distance_text((guide.line.to[1 - axis] - guide.line.from[1 - axis]).abs(), session.document.units),
                guide.matched);
        }
    }
}

fn paint_gap(painter: &Painter, a: Pos2, b: Pos2, stroke: Stroke, label: &str, matched: bool) {
    let vector = b - a;
    let length = vector.length();
    if length < 7.0 {
        return;
    }
    let direction = vector / length;
    let normal = vec2(-direction.y, direction.x);
    let tick = normal * 3.0;
    painter.line_segment([a, b], stroke);
    for (end, outward) in [(a, direction), (b, -direction)] {
        painter.line_segment([end - tick, end + tick], stroke);
        if matched && length >= 18.0 {
            let foot = end + outward * 4.0;
            painter.add(egui::Shape::line(vec![foot - normal * 2.0, end, foot + normal * 2.0], stroke));
        }
    }
    let center = a.lerp(b, 0.5) + normal * 7.0;
    let font = FontId::proportional(8.5);
    let galley = painter.layout_no_wrap(label.to_owned(), font, stroke.color);
    let label_rect = Align2::CENTER_CENTER.anchor_size(center, galley.size());
    painter.rect_filled(label_rect.expand2(vec2(2.0, 0.5)), 1.0, Color32::from_white_alpha(240));
    painter.galley(label_rect.min, galley, stroke.color);
}
