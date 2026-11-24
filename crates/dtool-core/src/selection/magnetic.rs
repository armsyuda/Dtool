use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::document::Document;
use crate::raster_view::RasterView;

const MAX_SAMPLE_SIDE: u32 = 2048;
const SEARCH_LIMIT: usize = 64_000;
const EDGE_MIN: u32 = 24;
const SNAP_RADIUS: i32 = 6;
pub const MAX_ANCHORS: usize = 1024;

#[derive(Debug)]
pub struct MagneticImage {
    size: [u32; 2],
    scale: [f32; 2],
    rgba: Vec<u8>,
}

impl MagneticImage {
    pub fn capture(document: &Document) -> Option<Self> {
        Self::capture_surface(document, super::PixelSurface::for_page(document, None)?)
    }

    pub fn capture_surface(document: &Document, surface: super::PixelSurface) -> Option<Self> {
        let [width, height] = surface.size;
        let ratio = (MAX_SAMPLE_SIDE as f32 / width.max(height) as f32).min(1.0);
        let size = [width, height].map(|side| (side as f32 * ratio).round().max(1.0) as u32);
        let scale = [size[0] as f32 / width as f32, size[1] as f32 / height as f32];
        let rgba = crate::layer_style::raster::render_region_scaled(
            document, &crate::CanvasFlags::default(), &[],
            surface.bounds, size[0], size[1], size[0] as f32 / surface.bounds[2],
        );
        (rgba.len() == size[0] as usize * size[1] as usize * 4).then_some(Self { size, scale, rgba })
    }

    pub fn snap(&self, point: [f32; 2]) -> [f32; 2] {
        self.edge_at(point).map_or(point, |edge| self.to_document(edge))
    }

    pub fn trace(&self, from: [f32; 2], to: [f32; 2]) -> Vec<[f32; 2]> {
        let start = self.edge_at(from);
        let end = self.edge_at(to);
        if let (Some(start), Some(end)) = (start, end)
            && let Some(path) = self.find_path(start, end)
        {
            return path.into_iter().map(|point| self.to_document(point)).collect();
        }
        vec![start.map_or(from, |point| self.to_document(point)), end.map_or(to, |point| self.to_document(point))]
    }

    pub fn outline(&self, anchors: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let mut path = Vec::new();
        for (&start, &end) in anchors.iter().zip(anchors.iter().cycle().skip(1)) {
            let segment = self.trace(start, end);
            path.extend(segment.into_iter().skip(usize::from(!path.is_empty())));
        }
        path
    }

    fn to_document(&self, [x, y]: [i32; 2]) -> [f32; 2] {
        [x as f32 / self.scale[0], y as f32 / self.scale[1]]
    }

    fn edge_at(&self, point: [f32; 2]) -> Option<[i32; 2]> {
        let scaled: [f32; 2] = std::array::from_fn(|axis| point[axis] * self.scale[axis]);
        if scaled.iter().zip(self.size).any(|(&p, size)| !p.is_finite() || p < 0.0 || p > size as f32) {
            return None;
        }
        let origin = scaled.map(|p| p.round() as i32);
        let raster = RasterView::new(self.size[0], self.size[1], &self.rgba);
        let mut closest = None;
        let mut best = f32::INFINITY;
        for y in (origin[1] - SNAP_RADIUS).max(0)..=(origin[1] + SNAP_RADIUS).min(self.size[1] as i32) {
            for x in (origin[0] - SNAP_RADIUS).max(0)..=(origin[0] + SNAP_RADIUS).min(self.size[0] as i32) {
                let position = [x, y];
                let distance = (x as f32 - scaled[0]).hypot(y as f32 - scaled[1]);
                if distance > SNAP_RADIUS as f32 { continue; }
                let strength = neighbors(position).into_iter()
                    .filter(|&other| self.in_grid(other))
                    .map(|other| contrast(&raster, position, other))
                    .max().unwrap_or(0);
                let rank = distance + (255 - strength) as f32 / 80.0;
                if strength >= EDGE_MIN && rank < best {
                    best = rank;
                    closest = Some(position);
                }
            }
        }
        closest
    }

    fn in_grid(&self, [x, y]: [i32; 2]) -> bool {
        x >= 0 && y >= 0 && x <= self.size[0] as i32 && y <= self.size[1] as i32
    }

    fn find_path(&self, start: [i32; 2], end: [i32; 2]) -> Option<Vec<[i32; 2]>> {
        if start == end { return Some(vec![start]); }
        let padding = (distance(start, end) / 3).clamp(12, 112) as i32;
        let min: [i32; 2] = std::array::from_fn(|axis| (start[axis].min(end[axis]) - padding).max(0));
        let max: [i32; 2] = std::array::from_fn(|axis| (start[axis].max(end[axis]) + padding).min(self.size[axis] as i32));
        let stride = (max[0] - min[0] + 1) as usize;
        let count = stride * (max[1] - min[1] + 1) as usize;
        let index = |[x, y]: [i32; 2]| (y - min[1]) as usize * stride + (x - min[0]) as usize;
        let position = |i: usize| [min[0] + (i % stride) as i32, min[1] + (i / stride) as i32];
        let mut costs = vec![u32::MAX; count];
        let mut previous = vec![usize::MAX; count];
        let start_index = index(start);
        let end_index = index(end);
        costs[start_index] = 0;
        let mut frontier = BinaryHeap::from([Reverse((distance(start, end), 0, start_index))]);
        let raster = RasterView::new(self.size[0], self.size[1], &self.rgba);
        let mut visited = 0;

        while let Some(Reverse((_, cost, current))) = frontier.pop() {
            if costs[current] != cost { continue; }
            if current == end_index {
                let mut path = vec![end];
                let mut i = current;
                while i != start_index {
                    i = previous[i];
                    path.push(position(i));
                }
                path.reverse();
                return Some(corners(path));
            }
            visited += 1;
            if visited == SEARCH_LIMIT { break; }
            let here = position(current);
            for [x, y] in neighbors(here) {
                if x < min[0] || y < min[1] || x > max[0] || y > max[1] { continue; }
                let next = [x, y];
                let next_index = index(next);
                let strength = contrast(&raster, here, next);
                let next_cost = cost + 1 + (255 - strength) / 32;
                if next_cost < costs[next_index] {
                    costs[next_index] = next_cost;
                    previous[next_index] = current;
                    frontier.push(Reverse((next_cost + distance(next, end), next_cost, next_index)));
                }
            }
        }
        None
    }
}

fn neighbors([x, y]: [i32; 2]) -> [[i32; 2]; 4] {
    [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]]
}

fn distance(a: [i32; 2], b: [i32; 2]) -> u32 {
    a[0].abs_diff(b[0]) + a[1].abs_diff(b[1])
}

fn contrast(raster: &RasterView, [x, y]: [i32; 2], [next_x, next_y]: [i32; 2]) -> u32 {
    let at = |x, y| raster.get_pixel(i64::from(x), i64::from(y));
    let (a, b) = if y == next_y {
        let column = x.min(next_x);
        (at(column, y - 1), at(column, y))
    } else {
        let row = y.min(next_y);
        (at(x - 1, row), at(x, row))
    };
    let premultiplied = |pixel: [u8; 4], channel: usize| u32::from(pixel[channel]) * u32::from(pixel[3]) / 255;
    (0..3).map(|channel| premultiplied(a, channel).abs_diff(premultiplied(b, channel)))
        .fold(u32::from(a[3].abs_diff(b[3])), u32::max)
}

fn corners(path: Vec<[i32; 2]>) -> Vec<[i32; 2]> {
    let mut result = vec![path[0]];
    result.extend(path.windows(3).filter_map(|turn| {
        let [a, b, c] = [turn[0], turn[1], turn[2]];
        ((b[0] - a[0], b[1] - a[1]) != (c[0] - b[0], c[1] - b[1])).then_some(b)
    }));
    result.push(*path.last().expect("nonempty path"));
    result
}
