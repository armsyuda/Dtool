use crate::{Document, ObjectId, ObjectKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Content {
    #[default]
    Images,
    Shapes,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToneBlend {
    pub strength: f32,
    pub colour: f32,
    pub contrast: f32,
    pub low_pass: f32,
    pub content: Content,
    pub bands: [f32; 3],
}

impl Default for ToneBlend {
    fn default() -> Self {
        Self { strength: 1.0, colour: 1.0, contrast: 0.0, low_pass: 1.0, content: Content::Images, bands: [1.0; 3] }
    }
}

impl ToneBlend {
    pub fn valid(self) -> bool {
        (-1.0..=1.0).contains(&self.contrast)
            && [self.strength, self.colour, self.low_pass].into_iter().chain(self.bands)
                .all(|value| (0.0..=1.0).contains(&value))
    }

    pub fn opacity(self, source: &[u8], backdrop: &[u8], soft_tone: Option<f32>) -> f32 {
        let lum = luminance(source);
        let value = if backdrop[3] > 0 {
            let raw = luminance(backdrop);
            raw + self.low_pass * (soft_tone.unwrap_or(raw) - raw)
        } else {
            lum
        };
        let slope = if self.contrast >= 0.0 { 1.0 + 3.0 * self.contrast } else { 1.0 + self.contrast };
        let tone = ((value - 0.5) * slope + 0.5).clamp(0.0, 1.0);
        let [shadows, mids, highlights] = self.bands;
        let band = if tone <= 0.5 { shadows + (mids - shadows) * (2.0 * tone) }
            else { mids + (highlights - mids) * (2.0 * tone - 1.0) };
        let similarity = if self.colour > 0.0 && backdrop[3] > 0 {
            let under = luminance(backdrop);
            let distance = (0..3).map(|i| ((f32::from(source[i]) - f32::from(backdrop[i])) / 255.0 - (lum - under)).abs())
                .fold(0.0_f32, f32::max);
            1.0 - self.colour * distance.min(1.0)
        } else {
            1.0
        };
        (1.0 + self.strength * (band * similarity - 1.0)).clamp(0.0, 1.0)
    }
}

pub fn for_layer(doc: &Document, id: ObjectId) -> Option<(ObjectId, ToneBlend)> {
    let layer = doc.get_object(id)?;
    let kind = match layer.kind {
        ObjectKind::Image | ObjectKind::PictureFrame => Content::Images,
        ObjectKind::Text(_) | ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_) => Content::Text,
        ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Compound(_) => Content::Shapes,
        _ => return None,
    };
    let mut parent = layer.parent_id;
    for _ in 0..doc.objects.len() {
        let object = doc.get_object(parent?)?;
        if let Some(spec) = object.tone_blend.filter(|spec| spec.valid()) {
            return (spec.strength > 0.0 && spec.content == kind).then_some((object.id, spec));
        }
        parent = object.parent_id;
    }
    None
}

pub fn luminance(pixel: &[u8]) -> f32 {
    (0.2126 * f32::from(pixel[0]) + 0.7152 * f32::from(pixel[1]) + 0.0722 * f32::from(pixel[2])) / 255.0
}

pub fn softened_tone(backdrop: &[u8], size: [u32; 2], index: usize) -> Option<f32> {
    let [width, height] = size.map(|extent| extent as usize);
    let (x, y) = (index % width, index / width);
    let mut sum = 0.0;
    let mut coverage = 0.0;
    for row in y.saturating_sub(1)..=(y + 1).min(height - 1) {
        for col in x.saturating_sub(1)..=(x + 1).min(width - 1) {
            let offset = (row * width + col) * 4;
            let pixel = &backdrop[offset..offset + 4];
            let alpha = f32::from(pixel[3]);
            sum += alpha * luminance(pixel);
            coverage += alpha;
        }
    }
    (coverage > 0.0).then(|| sum / coverage)
}
