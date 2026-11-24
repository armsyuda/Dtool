use crate::selection::morphology::line as extrema;
use crate::layer_style::OutlineAlign;
use std::collections::VecDeque;

pub(super) fn alpha_outline(buffer: &mut [u8], size: [usize; 2], radius: usize, align: OutlineAlign) {
    let alpha: Vec<_> = buffer.iter().skip(3).step_by(4).copied().collect();
    let outer = (align != OutlineAlign::Inside).then(|| octagon(&alpha, size, radius, true));
    let inner = (align != OutlineAlign::Outside).then(|| octagon(&alpha, size, radius, false));
    for (index, pixel) in buffer.chunks_exact_mut(4).enumerate() {
        let original = alpha[index];
        let high = outer.as_ref().map_or(original, |values| values[index]);
        let low = inner.as_ref().map_or(original, |values| values[index]);
        pixel[3] = high.saturating_sub(low);
    }
}

pub(super) fn octagon(alpha: &[u8], [width, height]: [usize; 2], radius: usize, grow: bool) -> Vec<u8> {
    // Four line filters produce a rounded approximation without scanning a disk for every pixel.
    let diagonal = ((radius as f32 * (1.0 - std::f32::consts::FRAC_1_SQRT_2)).round() as usize).min(radius / 2);
    let side = radius - 2 * diagonal;
    let mut input = alpha.to_vec();
    let mut output = vec![0; alpha.len()];
    let mut queue = VecDeque::new();
    let mut pass = |lines: &[(usize, usize)], step, reach| {
        if reach == 0 {
            return;
        }
        for &(start, len) in lines {
            extrema(&input, &mut output, start, len, step, reach, grow, &mut queue);
        }
        std::mem::swap(&mut input, &mut output);
    };
    let rows: Vec<_> = (0..height).map(|y| (y * width, width)).collect();
    pass(&rows, 1, side);
    let columns: Vec<_> = (0..width).map(|x| (x, height)).collect();
    pass(&columns, width, side);
    let down_right: Vec<_> = (0..width).map(|x| (x, (width - x).min(height)))
        .chain((1..height).map(|y| (y * width, width.min(height - y)))).collect();
    pass(&down_right, width + 1, diagonal);
    if width > 1 {
        let down_left: Vec<_> = (0..width).map(|x| (x, (x + 1).min(height)))
            .chain((1..height).map(|y| (y * width + width - 1, width.min(height - y)))).collect();
        pass(&down_left, width - 1, diagonal);
    }
    input
}
