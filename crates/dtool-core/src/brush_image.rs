use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

pub const MAX_STAMP_SIDE: u32 = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushImage {
    pub size: [u32; 2],
    #[serde(serialize_with = "encode", deserialize_with = "decode")]
    rgba: Arc<Vec<u8>>,
}

impl BrushImage {
    pub fn from_rgba(size: [u32; 2], rgba: Vec<u8>) -> Option<Self> {
        if !size.into_iter().all(|side| side > 0)
            || u64::from(size[0]) * u64::from(size[1]) != (rgba.len() / 4) as u64
            || rgba.len() % 4 != 0
        {
            return None;
        }
        let target = fit_stamp_size(size);
        let rgba = if target == size { rgba } else { resize(size, &rgba, target) };
        Some(Self { size: target, rgba: Arc::new(rgba) })
    }

    pub(crate) fn valid(&self) -> bool {
        self.size.into_iter().all(|side| (1..=MAX_STAMP_SIDE).contains(&side))
            && self.size[0] as usize * self.size[1] as usize * 4 == self.rgba.len()
            && self.rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0)
    }

    pub fn aspect(&self) -> [f32; 2] {
        let longest = self.size[0].max(self.size[1]).max(1) as f32;
        self.size.map(|side| side as f32 / longest)
    }

    pub(super) fn tiled_alpha(&self, uv: [f32; 2]) -> f32 {
        let [width, height] = self.size;
        let [x, y] = std::array::from_fn(|axis| uv[axis] * self.size[axis] as f32 - 0.5);
        let [left, top] = [x.floor(), y.floor()];
        let [u, v] = [x - left, y - top];
        let left = (left as i32).rem_euclid(width as i32) as u32;
        let top = (top as i32).rem_euclid(height as i32) as u32;
        let right = (left + 1) % width;
        let bottom = (top + 1) % height;
        let alpha = |x, y| f32::from(self.rgba[((y * width + x) * 4 + 3) as usize]) / 255.0;
        let upper = alpha(left, top) * (1.0 - u) + alpha(right, top) * u;
        let lower = alpha(left, bottom) * (1.0 - u) + alpha(right, bottom) * u;
        upper * (1.0 - v) + lower * v
    }

    pub(super) fn sample(&self, uv: [f32; 2]) -> Option<[u8; 4]> {
        if !uv.iter().all(|value| (0.0..=1.0).contains(value)) {
            return None;
        }
        let [width, height] = self.size;
        let [x, y] = std::array::from_fn(|axis| (uv[axis] * self.size[axis] as f32 - 0.5)
            .clamp(0.0, self.size[axis].saturating_sub(1) as f32));
        let [left, top] = [x.floor() as u32, y.floor() as u32];
        let [u, v] = [x.fract(), y.fract()];
        let mut value = [0.0; 4];
        for (row, wy) in [(top, 1.0 - v), ((top + 1).min(height - 1), v)] {
            for (col, wx) in [(left, 1.0 - u), ((left + 1).min(width - 1), u)] {
                sum_pixel(&mut value, &self.rgba, (row * width + col) as usize * 4, wy * wx);
            }
        }
        Some(straight(value, 1.0))
    }
}

pub(crate) fn fit_stamp_size(size: [u32; 2]) -> [u32; 2] {
    let long = size[0].max(size[1]);
    if long <= MAX_STAMP_SIDE {
        return size;
    }
    size.map(|side| (f64::from(side) * f64::from(MAX_STAMP_SIDE) / f64::from(long)).round().max(1.0) as u32)
}

fn encode<S: Serializer>(pixels: &Arc<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&STANDARD.encode(pixels.as_slice()))
}

fn decode<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<Vec<u8>>, D::Error> {
    let encoded = std::borrow::Cow::<str>::deserialize(deserializer)?;
    let max = MAX_STAMP_SIDE as usize * MAX_STAMP_SIDE as usize * 4;
    if encoded.len() > max.div_ceil(3) * 4 {
        return Err(serde::de::Error::custom("brush image exceeds the pixel limit"));
    }
    STANDARD.decode(encoded.as_bytes()).map(Arc::new).map_err(serde::de::Error::custom)
}

fn sum_pixel(sum: &mut [f32; 4], rgba: &[u8], offset: usize, weight: f32) {
    let alpha = f32::from(rgba[offset + 3]) * weight;
    for (channel, value) in sum[..3].iter_mut().enumerate() {
        *value += f32::from(rgba[offset + channel]) * alpha;
    }
    sum[3] += alpha;
}

fn straight(value: [f32; 4], weight: f32) -> [u8; 4] {
    if value[3] <= 0.0 { return [0; 4]; }
    std::array::from_fn(|channel| {
        (if channel == 3 { value[3] / weight } else { value[channel] / value[3] }).round() as u8
    })
}

fn resize([width, height]: [u32; 2], rgba: &[u8], [dest_width, dest_height]: [u32; 2]) -> Vec<u8> {
    let [sx, sy] = [width as f64 / dest_width as f64, height as f64 / dest_height as f64];
    let mut out = Vec::with_capacity(dest_width as usize * dest_height as usize * 4);
    for y in 0..dest_height {
        let [top, bottom] = [y as f64 * sy, (y + 1) as f64 * sy];
        for x in 0..dest_width {
            let [left, right] = [x as f64 * sx, (x + 1) as f64 * sx];
            let mut value = [0.0; 4];
            for row in top.floor() as u32..(bottom.ceil() as u32).min(height) {
                let wy = bottom.min((row + 1) as f64) - top.max(row as f64);
                for col in left.floor() as u32..(right.ceil() as u32).min(width) {
                    let wx = right.min((col + 1) as f64) - left.max(col as f64);
                    sum_pixel(&mut value, rgba, (row as usize * width as usize + col as usize) * 4, (wy * wx) as f32);
                }
            }
            out.extend(straight(value, (sx * sy) as f32));
        }
    }
    out
}
