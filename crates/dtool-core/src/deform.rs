use crate::raster_view::RasterView;

pub struct BoneChain {
    pub from: Vec<[f32; 2]>,
    pub to: Vec<[f32; 2]>,
}

impl BoneChain {
    pub fn new() -> Self {
        Self { from: Vec::new(), to: Vec::new() }
    }

    pub fn add_point(&mut self, from: [f32; 2], to: [f32; 2]) {
        self.from.push(from);
        self.to.push(to);
    }

    pub fn is_empty(&self) -> bool {
        self.from.is_empty() || self.to.is_empty()
    }

    pub fn len(&self) -> usize {
        self.from.len().min(self.to.len())
    }
}

pub fn deform(src: &RasterView, chains: &[BoneChain]) -> Vec<u8> {
    let width = src.width as usize;
    let height = src.height as usize;
    let channels = 4usize;
    let total_pixels = width * height;
    let mut output = vec![0u8; total_pixels * channels];

    if chains.is_empty() {
        return src.rgba.to_vec();
    }

    for y in 0..height {
        for x in 0..width {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let mut dx_total = 0.0f32;
            let mut dy_total = 0.0f32;
            let mut weight_total = 0.0f32;

            for chain in chains {
                if chain.is_empty() {
                    continue;
                }

                let (dx, dy, w) = bone_deform(px, py, &chain.from, &chain.to);
                dx_total += dx * w;
                dy_total += dy * w;
                weight_total += w;
            }

            if weight_total <= 0.0 {
                let idx = (y * width + x) * channels;
                for c in 0..channels {
                    output[idx + c] = src.rgba[idx + c];
                }
                continue;
            }

            let dx_avg = dx_total / weight_total;
            let dy_avg = dy_total / weight_total;

            let src_x = px - dx_avg;
            let src_y = py - dy_avg;

            if src_x < 0.0 || src_x >= width as f32 || src_y < 0.0 || src_y >= height as f32 {
                continue;
            }

            let x0 = src_x.floor() as usize;
            let y0 = src_y.floor() as usize;
            let x1 = (x0 + 1).min(width - 1);
            let y1 = (y0 + 1).min(height - 1);

            let fx = src_x - x0 as f32;
            let fy = src_y - y0 as f32;

            let idx00 = (y0 * width + x0) * channels;
            let idx10 = (y0 * width + x1) * channels;
            let idx01 = (y1 * width + x0) * channels;
            let idx11 = (y1 * width + x1) * channels;

            let dst_idx = (y * width + x) * channels;

            for c in 0..channels {
                let v00 = src.rgba[idx00 + c] as f32;
                let v10 = src.rgba[idx10 + c] as f32;
                let v01 = src.rgba[idx01 + c] as f32;
                let v11 = src.rgba[idx11 + c] as f32;

                let top = v00 * (1.0 - fx) + v10 * fx;
                let bottom = v01 * (1.0 - fx) + v11 * fx;
                let val = top * (1.0 - fy) + bottom * fy;

                output[dst_idx + c] = val.clamp(0.0, 255.0) as u8;
            }
        }
    }

    output
}

fn bone_deform(px: f32, py: f32, from: &[[f32; 2]], to: &[[f32; 2]]) -> (f32, f32, f32) {
    let n = from.len().min(to.len());
    if n == 0 {
        return (0.0, 0.0, 0.0);
    }

    if n == 1 {
        let dx = to[0][0] - from[0][0];
        let dy = to[0][1] - from[0][1];
        let dist_sq = (px - from[0][0]) * (px - from[0][0]) + (py - from[0][1]) * (py - from[0][1]);
        let w = 1.0 / (dist_sq + 1.0);
        return (dx, dy, w);
    }

    let mut best_t = 0.0f32;
    let mut best_dist_sq = f32::MAX;
    let mut best_idx = 0usize;

    for i in 0..n - 1 {
        let ax = from[i][0];
        let ay = from[i][1];
        let bx = from[i + 1][0];
        let by = from[i + 1][1];

        let abx = bx - ax;
        let aby = by - ay;
        let apx = px - ax;
        let apy = py - ay;

        let ab_len_sq = abx * abx + aby * aby;
        if ab_len_sq <= 0.0 {
            continue;
        }

        let t = (apx * abx + apy * aby) / ab_len_sq;
        let tc = t.clamp(0.0, 1.0);

        let cx = ax + tc * abx;
        let cy = ay + tc * aby;

        let dist_sq = (px - cx) * (px - cx) + (py - cy) * (py - cy);

        if dist_sq < best_dist_sq {
            best_dist_sq = dist_sq;
            best_t = tc;
            best_idx = i;
        }
    }

    let seg_from_a = from[best_idx];
    let seg_from_b = from[(best_idx + 1).min(n - 1)];
    let seg_to_a = to[best_idx];
    let seg_to_b = to[(best_idx + 1).min(n - 1)];

    let src_x = seg_from_a[0] + best_t * (seg_from_b[0] - seg_from_a[0]);
    let src_y = seg_from_a[1] + best_t * (seg_from_b[1] - seg_from_a[1]);

    let dst_x = seg_to_a[0] + best_t * (seg_to_b[0] - seg_to_a[0]);
    let dst_y = seg_to_a[1] + best_t * (seg_to_b[1] - seg_to_a[1]);

    let dx = dst_x - src_x;
    let dy = dst_y - src_y;

    let w = 1.0 / (best_dist_sq + 1.0);

    (dx, dy, w)
}
