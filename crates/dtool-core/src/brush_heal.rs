use super::{accumulate_coverage, dab_coverage, BrushSettings, Dab};
use crate::raster_view::RasterViewMut;

const MAX_PASSES: usize = 200;

pub(crate) fn heal_dabs(
    dst: &mut RasterViewMut,
    dabs: &[Dab],
    settings: &BrushSettings,
    origin: [u32; 2],
    sample: impl Fn([u32; 2]) -> [u8; 4],
) {
    let opacity = settings.opacity.clamp(0.0, 1.0);
    if opacity == 0.0 || settings.flow <= 0.0 { return; }
    let width = dst.width as usize;
    let height = dst.height as usize;
    let mut coverage = vec![0.0; width * height];
    for dab in dabs {
        dab_coverage(dab, settings, dst.width, dst.height, |x, y, amount| {
            accumulate_coverage(&mut coverage[y * width + x], amount, opacity);
        });
    }
    heal_region(dst, &mut coverage, |index| {
        sample([origin[0] + (index % width) as u32, origin[1] + (index / width) as u32])
    }, MAX_PASSES);
}

pub(crate) fn heal_region(
    dst: &mut RasterViewMut,
    coverage: &mut [f32],
    sample: impl Fn(usize) -> [u8; 4],
    max_passes: usize,
) -> bool {
    let width = dst.width as usize;
    let height = dst.height as usize;
    // Solve for a smooth correction using the colours along the selection boundary.
    let mut correction = vec![[f32::NAN; 3]; coverage.len()];
    let mut active = Vec::new();
    for (index, pixel) in dst.rgba.chunks_exact(4).enumerate() {
        let src = sample(index);
        if pixel[3] == 0 || src[3] == 0 {
            coverage[index] = 0.0;
            continue;
        }
        correction[index] = std::array::from_fn(|channel| f32::from(pixel[channel]) - f32::from(src[channel]));
        if coverage[index] > 0.0 { active.push(index); }
    }
    let neighbours = |index: usize| {
        let x = index % width;
        let y = index / width;
        [
            (x > 0).then(|| index - 1),
            (x + 1 < width).then(|| index + 1),
            (y > 0).then(|| index - width),
            (y + 1 < height).then(|| index + width),
        ].into_iter().flatten()
    };
    let mut boundary = [0.0; 3];
    let mut samples = 0;
    for &index in &active {
        for edge in neighbours(index).filter(|&n| coverage[n] == 0.0 && correction[n][0].is_finite()) {
            for (sum, value) in boundary.iter_mut().zip(correction[edge]) { *sum += value; }
            samples += 1;
        }
    }
    if samples == 0 || active.is_empty() { return false; }
    let initial = boundary.map(|sum| sum / samples as f32);
    for &index in &active { correction[index] = initial; }
    for _ in 0..max_passes {
        let mut largest_change = 0.0_f32;
        for &index in &active {
            let mut sum = [0.0; 3];
            let mut count = 0;
            for n in neighbours(index).filter(|&n| correction[n][0].is_finite()) {
                for (sum, value) in sum.iter_mut().zip(correction[n]) { *sum += value; }
                count += 1;
            }
            if count == 0 { continue; }
            for (current, sum) in correction[index].iter_mut().zip(sum) {
                let change = (sum / count as f32 - *current) * 1.65;
                *current += change;
                largest_change = largest_change.max(change.abs());
            }
        }
        if largest_change < 0.04 { break; }
    }
    for index in active {
        let src = sample(index);
        let target = &mut dst.rgba[index * 4..index * 4 + 3];
        for (channel, into) in target.iter_mut().enumerate() {
            let healed = (f32::from(src[channel]) + correction[index][channel]).clamp(0.0, 255.0);
            let previous = f32::from(*into);
            *into = (previous + (healed - previous) * coverage[index]).round() as u8;
        }
    }
    true
}
