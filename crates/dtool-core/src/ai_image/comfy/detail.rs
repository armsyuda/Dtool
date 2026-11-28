use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::{CommandError, ai::AiImage};

use super::{Candidate, MAX_SIDE, Request, agent, check_cancelled, encode_candidate, execute_graph, upload, workflow};

const TILE: u32 = 768;
const OVERLAP: u32 = 80;
const CONTEXT: u32 = 64;
pub const DEFAULT_PROMPT: &str = "natural fine detail, coherent original composition, high quality textures";

pub fn output_size([width, height]: [u32; 2], scale: u32) -> Option<[u32; 2]> {
    let size = [width.checked_mul(scale)?, height.checked_mul(scale)?];
    (matches!(scale, 1 | 2) && size.iter().all(|side| (64..=MAX_SIDE).contains(side))).then_some(size)
}

fn spans(length: u32) -> Vec<Range<u32>> {
    let count = length.saturating_sub(OVERLAP).div_ceil(TILE - OVERLAP).max(1);
    (0..count).map(|index| {
        let start = length * index / count;
        let end = length * (index + 1) / count;
        let left = if index == 0 { 0 } else { start - OVERLAP / 2 };
        let right = if index + 1 == count { length } else { end + OVERLAP / 2 };
        left..right
    }).collect()
}

fn weight(position: u32, span: &Range<u32>, limit: u32) -> f32 {
    let left = if span.start == 0 { OVERLAP } else { position - span.start + 1 };
    let right = if span.end == limit { OVERLAP } else { span.end - position };
    let t = left.min(right).min(OVERLAP) as f32 / OVERLAP as f32;
    t * t * (3.0 - 2.0 * t)
}

pub(in crate::ai_image) fn run(
    mut request: Request,
    source: &AiImage,
    size: [u32; 2],
    protected: Option<&[u8]>,
    progress: &AtomicU8,
    cancelled: &AtomicBool,
) -> Result<Vec<Candidate>, CommandError> {
    let [width, height] = size;
    check_cancelled(cancelled)?;
    let mut image = if size == [source.width, source.height] {
        source.clone()
    } else {
        AiImage { width, height, rgba: crate::canvas_size::resize_rgba_lanczos(
            &source.rgba, source.width, source.height, width, height,
        ) }
    };
    let mut sum = vec![[0.0; 4]; width as usize * height as usize];
    let columns = spans(width);
    let rows = spans(height);
    let total = columns.len() * rows.len();
    let base = format!("http://127.0.0.1:{}", request.port);
    let agent = agent();
    for (index, (cols, rows)) in rows.iter().flat_map(|rows| columns.iter().map(move |cols| (cols, rows))).enumerate() {
        check_cancelled(cancelled)?;
        let origin_x = cols.start.saturating_sub(CONTEXT);
        let origin_y = rows.start.saturating_sub(CONTEXT);
        let end_x = (cols.end + CONTEXT).min(width);
        let end_y = (rows.end + CONTEXT).min(height);
        let tile_width = (end_x - origin_x).next_multiple_of(8);
        let tile_height = (end_y - origin_y).next_multiple_of(8);
        let mut rgba = Vec::with_capacity(tile_width as usize * tile_height as usize * 4);
        for y in origin_y..origin_y + tile_height {
            for x in origin_x..origin_x + tile_width {
                let i = ((y.min(end_y - 1) * width + x.min(end_x - 1)) * 4) as usize;
                rgba.extend_from_slice(&image.rgba[i..i + 4]);
            }
        }
        let tile = AiImage { width: tile_width, height: tile_height, rgba };
        let input = upload(&agent, &base, &tile)?;
        let graph = workflow(&request, Some(&input), None);
        let mut results = execute_graph(&agent, &base, graph, 1, Ok, cancelled)?;
        let rendered = results.pop().ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?.image;
        if (rendered.width, rendered.height) != (tile_width, tile_height) {
            return Err(CommandError::new("AI_IMAGE_RESPONSE"));
        }
        for y in rows.clone() {
            let wy = weight(y, rows, height);
            for x in cols.clone() {
                let w = wy * weight(x, cols, width);
                let i = (((y - origin_y) * tile_width + x - origin_x) * 4) as usize;
                let pixel = &mut sum[(y * width + x) as usize];
                for channel in 0..3 {
                    pixel[channel] += rendered.rgba[i + channel] as f32 * w;
                }
                pixel[3] += w;
            }
        }
        request.seed = request.seed.wrapping_add(1);
        progress.store(((index + 1) * 99 / total) as u8, Ordering::Release);
    }
    check_cancelled(cancelled)?;
    let scale = width / source.width;
    for (index, (pixel, sum)) in image.rgba.chunks_exact_mut(4).zip(sum).enumerate() {
        let coverage = protected.map_or(0, |mask| {
            let x = (index as u32 % width) / scale;
            let y = (index as u32 / width) / scale;
            mask[(y * source.width + x) as usize]
        });
        if pixel[3] == 0 || coverage == 255 {
            continue;
        }
        let original = coverage as f32 / 255.0;
        for channel in 0..3 {
            let regenerated = sum[channel] / sum[3];
            pixel[channel] = (pixel[channel] as f32 * original + regenerated * (1.0 - original)).round() as u8;
        }
    }
    Ok(vec![encode_candidate(image)?])
}
