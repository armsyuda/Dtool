use dtool_core::curve::{Contour, Curve, PathPoint};
use dtool_core::{Session, scene::Scene};
use egui::{Painter, Pos2, Stroke};

use crate::canvas_curves::{NodeEdit, p32};

const MAX_TEETH: usize = 24_000;
const DISPLAY_LENGTH: f32 = 44.0;

struct Tooth {
    base: Pos2,
    normal: egui::Vec2,
    curvature: f64,
}

fn sample_segment(a: &PathPoint, b: &PathPoint, t: f64, to_screen: &impl Fn([f32; 2]) -> Pos2) -> Option<Tooth> {
    let [p0, p1, p2, p3] = [a.pos, a.handle_out.unwrap_or(a.pos), b.handle_in.unwrap_or(b.pos), b.pos];
    let s = 1.0 - t;
    let velocity: [f64; 2] = std::array::from_fn(|i| {
        3.0 * (s * s * (p1[i] - p0[i]) + 2.0 * s * t * (p2[i] - p1[i]) + t * t * (p3[i] - p2[i]))
    });
    let speed = velocity[0].hypot(velocity[1]);
    if speed <= 1e-8 {
        return None;
    }
    let acceleration: [f64; 2] = std::array::from_fn(|i| {
        6.0 * (s * ((p2[i] - p1[i]) - (p1[i] - p0[i])) + t * ((p3[i] - p2[i]) - (p2[i] - p1[i])))
    });
    let curvature = (velocity[0] * acceleration[1] - velocity[1] * acceleration[0]) / speed.powi(3);
    if !curvature.is_finite() {
        return None;
    }
    let point = std::array::from_fn(|i| {
        s.powi(3) * p0[i] + 3.0 * s * s * t * p1[i] + 3.0 * s * t * t * p2[i] + t.powi(3) * p3[i]
    });
    let base = to_screen(p32(point));
    let normal = egui::vec2((-velocity[1] / speed) as f32, (velocity[0] / speed) as f32);
    (base.is_finite() && normal.is_finite()).then_some(Tooth { base, normal, curvature })
}

fn sample_contour(
    contour: &Contour, to_screen: &impl Fn([f32; 2]) -> Pos2, segments: &mut Vec<Vec<Tooth>>, budget: &mut usize,
) {
    let count = contour.points.len();
    for i in 0..count.saturating_sub(usize::from(!contour.closed)) {
        if *budget < 3 {
            return;
        }
        let a = &contour.points[i];
        let b = &contour.points[(i + 1) % count];
        if a.handle_out.is_none() && b.handle_in.is_none() {
            continue;
        }
        let bounds = [a.pos, a.handle_out.unwrap_or(a.pos), b.handle_in.unwrap_or(b.pos), b.pos];
        let screen = bounds.map(|point| to_screen(p32(point)));
        if !screen.iter().all(|p| p.is_finite()) {
            continue;
        }
        let length: f32 = screen.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
        let intervals = ((length / 11.0).ceil() as usize).clamp(4, 32).min(*budget - 1);
        *budget -= intervals + 1;
        let mut teeth = Vec::with_capacity(intervals + 1);
        for j in 0..=intervals {
            if let Some(tooth) = sample_segment(a, b, j as f64 / intervals as f64, to_screen) {
                teeth.push(tooth);
            } else if !teeth.is_empty() {
                segments.push(std::mem::take(&mut teeth));
            }
        }
        if !teeth.is_empty() {
            segments.push(teeth);
        }
    }
}

pub(super) fn paint(
    painter: &Painter, session: &Session, scene: &Scene, nodes: &NodeEdit,
    to_screen: impl Fn([f32; 2]) -> Pos2, colour: egui::Color32,
) {
    let mut segments = Vec::new();
    let mut budget = MAX_TEETH;
    for &id in &session.selection.ids {
        if budget < 3 {
            break;
        }
        let Some(object) = session.document.get_object(id).filter(|object| object.curve().is_some())
            .filter(|_| scene.quad(id).is_some()) else { continue };
        if nodes.target == Some(id) && let Some(live) = &nodes.live {
            for contour in live {
                sample_contour(&dtool_core::outline::cornered_contour(contour), &to_screen, &mut segments, &mut budget);
            }
        } else if let Some(path) = dtool_core::outline::object_outline_world(object) {
            let curve = Curve::from_kurbo(&path);
            for contour in std::iter::once(&curve.contour).chain(&curve.holes) {
                sample_contour(contour, &to_screen, &mut segments, &mut budget);
            }
        }
    }
    let peak = segments.iter().flatten().map(|tooth| tooth.curvature.abs()).fold(0.0, f64::max);
    if peak <= 1e-12 {
        return;
    }
    let spoke = Stroke::new(0.75, colour.gamma_multiply(0.60));
    let edge = Stroke::new(1.25, colour);
    for segment in segments {
        if !segment.iter().any(|tooth| tooth.curvature.abs() > 1e-12) {
            continue;
        }
        let tips: Vec<_> = segment.into_iter().map(|tooth| {
            let tip = tooth.base + tooth.normal * ((tooth.curvature / peak) as f32 * DISPLAY_LENGTH);
            if (tip - tooth.base).length_sq() >= 0.16 {
                painter.line_segment([tooth.base, tip], spoke);
            }
            tip
        }).collect();
        if tips.len() >= 2 {
            painter.add(egui::Shape::line(tips, edge));
        }
    }
}
