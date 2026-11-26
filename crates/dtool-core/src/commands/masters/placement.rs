use serde::{Deserialize, Serialize};

use crate::command::{anchor_fraction, transform_group_member};
use crate::geometry::{apply_matrix, box_centre, compose, object_linear};
use crate::paint::{apply_transform_to_paint, apply_transform_to_stroke, transform_transparency_matrix};
use crate::transform::Anchor;
use crate::{CommandError, Object};

pub(super) fn project_picture_frame(
    doc: &crate::Document, source: &Object, size: [f32; 2], frame: &mut crate::commands::picture_frame::PictureFrame,
) {
    use crate::commands::picture_frame::{fitted_size, frame_image_size};
    let original = source.size.map(f32::abs);
    let size = size.map(f32::abs);
    if original == size || original.iter().chain(&size).any(|side| !side.is_finite() || *side <= 0.0) {
        return;
    }
    let factors = [size[0] / original[0], size[1] / original[1]];
    for (offset, factor) in frame.content.offset.iter_mut().zip(factors) {
        *offset *= factor;
    }
    let old_size = frame.manual_layout.map_or(original, |layout| layout.fit_size);
    if let Some(layout) = &mut frame.manual_layout {
        for (axis, factor) in factors.into_iter().enumerate() {
            layout.fit_size[axis] *= factor;
            layout.anchor_size[axis] *= factor;
        }
    }
    if let Some(image) = frame_image_size(doc, source, frame) {
        let new_size = frame.manual_layout.map_or(size, |layout| layout.fit_size);
        let old = fitted_size(old_size, image, frame.fit);
        let new = fitted_size(new_size, image, frame.fit);
        for axis in 0..2 {
            if new[axis].is_finite() && new[axis] > 0.0 {
                frame.content.scale[axis] *= factors[axis] * old[axis] / new[axis];
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    #[default]
    None,
    Stretch,
    Fit,
    Fill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MasterPlacement {
    pub scale: ScaleMode,
    pub anchor: Anchor,
    pub scale_strokes: bool,
}

impl Default for MasterPlacement {
    fn default() -> Self {
        Self { scale: ScaleMode::None, anchor: Anchor::TopLeft, scale_strokes: true }
    }
}

impl MasterPlacement {
    pub(super) fn is_default(&self) -> bool { *self == Self::default() }

    pub(super) fn projection(self, root: &Object, destination: [f32; 4]) -> Option<Projection> {
        let [w, h] = root.size;
        let [x, y, width, height] = destination;
        if [w, h, width, height].iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return None;
        }
        let [u, v] = [width / w, height / h];
        let [sx, sy] = match self.scale {
            ScaleMode::None => [1.0; 2],
            ScaleMode::Stretch => [u, v],
            ScaleMode::Fit => [u.min(v); 2],
            ScaleMode::Fill => [u.max(v); 2],
        };
        let [ax, ay] = anchor_fraction(self.anchor);
        let dx = x + (width - w * sx) * ax - root.pos[0] * sx;
        let dy = y + (height - h * sy) * ay - root.pos[1] * sy;
        let matrix = [sx, 0.0, 0.0, sy, dx, dy];
        (sx > 0.0 && sy > 0.0 && matrix.iter().all(|value| value.is_finite()))
            .then_some(Projection { matrix, scale_strokes: self.scale_strokes })
    }

    pub(crate) fn guide_offset(self, root: &Object, bounds: [f32; 4], axis: usize, position: f32) -> Option<f32> {
        if self.is_default() {
            return Some(position);
        }
        let projection = self.projection(root, bounds)?;
        let mut point = root.pos;
        point[axis] += position;
        Some(apply_matrix(&projection.matrix, point)[axis] - bounds[axis])
    }
}

pub(super) fn layer_matrix(group: &Object, basis: [f32; 2], origin: [f32; 2]) -> Option<[f32; 6]> {
    let [width, height] = basis;
    let [sx, sy] = [group.size[0] / width, group.size[1] / height];
    let [a, b, c, d] = object_linear(group.rotation_deg, group.shear_deg);
    let linear = [a * sx, b * sx, c * sy, d * sy, 0.0, 0.0];
    let reference = apply_matrix(&linear, [width * 0.5, height * 0.5]);
    let local_centre = std::array::from_fn::<_, 2, _>(|axis|
        group.pos[axis] - origin[axis] + group.size[axis].abs() * 0.5);
    let matrix = [linear[0], linear[1], linear[2], linear[3],
        local_centre[0] - reference[0], local_centre[1] - reference[1]];
    (matrix != [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] && crate::geometry::invert_matrix(&matrix).is_some())
        .then_some(matrix)
}

#[derive(Clone, Copy)]
pub(super) struct Projection {
    matrix: [f32; 6],
    scale_strokes: bool,
}

impl Projection {
    pub(super) fn with_layer(mut self, mut layer: [f32; 6], origin: [f32; 2]) -> Self {
        let [a, b, c, d, dx, dy] = layer;
        layer[4] = dx + (1.0 - a) * origin[0] - c * origin[1];
        layer[5] = dy - b * origin[0] + (1.0 - d) * origin[1];
        self.matrix = compose(&layer, &self.matrix);
        self
    }

    pub(super) fn position(self, object: &Object) -> [f32; 2] {
        if self.matrix[..4] == [1.0, 0.0, 0.0, 1.0] {
            return apply_matrix(&self.matrix, object.pos);
        }
        let [a, b, c, d] = object_linear(object.rotation_deg, object.shear_deg);
        let [a, b, c, d, _, _] = compose(&self.matrix, &[a, b, c, d, 0.0, 0.0]);
        let x_scale = a.hypot(b);
        let y_scale = (a * d - b * c) / x_scale;
        let [x, y] = apply_matrix(&self.matrix, box_centre(object));
        [x - (object.size[0] * x_scale).abs() * 0.5, y - (object.size[1] * y_scale).abs() * 0.5]
    }

    pub(super) fn object(self, source: &Object) -> Result<Object, CommandError> {
        let (mut object, [sx, sy]) = transform_group_member(source, &self.matrix, source.is_path_text(), false)?;
        if [sx, sy] == [1.0, 1.0] {
            return Ok(object);
        }
        for fill in std::iter::once(&mut object.fill).chain(&mut object.additional_fills) {
            apply_transform_to_paint(fill, sx, sy, 0.0);
        }
        for stroke in std::iter::once(&mut object.stroke).chain(&mut object.additional_strokes) {
            apply_transform_to_stroke(stroke, sx, sy, 0.0);
            if self.scale_strokes && stroke.scale_with_object {
                let factor = (sx * sy).sqrt() as f32;
                stroke.width *= factor;
                stroke.dash.iter_mut().for_each(|length| *length *= factor);
                stroke.dash_offset *= factor;
            }
        }
        transform_transparency_matrix(&mut object.transparency, [sx, 0.0, 0.0, sy, 0.0, 0.0]);
        if let Some(crop) = &mut object.vector_crop {
            crop.rect[0] *= sx as f32;
            crop.rect[1] *= sy as f32;
            crop.rect[2] *= sx as f32;
            crop.rect[3] *= sy as f32;
        }
        Ok(object)
    }
}
