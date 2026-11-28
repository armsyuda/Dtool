use crate::selection::PixelSelection;
use crate::{Document, Object, ObjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerMask {
    pub object: ObjectId,
    pub coverage: PixelSelection,
    /// Original hard boundary in normalized mask coordinates, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_outline: Option<crate::curve::Curve>,
    /// Mask rectangle in normalized object coordinates. Omitted for the full object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<[f32; 4]>,
}

impl LayerMask {
    pub fn new(object: ObjectId, coverage: PixelSelection) -> Self {
        Self { object, coverage, vector_outline: None, frame: None }
    }

    pub fn relative_frame(&self) -> [f32; 4] {
        self.frame.filter(|rect| rect.iter().all(|value| value.is_finite())
            && rect[2] > 0.0 && rect[3] > 0.0).unwrap_or([0.0, 0.0, 1.0, 1.0])
    }

    /// Left, top, width and height before the object's rotation, shear and reflection.
    pub fn local_rect(&self, object: &Object) -> [f32; 4] {
        let [x, y, w, h] = self.relative_frame();
        let [width, height] = object.size.map(f32::abs);
        [x * width, y * height, w * width, h * height]
    }
}

/// Masks on raster sources are applied while decoding their pixels.
pub fn compositing_mask<'a>(document: &'a Document, object: &Object) -> Option<&'a PixelSelection> {
    compositing_layer_mask(document, object).map(|mask| &mask.coverage)
}

pub fn compositing_layer_mask<'a>(document: &'a Document, object: &Object) -> Option<&'a LayerMask> {
    if dimensions(document, object).is_some() {
        return None;
    }
    let mask = document.layer_masks.iter().find(|mask| mask.object == object.id)?;
    let length = (mask.coverage.width as usize).checked_mul(mask.coverage.height as usize)?;
    (length > 0 && length == mask.coverage.coverage.len()).then_some(mask)
}

pub fn dimensions(document: &Document, object: &Object) -> Option<[usize; 2]> {
    if let Some(layer) = document.pixel_layers.get(object.id) {
        return (layer.width > 0 && layer.height > 0).then_some([layer.width as usize, layer.height as usize]);
    }
    let (width, height) = object.raster_size?;
    let length = (width as usize).checked_mul(height as usize)?;
    let data = object.raster_data.as_ref()?;
    (length > 0 && (Some(data.len()) == length.checked_mul(3) || Some(data.len()) == length.checked_mul(4))).then_some([width as usize, height as usize])
}

pub fn rgba(document: &Document, object: &Object) -> Option<([usize; 2], Vec<u8>)> {
    let size = dimensions(document, object)?;
    let mut rgba = match document.pixel_layers.get(object.id) {
        Some(layer) => layer.to_rgba(),
        None => crate::scene::raster_rgba(object)?.1,
    };
    let length = size[0].checked_mul(size[1])?;
    if rgba.len() != length.checked_mul(4)? { return None; }
    if let Some(mask) = document.layer_masks.iter().find(|mask| mask.object == object.id) {
        if [mask.coverage.width as usize, mask.coverage.height as usize] != size || mask.coverage.coverage.len() != length { return None; }
        for (pixel, alpha) in rgba.chunks_exact_mut(4).zip(&mask.coverage.coverage) {
            pixel[3] = ((u16::from(pixel[3]) * u16::from(*alpha) + 127) / 255) as u8;
        }
    }
    crate::tone::apply(document, object, size, &mut rgba);
    Some((size, rgba))
}
