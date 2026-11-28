use super::{TraceMode, TraceSettings, included};
use crate::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint};
use crate::raster_view::RasterView;
use crate::shape::ShapeKind;
use kurbo::{BezPath, Shape};

#[derive(Debug, Clone)]
pub(super) struct LinearGradient {
    pub colors: [[u8; 4]; 2],
    line: [[f64; 2]; 2],
}

impl LinearGradient {
    pub fn paint(&self, scale: [f64; 2]) -> Paint {
        let [start, end] = self.line.map(|point| [point[0] * scale[0], point[1] * scale[1]]);
        Paint::Gradient {
            kind: GradientKind::Linear,
            stops: self.colors.into_iter().enumerate().map(|(index, [r, g, b, a])| GradientStop {
                offset: index as f32, colour: Colour::new(r, g, b, a),
            }).collect(),
            geom: GradientGeom { start, end, ..Default::default() },
        }
    }
}

pub(super) fn detect_gradient(image: &RasterView, settings: &TraceSettings) -> Option<LinearGradient> {
    let mut samples = Vec::new();
    for y in 0..17 {
        for x in 0..17 {
            let point = [x as f64 / 16.0, y as f64 / 16.0];
            let pixel = image.pixel((point[0] * f64::from(image.width - 1)).round() as u32,
                (point[1] * f64::from(image.height - 1)).round() as u32)?;
            if !included(&pixel, settings) || pixel[3] < 245 { continue; }
            let color = if settings.mode == TraceMode::Grayscale {
                [super::luminance(pixel[0], pixel[1], pixel[2]) as f64; 3]
            } else {
                [pixel[0], pixel[1], pixel[2]].map(f64::from)
            };
            samples.push((point, color));
        }
    }
    if samples.len() < 24 { return None; }
    let n = samples.len() as f64;
    let center: [f64; 2] = std::array::from_fn(|axis| samples.iter().map(|(point, _)| point[axis]).sum::<f64>() / n);
    let mean: [f64; 3] = std::array::from_fn(|channel| samples.iter().map(|(_, color)| color[channel]).sum::<f64>() / n);
    let mut covariance = [0.0; 3];
    let mut moments = [[0.0; 2]; 3];
    for (point, color) in &samples {
        let x = point[0] - center[0];
        let y = point[1] - center[1];
        covariance[0] += x * x;
        covariance[1] += x * y;
        covariance[2] += y * y;
        for channel in 0..3 {
            let value = color[channel] - mean[channel];
            moments[channel][0] += x * value;
            moments[channel][1] += y * value;
        }
    }
    let [xx, xy, yy] = covariance;
    let determinant = xx * yy - xy * xy;
    if determinant < 0.0001 { return None; }
    let slopes = moments.map(|[x, y]| [(yy * x - xy * y) / determinant, (xx * y - xy * x) / determinant]);
    let [dx, dy] = slopes.iter().copied().max_by(|a, b| a[0].hypot(a[1]).total_cmp(&b[0].hypot(b[1])))?;
    let length = dx.hypot(dy);
    if length < 22.0 { return None; }
    let axis = [dx / length, dy / length];
    if slopes.iter().any(|slope| (slope[0] * axis[1] - slope[1] * axis[0]).abs() > 6.0) { return None; }
    let at = |point: [f64; 2], channel: usize| {
        mean[channel] + (point[0] - center[0]) * slopes[channel][0] + (point[1] - center[1]) * slopes[channel][1]
    };
    if samples.iter().any(|(point, color)| (0..3).any(|channel| (at(*point, channel) - color[channel]).abs() > 9.0)) {
        return None;
    }
    let [min, max] = samples.iter().map(|(point, _)| (point[0] - center[0]) * axis[0] + (point[1] - center[1]) * axis[1])
        .fold([0.0_f64, 0.0_f64], |[low, high], t| [low.min(t), high.max(t)]);
    let end_points = [min, max].map(|t| [center[0] + t * axis[0], center[1] + t * axis[1]]);
    let colors = end_points.map(|point| std::array::from_fn(|channel| {
        if channel < 3 { at(point, channel).round().clamp(0.0, 255.0) as u8 } else { 255 }
    }));
    // A normalized gradient is a scalar field. Convert its normal to pixel
    // coordinates before choosing the endpoint line on a non-square image.
    let size = [f64::from(image.width), f64::from(image.height)];
    let normal = [axis[0] / size[0], axis[1] / size[1]];
    let scale = normal[0].powi(2) + normal[1].powi(2);
    let line = [min, max].map(|t| std::array::from_fn(|axis| center[axis] * size[axis] + t * normal[axis] / scale));
    Some(LinearGradient { colors, line })
}

pub(super) fn recognize_shape(path: &mut BezPath, tolerance: f32) -> Option<ShapeKind> {
    let parts = crate::outline::flatten_subpaths(path, 0.35);
    let [(points, true)] = parts.as_slice() else { return None };
    let rect = path.bounding_box();
    if rect.width() < 7.0 || rect.height() < 7.0 { return None; }
    let error = f64::from(tolerance).clamp(0.65, 2.5);
    let area = path.area().abs();
    if (area / rect.area() - 1.0).abs() < 0.055 && points.iter().all(|&[x, y]| {
        [(x - rect.x0).abs(), (x - rect.x1).abs(), (y - rect.y0).abs(), (y - rect.y1).abs()]
            .into_iter().fold(f64::INFINITY, f64::min) <= error
    }) {
        *path = crate::geometry::rect_to_bez_path(rect.x0, rect.y0, rect.width(), rect.height());
        return Some(ShapeKind::Rectangle);
    }
    let center = rect.center();
    let [rx, ry] = [rect.width() * 0.5, rect.height() * 0.5];
    if (area / rect.area() - std::f64::consts::FRAC_PI_4).abs() < 0.04 && points.iter().all(|&[x, y]| {
        (((x - center.x) / rx).hypot((y - center.y) / ry) - 1.0).abs() * rx.min(ry) <= error
    }) {
        *path = crate::geometry::ellipse_to_bez_path(center.x, center.y, rx, ry);
        return Some(ShapeKind::Ellipse);
    }
    None
}
