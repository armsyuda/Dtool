use crate::shape::{SpiralRecipe, SpiralStyle};
use kurbo::{Affine, Arc, BezPath, Point, Shape, Vec2};
use std::f64::consts::{FRAC_PI_2, TAU};

pub(super) fn outline(recipe: &SpiralRecipe, width: f64, height: f64) -> Option<BezPath> {
    let turns = f64::from(recipe.turns).clamp(0.0, 32.0);
    let path = match recipe.style {
        SpiralStyle::Linear => {
            let fraction = f64::from(recipe.part_turn).clamp(0.0, 1.0);
            let turns = (turns - turns.min(1.0) * (1.0 - fraction)).max(0.02);
            polar(turns, recipe.cusped, false, |t| t)
        }
        SpiralStyle::Decaying => {
            let rate = (-f64::from(recipe.decay_per_turn).clamp(0.05, 20.0).ln() * turns).clamp(-40.0, 40.0);
            polar(turns, recipe.cusped, false, |t| {
                if rate.abs() < 1e-5 { t } else { (t * rate).exp_m1() / rate.exp_m1() }
            })
        }
        SpiralStyle::SemiCircular | SpiralStyle::CounterSemiCircular => {
            let step = f64::from(recipe.segment_angle.abs()).clamp(15.0, 180.0).to_radians();
            let divisions = f64::from(recipe.divisions.clamp(1, 256));
            let decay = f64::from(recipe.decay_per_segment).clamp(0.05, 20.0).ln();
            let direction = if recipe.style == SpiralStyle::CounterSemiCircular { -1.0 } else { 1.0 };
            arcs(turns, step, direction, recipe.cusped, |index| {
                (1.0 + index / divisions) * (-decay * index / divisions).clamp(-20.0, 20.0).exp()
            })
        }
        SpiralStyle::Fibonacci => {
            const PHI: f64 = 1.618_033_988_749_895;
            let inner = f64::from(recipe.inner_radius).clamp(0.0, 100.0) / 100.0;
            let growth = 1.0 - f64::from(recipe.choke).clamp(0.0, 100.0) * 0.009;
            let steps = (turns * 4.0).ceil();
            arcs(turns, FRAC_PI_2, 1.0, recipe.cusped, |index| {
                inner + PHI.powf((index - steps) * growth)
            })
        }
        SpiralStyle::Plotted => {
            let bias = 2.0_f64.powf(f64::from(recipe.bias).clamp(-100.0, 100.0) / 100.0);
            let cap = f64::from(recipe.cap_choke).clamp(0.0, 100.0) / 100.0;
            polar(turns, recipe.cusped, true, |t| {
                t.powf(bias) * (1.0 - cap * t.powi(4) * 0.20)
            })
        }
    };
    let bounds = path.bounding_box();
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return None;
    }
    let fit = Affine::scale_non_uniform(width / bounds.width(), height / bounds.height())
        * Affine::translate((-bounds.x0, -bounds.y0));
    let result = fit * path;
    Some(if recipe.flip {
        Affine::translate((width, 0.0)) * Affine::scale_non_uniform(-1.0, 1.0) * result
    } else {
        result
    })
}

fn polar(turns: f64, cusped: bool, plotted: bool, radius: impl Fn(f64) -> f64) -> BezPath {
    let resolution = match (plotted, cusped) {
        (true, true) => 4.0,
        (true, false) => 6.0,
        (false, true) => 8.0,
        (false, false) => 16.0,
    };
    let vertices = (turns.max(0.02) * resolution).ceil().max(3.0) as usize;
    let points: Vec<_> = (0..=vertices).map(|index| {
        let t = index as f64 / vertices as f64;
        let angle = t * turns.max(0.02) * TAU;
        let radius = radius(t);
        Point::new(radius * angle.cos(), radius * angle.sin())
    }).collect();
    let mut path = BezPath::new();
    path.move_to(points[0]);
    for index in 0..vertices {
        let (start, end) = (points[index], points[index + 1]);
        if cusped || plotted {
            path.line_to(end);
        } else {
            let before = points[index.saturating_sub(1)];
            let after = points[(index + 2).min(vertices)];
            path.curve_to(start + (end - before) / 6.0, end - (after - start) / 6.0, end);
        }
    }
    path
}

fn arcs(turns: f64, step: f64, direction: f64, cusped: bool, radius: impl Fn(f64) -> f64) -> BezPath {
    let mut path = BezPath::new();
    let mut point = Point::ORIGIN;
    let mut heading = 0.0_f64;
    let mut remaining = turns.max(0.02) * TAU;
    let mut index = 0.0;
    path.move_to(point);
    while remaining > 1e-10 {
        let span = remaining.min(step);
        let sweep = span * direction;
        let radius = radius(index);
        let centre = point + Vec2::new(-heading.sin(), heading.cos()) * (radius * direction);
        let start = heading - direction * FRAC_PI_2;
        let end = start + sweep;
        let arc = Arc::new(centre, Vec2::splat(radius), start, sweep, 0.0);
        path.extend(arc.append_iter(radius * 0.0001));
        point = centre + Vec2::new(end.cos(), end.sin()) * radius;
        heading += if cusped { sweep + 0.08 * step * direction } else { sweep };
        remaining -= span;
        index += 1.0;
    }
    path
}
