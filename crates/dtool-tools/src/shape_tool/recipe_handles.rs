use dtool_core::shape::{ShapeKind, ShapeRecipe};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

pub(super) struct Control {
    pub field: &'static str,
    value: f32,
    range: [f32; 2],
    axis: Axis,
}

enum Axis {
    Linear { zero: [f32; 2], unit: [f32; 2] },
    Angle { radius: f32 },
}

impl Control {
    fn linear(
        field: &'static str,
        value: f32,
        range: [f32; 2],
        zero: [f32; 2],
        unit: [f32; 2],
    ) -> Self {
        Self {
            field,
            value,
            range,
            axis: Axis::Linear { zero, unit },
        }
    }

    fn radial(
        field: &'static str,
        value: f32,
        range: [f32; 2],
        angle: f32,
        origin: f32,
        scale: f32,
    ) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::linear(
            field,
            value,
            range,
            [0.5 + cos * origin, 0.5 + sin * origin],
            [cos * scale, sin * scale],
        )
    }

    fn angle(field: &'static str, value: f32, radius: f32) -> Self {
        Self {
            field,
            value,
            range: [-360.0, 360.0],
            axis: Axis::Angle { radius },
        }
    }

    pub fn position(&self) -> [f32; 2] {
        let value = self.value.clamp(self.range[0], self.range[1]);
        match self.axis {
            Axis::Linear {
                zero: [x, y],
                unit: [dx, dy],
            } => [x + dx * value, y + dy * value],
            Axis::Angle { radius } => {
                let (sin, cos) = self.value.to_radians().sin_cos();
                [0.5 + radius * cos, 0.5 + radius * sin]
            }
        }
    }

    pub fn value_at(&self, [x, y]: [f32; 2]) -> Option<f32> {
        let value = match self.axis {
            Axis::Linear {
                zero: [ox, oy],
                unit: [dx, dy],
            } => ((x - ox) * dx + (y - oy) * dy) / (dx * dx + dy * dy),
            Axis::Angle { .. } => {
                let (dx, dy) = (x - 0.5, y - 0.5);
                if dx.hypot(dy) < 1e-4 {
                    return None;
                }
                let angle = dy.atan2(dx).to_degrees();
                let nearest = angle + 360.0 * ((self.value - angle) / 360.0).round();
                if nearest > 360.0 {
                    nearest - 360.0
                } else if nearest < -360.0 {
                    nearest + 360.0
                } else {
                    nearest
                }
            }
        };
        if !value.is_finite() {
            return None;
        }
        let value = value.clamp(self.range[0], self.range[1]);
        ((value - self.value).abs() > 1e-3).then_some(value)
    }
}

const PERCENT: [f32; 2] = [0.0, 100.0];

pub(super) fn controls(recipe: &ShapeRecipe, [w, h]: [f32; 2]) -> Option<Vec<Control>> {
    use ShapeKind::*;
    let linear = |field, value, zero, unit| Control::linear(field, value, PERCENT, zero, unit);
    let inner = |value, outer: f32, angle| {
        Control::radial(
            "inner_radius",
            value,
            [0.0, (outer - 0.01).max(0.0)],
            angle,
            0.0,
            0.005,
        )
    };
    let control = match recipe.kind {
        Star => {
            let star = recipe.star.as_ref()?;
            let angle = -FRAC_PI_2 + PI / star.number_of_points.max(3) as f32;
            inner(
                star.inner_radius,
                star.outer_radius.clamp(0.01, 100.0),
                angle,
            )
        }
        Arrow => {
            let arrow = recipe.arrow.as_ref()?;
            let base = 1.0 - arrow.head_length.clamp(0.0, 100.0) * 0.01;
            return Some(vec![
                linear(
                    "head_length",
                    arrow.head_length,
                    [1.0, 0.5 + arrow.head_width.clamp(0.0, 50.0) * 0.01],
                    [-0.01, 0.0],
                ),
                Control::linear(
                    "head_width",
                    arrow.head_width,
                    [0.0, 50.0],
                    [base, 0.5],
                    [0.0, -0.01],
                ),
            ]);
        }
        CalloutEllipse => {
            let callout = recipe.callout_ellipse.as_ref()?;
            let (sin, cos) = (FRAC_PI_2 - callout.pointer_angle.to_radians()).sin_cos();
            let short = w.min(h);
            linear(
                "pointer_length",
                callout.pointer_length,
                [
                    0.5 + cos * callout.radius_x.clamp(0.0, 50.0) * 0.01,
                    0.5 + sin * callout.radius_y.clamp(0.0, 50.0) * 0.01,
                ],
                [0.01 * cos * short / w, 0.01 * sin * short / h],
            )
        }
        CalloutRectangle => {
            let callout = recipe.callout_rectangle.as_ref()?;
            let (sin, cos) = callout.pointer_angle.to_radians().sin_cos();
            linear(
                "pointer_length",
                callout.pointer_length,
                [0.5, 1.0],
                [0.01 * sin * h / w, 0.01 * cos],
            )
        }
        Donut => {
            let donut = recipe.donut.as_ref()?;
            let outer = donut.outer_radius.clamp(0.01, 100.0);
            return Some(vec![
                inner(donut.inner_radius, outer, 0.0),
                Control::radial(
                    "outer_radius",
                    outer,
                    [(donut.inner_radius + 0.01).clamp(0.01, 100.0), 100.0],
                    -FRAC_PI_2,
                    0.0,
                    0.005,
                ),
            ]);
        }
        Pie => {
            let pie = recipe.pie.as_ref()?;
            let outer = pie.outer_radius.clamp(0.01, 100.0);
            let sweep = (pie.end_angle - pie.start_angle).rem_euclid(360.0);
            let half_sweep = if sweep == 0.0 { 180.0 } else { sweep * 0.5 };
            return Some(vec![
                inner(
                    pie.inner_radius,
                    outer,
                    (pie.start_angle + half_sweep).to_radians(),
                ),
                Control::angle("start_angle", pie.start_angle, outer * 0.004),
                Control::angle("end_angle", pie.end_angle, outer * 0.005),
            ]);
        }
        Heart => linear(
            "indent",
            recipe.heart.as_ref()?.indent,
            [0.5, 0.08],
            [0.0, 0.0036],
        ),
        Cog => {
            let cog = recipe.cog.as_ref()?;
            let angle = -FRAC_PI_2
                + PI * cog.tooth_width.clamp(0.0, 100.0) * 0.01 / cog.number_of_teeth.max(3) as f32;
            Control::radial(
                "inner_radius",
                cog.inner_radius,
                [0.0, (cog.outer_radius.clamp(0.01, 50.0) - 0.01).max(0.0)],
                angle,
                0.0,
                0.01,
            )
        }
        Tear => linear(
            "indent",
            recipe.tear.as_ref()?.indent,
            [1.0, 0.28],
            [0.0, 0.0044],
        ),
        Crescent => linear(
            "indent",
            recipe.crescent.as_ref()?.indent,
            [0.5, 0.5],
            [-0.005, 0.0],
        ),
        Cloud => Control::radial(
            "indent",
            recipe.cloud.as_ref()?.indent,
            PERCENT,
            -FRAC_PI_2 - PI / 12.0,
            0.5,
            -0.0016,
        ),
        Segment => {
            let segment = recipe.segment.as_ref()?;
            let middle = 0.40 + 0.002 * segment.pointiness.clamp(0.0, 100.0);
            linear(
                "indent",
                segment.indent,
                [0.5, middle],
                [0.0, 0.0094 * middle],
            )
        }
        Cat => linear(
            "indent",
            recipe.cat.as_ref()?.indent,
            [0.30, 0.13],
            [0.0, 0.0017],
        ),
        SquareStar => {
            let star = recipe.square_star.as_ref()?;
            Control::radial(
                "cutout",
                star.cutout,
                PERCENT,
                -FRAC_PI_2 + PI / star.number_of_sides.max(3) as f32,
                0.5,
                -0.005,
            )
        }
        DoubleStar => {
            let star = recipe.double_star.as_ref()?;
            Control::radial(
                "inner_radius",
                star.inner_radius,
                PERCENT,
                -FRAC_PI_2 + FRAC_PI_2 / star.number_of_points.clamp(3, 1024) as f32,
                0.0,
                0.005,
            )
        }
        Diamond => linear(
            "indent",
            recipe.diamond.as_ref()?.indent,
            [1.0, 0.0],
            [0.0, 0.01],
        ),
        Trapezoid => Control::linear(
            "indent",
            recipe.trapezoid.as_ref()?.indent,
            [0.0, 50.0],
            [0.0, 0.0],
            [0.01, 0.0],
        ),
        Polygon => {
            let polygon = recipe.polygon.as_ref()?;
            let count = polygon.number_of_sides.clamp(3, 1024) as f32;
            let step = TAU / count;
            let (sin, cos) = step.sin_cos();
            let edge = [sin * 0.5, (1.0 - cos) * 0.5];
            let middle = [0.5 + edge[0] * 0.5, edge[1] * 0.5];
            let normal = if polygon.smooth_points {
                let factor = (PI / count).cos();
                let offset = 0.5 * factor * (1.0 - factor);
                [(step * 0.5).sin() * offset, -(step * 0.5).cos() * offset]
            } else {
                [edge[1] * h / w * 0.25, -edge[0] * w / h * 0.25]
            };
            let base = if polygon.smooth_points {
                [middle[0] + normal[0], middle[1] + normal[1]]
            } else {
                middle
            };
            let factor = if polygon.smooth_points { 0.006 } else { 0.01 };
            Control::linear(
                "curve",
                polygon.curve,
                [-100.0, 100.0],
                base,
                normal.map(|v| v * factor),
            )
        }
        Spiral => {
            let spiral = recipe.spiral.as_ref()?;
            Control::linear(
                "turns",
                spiral.turns,
                [0.1, 32.0],
                [0.0, 0.14],
                [1.0 / 32.0, 0.0],
            )
        }
        Rectangle | Ellipse | Triangle | RoundedRectangle => return None,
    };
    Some(vec![control])
}
