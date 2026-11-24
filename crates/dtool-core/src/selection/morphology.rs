use std::collections::VecDeque;

pub(super) fn extrema(input: &[u8], size: [usize; 2], radius: f32, circular: bool, grow: bool) -> Vec<u8> {
    let [width, height] = size;
    if width == 0 || height == 0 || radius < 1.0 { return input.to_vec(); }
    let radius = radius.floor() as usize;
    if !grow && radius >= width.min(height).div_ceil(2) {
        return vec![0; input.len()];
    }
    let diagonal = ((width - 1) as f64).hypot((height - 1) as f64);
    if grow && ((!circular && radius >= width.max(height) - 1) || radius as f64 >= diagonal) {
        return vec![input.iter().copied().max().unwrap_or(0); input.len()];
    }
    let mut row = vec![0; input.len()];
    let mut result = vec![if grow { 0 } else { 255 }; input.len()];
    let mut queue = VecDeque::new();
    if !circular {
        horizontal(input, &mut row, size, radius, grow, &mut queue);
        for x in 0..width {
            line(&row, &mut result, x, height, width, radius, grow, &mut queue);
        }
        return result;
    }

    let square = (radius as u64).pow(2);
    let mut previous_span = usize::MAX;
    // Each offset uses the exact horizontal chord of the circle.
    for dy in 0..=radius.min(height - 1) {
        let span = (square - (dy as u64).pow(2)).isqrt().min(width as u64) as usize;
        if span != previous_span {
            horizontal(input, &mut row, size, span, grow, &mut queue);
            previous_span = span;
        }
        let offset = dy * width;
        let length = input.len() - offset;
        merge(&mut result[..length], &row[offset..], grow);
        if dy > 0 { merge(&mut result[offset..], &row[..length], grow); }
    }
    if !grow {
        let inset = radius * width;
        result[..inset].fill(0);
        let bottom = result.len() - inset;
        result[bottom..].fill(0);
    }
    result
}

fn horizontal(
    input: &[u8], output: &mut [u8], [width, height]: [usize; 2], radius: usize, grow: bool,
    queue: &mut VecDeque<(usize, u8)>,
) {
    for y in 0..height {
        line(input, output, y * width, width, 1, radius, grow, queue);
    }
}

fn merge(dst: &mut [u8], src: &[u8], grow: bool) {
    if grow {
        for (dst, src) in dst.iter_mut().zip(src) { *dst = (*dst).max(*src); }
    } else {
        for (dst, src) in dst.iter_mut().zip(src) { *dst = (*dst).min(*src); }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn line(
    input: &[u8], output: &mut [u8], start: usize, len: usize, step: usize, radius: usize, grow: bool,
    queue: &mut VecDeque<(usize, u8)>,
) {
    queue.clear();
    let mut end = 0;
    for i in 0..len {
        let from = i.saturating_sub(radius);
        while queue.front().is_some_and(|&(index, _)| index < from) { queue.pop_front(); }
        let to = i.saturating_add(radius).saturating_add(1).min(len);
        while end < to {
            let value = input[start + end * step];
            while queue.back().is_some_and(|&(_, last)| if grow { last <= value } else { last >= value }) {
                queue.pop_back();
            }
            queue.push_back((end, value));
            end += 1;
        }
        output[start + i * step] = if !grow && (i < radius || radius >= len - i) {
            0
        } else {
            queue.front().map_or(0, |&(_, value)| value)
        };
    }
}
