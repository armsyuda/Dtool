use crate::canvas_size;
use crate::pixel::PixelLayer;
use crate::{CommandError, Document};

pub const DOC_RESIZE_PIXEL_ART: &str = "doc.resize_pixel_art";
pub const DOC_RESIZE_PIXEL_ART_RESTORE: &str = "doc.resize_pixel_art.restore";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter { Hqx, Xbr }

pub fn options(params: &serde_json::Value) -> Result<(u32, Filter), CommandError> {
    let factor = match params.get("scale").and_then(|value| value.as_str()) {
        Some("2x") => 2,
        Some("3x") => 3,
        Some("4x") => 4,
        _ => return Err(CommandError::invalid_params()),
    };
    let filter = match params.get("filter").and_then(|value| value.as_str()) {
        Some("hqx") => Filter::Hqx,
        Some("xbr") => Filter::Xbr,
        _ => return Err(CommandError::invalid_params()),
    };
    Ok((factor, filter))
}

pub fn dimensions([width, height]: [u32; 2], factor: u32) -> Result<[u32; 2], CommandError> {
    const MAX_PIXELS: u64 = 32 * 1024 * 1024;
    let width = width.checked_mul(factor).filter(|width| *width > 0).ok_or_else(CommandError::invalid_params)?;
    let height = height.checked_mul(factor).filter(|height| *height > 0).ok_or_else(CommandError::invalid_params)?;
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    Ok([width, height])
}

pub(crate) fn resize(doc: &mut Document, factor: u32, filter: Filter) -> Result<[u32; 2], CommandError> {
    let (w, h) = canvas_size::document_pixel_size(doc)?;
    let size = dimensions([w, h], factor)?;
    let ratio = factor as f32;
    let page_sizes: Vec<_> = doc.pages.iter().map(|page| page.size.map(|side| side * ratio)).collect();
    canvas_size::scale_document(doc, ratio, ratio, page_sizes[0], doc.dpi)?;
    for (page, size) in doc.pages.iter_mut().zip(page_sizes) { page.size = size; }
    for id in doc.pixel_layers.ids() {
        let Some(layer) = doc.pixel_layers.get(id) else { continue; };
        let pixels = upscale(&layer.to_rgba(), [layer.width, layer.height], factor, filter)?;
        let (w, h) = (pixels.width, pixels.height);
        doc.pixel_layers.insert(id, pixels);
        if let Some(object) = doc.find_object_mut(id.0) {
            object.raster_data = None;
            object.raster_size = Some((w, h));
        }
    }
    for object in &mut doc.objects {
        if let (Some(data), Some((w, h))) = (&object.raster_data, object.raster_size) {
            let pixels = upscale(data, [w, h], factor, filter)?;
            object.raster_size = Some((pixels.width, pixels.height));
            object.raster_data = Some(pixels.to_rgba());
        }
    }
    for mask in &mut doc.layer_masks {
        mask.coverage = mask.coverage.magnify(factor)?;
    }
    if let Some(selection) = &mut doc.pixel_selection {
        *selection = selection.magnify(factor)?;
    }
    Ok(size)
}

type Rgba = [u8; 4];

fn distance(a: Rgba, b: Rgba) -> u32 {
    let [r, g, b_delta] = std::array::from_fn(|channel| {
        (i32::from(a[channel]) * i32::from(a[3]) - i32::from(b[channel]) * i32::from(b[3])) / 255
    });
    let y = (299 * r + 587 * g + 114 * b_delta).unsigned_abs();
    let u = (-169 * r - 331 * g + 500 * b_delta).unsigned_abs();
    let v = (500 * r - 419 * g - 81 * b_delta).unsigned_abs();
    (48 * y + 7 * u + 6 * v) / 1000 + 48 * u32::from(a[3].abs_diff(b[3]))
}

fn corner_decision(sample: impl Fn(i32, i32) -> Rgba, filter: Filter) -> Option<(Rgba, [u32; 2])> {
    let center = sample(0, 0);
    let right = sample(1, 0);
    let bottom = sample(0, 1);
    if center == right || center == bottom { return None; }

    let diagonal = sample(1, 1);
    let left = sample(-1, 0);
    let top = sample(0, -1);
    let upper_right = sample(1, -1);
    let lower_left = sample(-1, 1);
    let near = |a, b| distance(a, b) <= 600;
    if !near(center, left) && !near(center, top) && !near(center, upper_right) && !near(center, lower_left) {
        return None;
    }

    let slope = match filter {
        Filter::Hqx => {
            // Classify the immediate neighbourhood by perceptual colour, then
            // extend a supported corner using existing palette entries only.
            if !near(right, bottom) || near(right, left) || near(bottom, top) || near(center, diagonal) {
                return None;
            }
            [1, 1]
        }
        Filter::Xbr => {
            // Compare the two diagonal edge directions over a wider neighbourhood.
            let across = distance(center, upper_right) + distance(center, lower_left)
                + distance(diagonal, sample(2, 0)) + distance(diagonal, sample(0, 2))
                + 4 * distance(right, bottom);
            let along = distance(bottom, left) + distance(right, top)
                + distance(bottom, sample(1, 2)) + distance(right, sample(2, 1))
                + 4 * distance(center, diagonal);
            if across >= along { return None; }
            match (near(right, lower_left), near(bottom, upper_right)) {
                (true, false) => [1, 2],
                (false, true) => [2, 1],
                _ => [1, 1],
            }
        }
    };
    let chosen = if distance(center, right) <= distance(center, bottom) { right } else { bottom };
    Some((chosen, slope))
}

fn upscale(rgba: &[u8], [w, h]: [u32; 2], factor: u32, filter: Filter) -> Result<PixelLayer, CommandError> {
    let [width, height] = dimensions([w, h], factor)?;
    if rgba.len() != w as usize * h as usize * 4 { return Err(CommandError::invalid_params()); }
    let mut output = vec![0; width as usize * height as usize * 4];
    for y in 0..h {
        for x in 0..w {
            let pixel = |dx: i32, dy: i32| {
                let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                let i = (sy * w as usize + sx) * 4;
                [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
            };
            let center = pixel(0, 0);
            let corners = [(-1, -1), (1, -1), (-1, 1), (1, 1)].map(|(sx, sy)| {
                corner_decision(|dx, dy| pixel(dx * sx, dy * sy), filter)
            });
            for dy in 0..factor {
                for dx in 0..factor {
                    let right = dx >= factor / 2;
                    let bottom = dy >= factor / 2;
                    let corner = usize::from(right) + 2 * usize::from(bottom);
                    let colour = corners[corner].filter(|(_, [ax, ay])| {
                        let x = 1 + 2 * if right { factor - 1 - dx } else { dx };
                        let y = 1 + 2 * if bottom { factor - 1 - dy } else { dy };
                        x * ax + y * ay <= factor + ax + ay - 2
                    }).map_or(center, |(colour, _)| colour);
                    let i = ((y * factor + dy) as usize * width as usize + (x * factor + dx) as usize) * 4;
                    output[i..i + 4].copy_from_slice(&colour);
                }
            }
        }
    }
    Ok(PixelLayer::from_rgba(width, height, &output))
}
