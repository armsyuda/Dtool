use crate::{Document, Object, ObjectKind, geometry, paint::{Cap, Colour, Paint}, scene};
use crate::resources::{ImageFormat, ImageResource};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureMode {
    Intensity,
    Image,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Texture {
    pub mode: TextureMode,
    pub resource_id: String,
}

pub(crate) fn resource<'a>(doc: &'a Document, reference: &str) -> Option<&'a ImageResource> {
    doc.resources.iter().find(|resource| resource.id.0.to_string() == reference)
        .or_else(|| doc.resources.iter().find(|resource| !resource.name.is_empty() && resource.name == reference))
}

pub(crate) fn source_pixels(doc: &Document, reference: &str) -> Option<([usize; 2], Vec<u8>)> {
    match resource(doc, reference) {
        Some(image) => resource_pixels(doc, image),
        None => scene::bitmap_source(doc, reference),
    }
}

pub(crate) fn resource_pixels(doc: &Document, resource: &ImageResource) -> Option<([usize; 2], Vec<u8>)> {
    if let Some(pixels) = doc.resource_uses(resource.id).into_iter().filter_map(|id| doc.get_object(id))
        .find_map(|object| crate::raster_source::rgba(doc, object))
    {
        return Some(pixels);
    }
    let data = resource.data.as_deref()?;
    let size = resource.pixel_size.map(|side| side as usize);
    let pixel_bytes = size[0].checked_mul(size[1])?.checked_mul(4)?;
    if pixel_bytes == 0 {
        return None;
    }
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") && data.len() == pixel_bytes {
        return Some((size, data.to_vec()));
    }
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if [info.width, info.height] != resource.pixel_size {
        return None;
    }
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut pixels).ok()?;
    if [frame.width, frame.height] != resource.pixel_size || frame.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let rgba = match frame.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => pixels.into_iter().flat_map(|v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == pixel_bytes).then_some((size, rgba))
}

pub(crate) fn snapshot_resource(doc: &Document, reference: &str) -> Option<ImageResource> {
    let (size, pixels) = source_pixels(doc, reference)?;
    let source = resource(doc, reference);
    let mut copy = encode_resource(size, &pixels, source.map_or(72.0, |image| image.dpi))?;
    if let Some(source) = source {
        copy.name = source.name.clone();
        copy.colour_space = source.colour_space.clone();
        copy.icc = source.icc.clone();
    }
    Some(copy)
}

pub(crate) fn import_snapshot(doc: &mut Document, image: &ImageResource) -> crate::resources::ResourceId {
    let matching = doc.resources.iter().find(|existing| {
        existing.embedded && existing.pixel_size == image.pixel_size && existing.data == image.data
            && existing.icc == image.icc && existing.colour_space == image.colour_space
            && !doc.placed_images.iter().any(|placed| placed.resource == existing.id)
    });
    match matching {
        Some(image) => image.id,
        None => doc.add_image_resource(image.clone()),
    }
}

fn encode_resource(size: [usize; 2], pixels: &[u8], dpi: f32) -> Option<ImageResource> {
    let size = [u32::try_from(size[0]).ok()?, u32::try_from(size[1]).ok()?];
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(pixels).ok()?;
    writer.finish().ok()?;
    Some(ImageResource::new_embedded(ImageFormat::Png, size, dpi, bytes))
}

const MAX_PIXELS: f32 = 4_000_000.0;
const MAX_SIDE: f32 = 4096.0;

#[derive(Clone, Copy)]
struct Segment {
    start: [f32; 2],
    direction: [f32; 2],
    length: f32,
    offset: f32,
    first: bool,
    last: bool,
}

pub struct Ink {
    pub bounds: [f32; 4],
    segments: Vec<Segment>,
    size: [usize; 2],
    rgba: Vec<u8>,
    diameter: f32,
    cap: Cap,
    mode: TextureMode,
    tint: Colour,
}

impl Ink {
    pub fn new(doc: &Document, object: &Object) -> Option<Self> {
        let texture = object.path_brush.as_ref()?;
        let radius = object.stroke.width * 0.5;
        if !matches!(object.kind, ObjectKind::Curve(_)) || !radius.is_finite() || radius <= 0.0
            || object.stroke.paint == Paint::None
            || !object.hidden_appearance.is_visible(crate::commands::appearance::PaintSlot {
                kind: crate::commands::appearance::Kind::Stroke, index: 0,
            })
        {
            return None;
        }
        let matrix = geometry::object_matrix(object);
        let contours = scene::warped_contours(doc, object, &matrix)
            .unwrap_or_else(|| scene::doc_contours(object, &matrix));
        let mut segments = Vec::new();
        for (mut points, closed) in contours {
            points.dedup();
            if closed && points.first() == points.last() {
                points.pop();
            }
            if points.len() < 2 {
                continue;
            }
            if closed {
                points.push(points[0]);
            }
            let mut offset = 0.0;
            let last = points.len() - 2;
            for (index, pair) in points.windows(2).enumerate() {
                let dx = pair[1][0] - pair[0][0];
                let dy = pair[1][1] - pair[0][1];
                let length = dx.hypot(dy);
                if !length.is_finite() || length <= f32::EPSILON {
                    continue;
                }
                segments.push(Segment {
                    start: pair[0], direction: [dx / length, dy / length], length, offset,
                    first: !closed && index == 0, last: !closed && index == last,
                });
                offset += length;
            }
        }
        let ends = segments.iter().flat_map(|segment| {
            [segment.start, [
                segment.start[0] + segment.direction[0] * segment.length,
                segment.start[1] + segment.direction[1] * segment.length,
            ]]
        });
        let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for point in ends {
            for axis in 0..2 {
                min[axis] = min[axis].min(point[axis]);
                max[axis] = max[axis].max(point[axis]);
            }
        }
        let padding = if object.stroke.cap == Cap::Square { radius * std::f32::consts::SQRT_2 } else { radius };
        let bounds = [min[0] - padding, min[1] - padding, max[0] - min[0] + 2.0 * padding, max[1] - min[1] + 2.0 * padding];
        if segments.is_empty() || !bounds.iter().all(|v| v.is_finite()) {
            return None;
        }
        let (size, rgba) = source_pixels(doc, &texture.resource_id)?;
        let tint = scene::representative_colour(&object.stroke.paint).unwrap_or(Colour::new(0, 0, 0, 255));
        Some(Self { bounds, segments, size, rgba, diameter: radius * 2.0, cap: object.stroke.cap, mode: texture.mode, tint })
    }

    pub fn hit(&self, point: [f32; 2], tolerance: f32) -> bool {
        let radius = (self.diameter * 0.5).max(tolerance);
        self.segments.iter().any(|segment| {
            let [dx, dy] = segment.direction;
            let x = point[0] - segment.start[0];
            let y = point[1] - segment.start[1];
            let along = x * dx + y * dy;
            let across = -x * dy + y * dx;
            let cap = |end| if end { self.cap } else { Cap::Round };
            let (overhang, cap) = if along < 0.0 {
                (-along, cap(segment.first))
            } else {
                ((along - segment.length).max(0.0), cap(segment.last))
            };
            match cap {
                Cap::Butt => overhang == 0.0 && across.abs() <= radius,
                Cap::Round => overhang.hypot(across) <= radius,
                Cap::Square => overhang <= radius && across.abs() <= radius,
            }
        })
    }

    pub fn quad(&self) -> [[f32; 2]; 4] {
        let [x, y, w, h] = self.bounds;
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
    }

    pub fn pixel_size(&self, requested_scale: f32) -> [usize; 2] {
        let [_, _, width, height] = self.bounds;
        pixel_size([width, height], requested_scale)
    }

    pub fn render_clipped(&self, doc: &Document, object: crate::ObjectId, size: [usize; 2]) -> Vec<u8> {
        let mut rgba = self.render(size);
        let Some(clip) = crate::clip_mask::clip_region(doc, object) else { return rgba };
        let contours: Vec<Vec<_>> = clip.into_iter()
            .map(|ring| ring.into_iter().map(|point| point.map(|axis| axis as f32)).collect())
            .collect();
        let [x, y, w, h] = self.bounds;
        let [width, height] = size;
        for (index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            let px = x + ((index % width) as f32 + 0.5) * w / width as f32;
            let py = y + ((index / width) as f32 + 0.5) * h / height as f32;
            if pixel[3] != 0 && !scene::inside(&contours, px, py) {
                pixel[3] = 0;
            }
        }
        rgba
    }

    pub(crate) fn expanded_fill(&self, doc: &mut Document, origin: [f32; 2]) -> Option<Paint> {
        let dpi = crate::effect_raster::pdf_dpi(doc);
        let scale = dpi / 72.0 / crate::units::units_per_point(doc.units, doc.dpi);
        let size = self.pixel_size(scale);
        let [x, y, w, h] = self.bounds;
        let [dx, dy] = [f64::from(w) / size[0] as f64, f64::from(h) / size[1] as f64];
        // Expanded strokes use a tiling bitmap fill. Keep the adjacent tile's
        // ink outside the outline even when a viewer samples across its edge.
        let border = 2;
        let padded = size.map(|side| side + 2 * border);
        let mut pixels = vec![0; padded[0] * padded[1] * 4];
        for (row, source) in self.render(size).chunks_exact(size[0] * 4).enumerate() {
            let start = ((row + border) * padded[0] + border) * 4;
            pixels[start..start + source.len()].copy_from_slice(source);
        }
        let mut resource = encode_resource(padded, &pixels, dpi)?;
        resource.colour_space = "RGB".into();
        let id = doc.add_image_resource(resource);
        Some(Paint::Bitmap(crate::paint::BitmapPaint {
            resource_id: id.0.to_string(),
            matrix: [dx, 0.0, 0.0, dy,
                f64::from(x - origin[0]) - dx * border as f64, f64::from(y - origin[1]) - dy * border as f64],
            seed: None,
        }))
    }

    pub fn render(&self, size: [usize; 2]) -> Vec<u8> {
        let [x, y, width, height] = self.bounds;
        let step = [width / size[0] as f32, height / size[1] as f32];
        let radius = self.diameter * 0.5;
        let radius_squared = radius * radius;
        let padding = if self.cap == Cap::Square { radius * std::f32::consts::SQRT_2 } else { radius };
        let mut rgba = vec![0; size[0] * size[1] * 4];
        let mut distances = vec![f32::INFINITY; size[0] * size[1]];
        for segment in &self.segments {
            let [sx, sy] = segment.start;
            let [dx, dy] = segment.direction;
            let end = [sx + dx * segment.length, sy + dy * segment.length];
            let window = |axis: usize, origin: f32| {
                let start = ((segment.start[axis].min(end[axis]) - padding - origin) / step[axis]).floor() as usize;
                let finish = ((segment.start[axis].max(end[axis]) + padding - origin) / step[axis]).ceil() as usize;
                start.min(size[axis])..finish.min(size[axis])
            };
            let cols = window(0, x);
            for py in window(1, y) {
                for px in cols.clone() {
                    let [vx, vy] = [x + (px as f32 + 0.5) * step[0] - sx, y + (py as f32 + 0.5) * step[1] - sy];
                    let along = vx * dx + vy * dy;
                    let across = -vx * dy + vy * dx;
                    let at_end = (segment.first && along < 0.0) || (segment.last && along > segment.length);
                    let distance = match (at_end, self.cap) {
                        (true, Cap::Butt) => continue,
                        (true, Cap::Square) if along >= -radius && along <= segment.length + radius => across * across,
                        _ => across * across + (along - along.clamp(0.0, segment.length)).powi(2),
                    };
                    let index = py * size[0] + px;
                    if distance <= radius_squared && distance < distances[index] {
                        distances[index] = distance;
                        rgba[index * 4..index * 4 + 4].copy_from_slice(&self.sample(segment.offset + along.clamp(0.0, segment.length), across));
                    }
                }
            }
        }
        rgba
    }

    fn sample(&self, along: f32, across: f32) -> [u8; 4] {
        let [w, h] = self.size;
        let unit = h as f32 / self.diameter;
        let col = ((along * unit).floor() as usize) % w;
        let row = (((across / self.diameter + 0.5) * h as f32).floor() as usize).min(h - 1);
        let offset = (row * w + col) * 4;
        let pixel = &self.rgba[offset..offset + 4];
        match self.mode {
            TextureMode::Image => [pixel[0], pixel[1], pixel[2], pixel[3]],
            TextureMode::Intensity => {
                let luminance = (f32::from(pixel[0]) * 0.2126 + f32::from(pixel[1]) * 0.7152 + f32::from(pixel[2]) * 0.0722) / 255.0;
                let alpha = f32::from(pixel[3]) * luminance * f32::from(self.tint.a) / 255.0;
                [self.tint.r, self.tint.g, self.tint.b, alpha.round() as u8]
            }
        }
    }
}

pub fn pixel_size([width, height]: [f32; 2], requested_scale: f32) -> [usize; 2] {
    let scale = requested_scale.max(0.01).min(MAX_SIDE / width.max(height))
        .min((MAX_PIXELS / (width * height)).sqrt());
    [width, height].map(|side| (side * scale).ceil().max(1.0) as usize)
}
