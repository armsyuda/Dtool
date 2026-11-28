use super::{accumulate_coverage, channel8, dab_coverage, mix_colour, BrushOp, BrushSettings, Dab, OpParams, SharpenMode};
use crate::raster_view::{RasterView, RasterViewMut};
use std::collections::VecDeque;

type Pixel = [f32; 4];

pub(crate) fn filter_padding(op: BrushOp, settings: &BrushSettings, params: &OpParams) -> f32 {
    radius(op, settings, params) as f32
}

fn radius(op: BrushOp, settings: &BrushSettings, params: &OpParams) -> usize {
    let flow = settings.flow.clamp(0.0, 1.0);
    match op {
        BrushOp::Blur => 1 + (flow * 5.0).round() as usize,
        BrushOp::Median => 1 + (flow * 7.0).round() as usize,
        BrushOp::Sharpen => match params.sharpen_mode {
            SharpenMode::Clarity => 4 + (flow * 6.0).round() as usize,
            SharpenMode::UnsharpMask => 1 + (flow * 4.0).round() as usize,
            SharpenMode::Harsh => 1,
        },
        _ => 0,
    }
}

pub(crate) fn apply(
    dst: &mut RasterViewMut, paths: impl Iterator<Item = Vec<Dab>>, settings: &BrushSettings,
    params: &OpParams, op: BrushOp,
) {
    let opacity = settings.opacity.clamp(0.0, 1.0);
    if opacity == 0.0 || settings.flow <= 0.0 || dst.width == 0 || dst.height == 0 { return; }
    let original = dst.rgba.to_vec();
    let source = RasterView::new(dst.width, dst.height, &original);
    if op == BrushOp::Smudge {
        for path in paths {
            smudge(dst, &source, &path, settings, params);
        }
        return;
    }

    let width = dst.width as usize;
    let mut coverage = vec![0.0_f32; original.len() / 4];
    for path in paths {
        for dab in &path {
            dab_coverage(dab, settings, dst.width, dst.height, |x, y, amount| {
                accumulate_coverage(&mut coverage[y * width + x], amount, opacity);
            });
        }
    }
    let mut bounds = [width, dst.height as usize, 0, 0];
    for (i, _) in coverage.iter().enumerate().filter(|(_, amount)| **amount > 0.0) {
        let (x, y) = (i % width, i / width);
        bounds = [bounds[0].min(x), bounds[1].min(y), bounds[2].max(x + 1), bounds[3].max(y + 1)];
    }
    if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] { return; }

    let radius = radius(op, settings, params);
    let original_pixel = |x, y| clamped(&source, x as i64, y as i64);
    let mut write = |x: usize, y: usize, sampled: Pixel| {
        let index = y * width + x;
        let amount = coverage[index];
        if amount == 0.0 { return; }
        let initial = original_pixel(x, y);
        let amount = amount * tonal_coverage(params, || initial);
        if amount == 0.0 { return; }
        let adjusted = match op {
            BrushOp::Sharpen => sharpen(initial, sampled, params.sharpen_mode),
            _ => sampled,
        };
        dst.rgba[index * 4..index * 4 + 4].copy_from_slice(&straight(lerp(initial, adjusted, amount)));
    };
    match (op, params.sharpen_mode) {
        (BrushOp::Median, _) => median(&source, bounds, radius, &mut write),
        (BrushOp::Sharpen, SharpenMode::Harsh) => {
            let [left, top, right, bottom] = bounds;
            for y in top..bottom {
                for x in left..right {
                    let (xi, yi) = (x as i64, y as i64);
                    let neighbours = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                        .map(|(dx, dy)| clamped(&source, xi + dx, yi + dy));
                    write(x, y, std::array::from_fn(|i| neighbours.iter().map(|p| p[i]).sum::<f32>() * 0.25));
                }
            }
        }
        _ => gaussian(&source, bounds, radius, &mut write),
    }
}

fn tonal_coverage(params: &OpParams, pixel: impl FnOnce() -> Pixel) -> f32 {
    let Some(range) = params.filter_tonal_range else { return 1.0; };
    let [r, g, b, alpha] = pixel();
    if alpha <= 0.0 { return 0.0; }
    super::tonal_weight([r / alpha, g / alpha, b / alpha], range)
}

fn premultiplied(pixel: [u8; 4]) -> Pixel {
    let [r, g, b, a] = pixel.map(|channel| f32::from(channel) / 255.0);
    [r * a, g * a, b * a, a]
}

fn straight(pixel: Pixel) -> [u8; 4] {
    let alpha = pixel[3];
    let a = channel8(alpha);
    if a == 0 { return [0; 4]; }
    let [r, g, b] = [0, 1, 2].map(|i| channel8(pixel[i] / alpha));
    [r, g, b, a]
}

fn clamped(source: &RasterView, x: i64, y: i64) -> Pixel {
    premultiplied(clamped_rgba(source, x, y))
}

fn clamped_rgba(source: &RasterView, x: i64, y: i64) -> [u8; 4] {
    source.get_pixel(x.clamp(0, i64::from(source.width) - 1), y.clamp(0, i64::from(source.height) - 1))
}

fn lerp(from: Pixel, to: Pixel, amount: f32) -> Pixel {
    std::array::from_fn(|i| from[i] + (to[i] - from[i]) * amount)
}

fn sharpen(initial: Pixel, low_pass: Pixel, mode: SharpenMode) -> Pixel {
    let alpha = initial[3];
    if alpha == 0.0 || low_pass[3] <= f32::EPSILON { return initial; }
    let luma = (0.2126 * initial[0] + 0.7152 * initial[1] + 0.0722 * initial[2]) / alpha;
    let gain = match mode {
        SharpenMode::Clarity => 1.5 * (1.0 - (2.0 * luma - 1.0).abs()),
        SharpenMode::UnsharpMask => 1.0,
        SharpenMode::Harsh => 4.0,
    };
    let mut out = initial;
    for i in 0..3 {
        let blurred = low_pass[i] * alpha / low_pass[3];
        out[i] = (initial[i] + gain * (initial[i] - blurred)).clamp(0.0, alpha);
    }
    out
}

fn gaussian(source: &RasterView, [left, top, right, bottom]: [usize; 4], radius: usize, write: &mut impl FnMut(usize, usize, Pixel)) {
    let radius = radius as i64;
    let sigma = radius as f32 / 2.0;
    let mut kernel: Vec<_> = (-radius..=radius).map(|i| (-0.5 * (i as f32 / sigma).powi(2)).exp()).collect();
    let sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|weight| *weight /= sum);
    let horizontal = |y: i64| -> Vec<Pixel> {
        (left..right).map(|x| {
            let mut sum = [0.0; 4];
            for (i, weight) in kernel.iter().enumerate() {
                let sample = clamped(source, x as i64 + i as i64 - radius, y);
                for c in 0..4 { sum[c] += sample[c] * weight; }
            }
            sum
        }).collect()
    };
    let mut rows: VecDeque<_> = (top as i64 - radius..=top as i64 + radius).map(horizontal).collect();
    for y in top..bottom {
        for x in left..right {
            let mut sum = [0.0; 4];
            for (row, weight) in rows.iter().zip(&kernel) {
                for c in 0..4 { sum[c] += row[x - left][c] * weight; }
            }
            write(x, y, sum);
        }
        if y + 1 < bottom {
            rows.pop_front();
            rows.push_back(horizontal(y as i64 + radius + 1));
        }
    }
}

fn median(source: &RasterView, [left, top, right, bottom]: [usize; 4], radius: usize, write: &mut impl FnMut(usize, usize, Pixel)) {
    let sample_count = (2 * radius + 1).pow(2);
    let radius = radius as i64;
    for y in top..bottom {
        let mut histograms = [[0u16; 256]; 4];
        let column = |x: i64, adding: bool, histograms: &mut [[u16; 256]; 4]| {
            for dy in -radius..=radius {
                let sample = clamped_rgba(source, x, y as i64 + dy);
                let first_channel = if sample[3] == 0 { 3 } else { 0 };
                for (histogram, channel) in histograms.iter_mut().zip(sample).skip(first_channel) {
                    let count = &mut histogram[channel as usize];
                    if adding { *count += 1; } else { *count -= 1; }
                }
            }
        };
        for x in left as i64 - radius..=left as i64 + radius {
            column(x, true, &mut histograms);
        }
        for x in left..right {
            let middle_visible = (sample_count - usize::from(histograms[3][0])) / 2;
            let channels = std::array::from_fn(|i| {
                let middle = if i == 3 { sample_count / 2 } else { middle_visible };
                let mut total = 0usize;
                histograms[i].iter().position(|&count| {
                    total += usize::from(count);
                    total > middle
                }).unwrap_or_default() as u8
            });
            write(x, y, premultiplied(channels));
            if x + 1 < right {
                column(x as i64 - radius, false, &mut histograms);
                column(x as i64 + radius + 1, true, &mut histograms);
            }
        }
    }
}

fn smudge(dst: &mut RasterViewMut, source: &RasterView, dabs: &[Dab], settings: &BrushSettings, params: &OpParams) {
    let Some(first) = dabs.first() else { return; };
    let strength = params.strength.clamp(0.0, 1.0);
    if strength == 0.0 { return; }
    let radius = dabs.iter().map(|dab| dab.size).fold(0.0, f32::max).mul_add(0.5, 1.0).ceil() as i64;
    let side = (2 * radius + 1) as usize;
    let count = side * side;
    let anchor = |dab: &Dab| (dab.x.floor() as i64, dab.y.floor() as i64);
    let sample = |cx, cy, i: usize| {
        premultiplied(source.get_pixel(cx + i as i64 % side as i64 - radius, cy + i as i64 / side as i64 - radius))
    };
    let (first_x, first_y) = anchor(first);
    let mut carried: Vec<_> = (0..count).map(|i| sample(first_x, first_y, i)).collect();
    let pickup = 1.0 - 0.8 * strength;
    for dab in dabs.iter().skip(1) {
        let (cx, cy) = anchor(dab);
        dab_coverage(dab, settings, dst.width, dst.height, |x, y, amount| {
            let index = (y as i64 - cy + radius) as usize * side + (x as i64 - cx + radius) as usize;
            let load = carried[index];
            let current = premultiplied(dst.get_pixel(x as i64, y as i64));
            let weight = amount * strength * tonal_coverage(params, || {
                premultiplied(source.get_pixel(x as i64, y as i64))
            });
            if weight <= 0.0 { return; }
            let mut result = lerp(current, load, weight);
            if params.pigment && current[3] > 0.0 && load[3] > 0.0 {
                let mixed = mix_colour(straight(current), straight(load), weight, true);
                for c in 0..3 { result[c] = f32::from(mixed[c]) / 255.0 * result[3]; }
            }
            let [r, g, b, a] = straight(result);
            dst.set_pixel(x as i64, y as i64, r, g, b, a);
        });
        for (i, load) in carried.iter_mut().enumerate() {
            *load = lerp(*load, sample(cx, cy, i), pickup);
        }
    }
}
