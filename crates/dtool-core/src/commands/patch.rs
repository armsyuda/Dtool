pub mod blemish;

use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::pixel::PixelLayer;
use crate::raster_view::RasterViewMut;
use crate::selection::{PixelSurface, polygon_mask};
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, Session};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const TOOL: &str = "tool.patch";
pub const APPLY: &str = "pixel.patch";
pub const SELECTION_IS_SOURCE: &str = "patch.selection_source";
pub const TEXTURE_ONLY: &str = "patch.texture_only";
pub const TRANSPARENT: &str = "patch.transparent";
pub const MAX_POINTS: usize = 2048;
const MAX_PIXELS: u64 = 4_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Area {
    Rect([f32; 4]),
    Polygon(Vec<[f32; 2]>),
}

#[derive(Deserialize)]
struct Args {
    area: Option<Area>,
    offset: [f32; 2],
    selection_is_source: Option<bool>,
    texture_only: Option<bool>,
    transparent: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub selection_is_source: bool,
    pub texture_only: bool,
    pub transparent: bool,
}

impl Options {
    pub fn current(session: &Session) -> Self {
        let flag = |key| session.tool_options.flag(TOOL, key).unwrap_or(false);
        Self { selection_is_source: flag(SELECTION_IS_SOURCE), texture_only: flag(TEXTURE_ONLY), transparent: flag(TRANSPARENT) }
    }
}

pub fn can_patch(session: &Session) -> bool {
    session.has_document && super::pixel_brush::clone_layer(session).is_some()
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == APPLY).then(|| can_patch(session))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    (command.name == APPLY).then(|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        let args: Args = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
        let defaults = Options::current(session);
        let options = Options {
            selection_is_source: args.selection_is_source.unwrap_or(defaults.selection_is_source),
            texture_only: args.texture_only.unwrap_or(defaults.texture_only),
            transparent: args.transparent.unwrap_or(defaults.transparent),
        };
        let prepared = Prepared::new(session, args.area.as_ref())?;
        let Some(document) = prepared.document(session, prepared.offset(args.offset)?, options, false) else {
            return Ok(json!({"changes": []}));
        };
        let selection = session.selection.ids.clone();
        super::document_change::commit(session, APPLY, document, selection, json!({"id": prepared.layer().0}))
    })
}

#[derive(Debug, Clone)]
pub struct Prepared {
    object: Object,
    pixels: PixelLayer,
    mask: crate::selection::PixelSelection,
    bounds: [u32; 4],
    matrix: [f32; 6],
    inverse: [f32; 6],
    scale: [f32; 2],
}

impl Prepared {
    pub fn new(session: &Session, area: Option<&Area>) -> Result<Self, CommandError> {
        let id = super::pixel_brush::clone_layer(session).ok_or_else(CommandError::invalid_params)?;
        let (object, pixels, _) = super::pixel_brush::edit_layer(session, Some(id), None, false)?
            .ok_or_else(CommandError::invalid_params)?;
        let matrix = object_matrix(&object);
        let inverse = invert_matrix(&matrix).ok_or_else(CommandError::invalid_params)?;
        let size = object.size.map(f32::abs);
        if size.iter().any(|side| !side.is_finite() || *side <= 0.0) || pixels.width == 0 || pixels.height == 0 {
            return Err(CommandError::invalid_params());
        }
        let scale = [pixels.width as f32 / size[0], pixels.height as f32 / size[1]];
        let selection = if area.is_none() {
            let mask = session.document.pixel_selection.as_ref().ok_or_else(CommandError::invalid_params)?;
            let surface = PixelSurface::for_page(&session.document, mask.page_id)
                .filter(|surface| surface.matches(&session.document, mask)).ok_or_else(CommandError::invalid_params)?;
            Some((mask, surface))
        } else { None };
        let points = match area {
            Some(Area::Rect(rect)) => corners(*rect).to_vec(),
            Some(Area::Polygon(points)) => points.clone(),
            None => {
                let (mask, surface) = selection.unwrap();
                let bounds = mask.bounds().ok_or_else(CommandError::invalid_params)?;
                corners(bounds.map(|value| value as f32)).map(|point| surface.pixel_to_doc(point)).to_vec()
            }
        };
        if !(3..=MAX_POINTS).contains(&points.len()) || points.iter().flatten().any(|value| !value.is_finite()) {
            return Err(CommandError::invalid_params());
        }
        let to_raster = |point| {
            let pos = apply_matrix(&inverse, point);
            std::array::from_fn(|axis| (pos[axis] - object.pos[axis]) * scale[axis])
        };
        let points: Vec<[f32; 2]> = points.into_iter().map(to_raster).collect();
        if points.iter().flatten().any(|value| !value.is_finite()) { return Err(CommandError::invalid_params()); }
        let [width, height] = [pixels.width, pixels.height];
        let bounds = |axis: usize, limit: u32| {
            let low = points.iter().map(|point| point[axis]).fold(f32::INFINITY, f32::min);
            let high = points.iter().map(|point| point[axis]).fold(f32::NEG_INFINITY, f32::max);
            [low.floor().clamp(0.0, limit as f32) as u32, high.ceil().clamp(0.0, limit as f32) as u32]
        };
        let [x0, x1] = bounds(0, width);
        let [y0, y1] = bounds(1, height);
        let [w, h] = [x1 - x0, y1 - y0];
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
            return Err(CommandError::invalid_params());
        }
        let mask = if let Some((mask, surface)) = selection {
            let coverage = (0..h).flat_map(|y| (0..w).map(move |x| {
                let local = [object.pos[0] + ((x0 + x) as f32 + 0.5) / scale[0],
                    object.pos[1] + ((y0 + y) as f32 + 0.5) / scale[1]];
                surface.coverage_at(mask, apply_matrix(&matrix, local))
            })).collect();
            crate::selection::PixelSelection::from_coverage(w, h, coverage)
        } else {
            let local: Vec<_> = points.iter().map(|[x, y]| [x - x0 as f32, y - y0 as f32]).collect();
            polygon_mask(w, h, &local, true)
        };
        if mask.is_empty() { return Err(CommandError::invalid_params()); }
        Ok(Self { object, pixels, mask, bounds: [x0, y0, w, h], matrix, inverse, scale })
    }

    pub fn layer(&self) -> ObjectId { self.object.id }

    pub fn offset(&self, delta: [f32; 2]) -> Result<[i64; 2], CommandError> {
        let [a, b, c, d, _, _] = self.inverse;
        let local = [(a * delta[0] + c * delta[1]) * self.scale[0], (b * delta[0] + d * delta[1]) * self.scale[1]];
        if local.iter().any(|value| !value.is_finite() || value.abs() > u32::MAX as f32) {
            return Err(CommandError::invalid_params());
        }
        Ok(local.map(|value| value.round() as i64))
    }

    pub fn contains(&self, point: [f32; 2]) -> bool {
        let local = apply_matrix(&self.inverse, point);
        let [x, y] = std::array::from_fn(|axis| {
            (local[axis] - self.object.pos[axis]) * self.scale[axis] - self.bounds[axis] as f32
        });
        x >= 0.0 && y >= 0.0 && self.mask.get(x as u32, y as u32) != 0
    }

    pub fn world_offset(&self, offset: [i64; 2]) -> [f32; 2] {
        let [a, b, c, d, _, _] = self.matrix;
        let [x, y] = std::array::from_fn(|axis| offset[axis] as f32 / self.scale[axis]);
        [a * x + c * y, b * x + d * y]
    }

    pub fn outline(&self, offset: [i64; 2]) -> [[f32; 2]; 4] {
        let mut bounds = self.bounds.map(|value| value as f32);
        bounds[0] += offset[0] as f32;
        bounds[1] += offset[1] as f32;
        corners(bounds).map(|point| {
            let local = std::array::from_fn(|axis| self.object.pos[axis] + point[axis] / self.scale[axis]);
            apply_matrix(&self.matrix, local)
        })
    }

    pub fn document(&self, session: &Session, offset: [i64; 2], options: Options, preview: bool) -> Option<Document> {
        if offset == [0; 2] { return None; }
        let [start_x, start_y, w, h] = self.bounds.map(i64::from);
        let origin = if options.selection_is_source { [start_x + offset[0], start_y + offset[1]] } else { [start_x, start_y] };
        let source = if options.selection_is_source { [start_x, start_y] } else { [start_x + offset[0], start_y + offset[1]] };
        let [left, top] = origin.map(|value| value.saturating_sub(1).max(0));
        let right = (origin[0] + w + 1).min(i64::from(self.pixels.width));
        let bottom = (origin[1] + h + 1).min(i64::from(self.pixels.height));
        if left >= right || top >= bottom { return None; }
        let region = [left as u32, top as u32, (right - left) as u32, (bottom - top) as u32];
        let before = self.pixels.read_region(region);
        let mut output = before.clone();
        let width = region[2] as usize;
        let coordinate = |index: usize| [left + (index % width) as i64 - origin[0], top + (index / width) as i64 - origin[1]];
        let mut sampled = Vec::with_capacity(before.len() / 4);
        let mut coverage = Vec::with_capacity(before.len() / 4);
        for (index, old) in before.as_chunks::<4>().0.iter().enumerate() {
            let [x, y] = coordinate(index);
            let alpha = if (0..w).contains(&x) && (0..h).contains(&y) { self.mask.get(x as u32, y as u32) } else { 0 };
            let [sx, sy] = [source[0] + x, source[1] + y];
            let mut ink = if (0..i64::from(self.pixels.width)).contains(&sx) && (0..i64::from(self.pixels.height)).contains(&sy) {
                self.pixels.get(sx as u32, sy as u32)
            } else { [0; 4] };
            if options.texture_only {
                let luma = luminance(ink);
                if preview {
                    let previous = luminance(*old);
                    for (channel, value) in ink[..3].iter_mut().enumerate() {
                        *value = (f32::from(old[channel]) + luma - previous).clamp(0.0, 255.0).round() as u8;
                    }
                } else {
                    ink[..3].fill(luma.round() as u8);
                }
            }
            let strength = f32::from(alpha) / 255.0 * if options.transparent { f32::from(ink[3]) / 255.0 } else { 1.0 };
            coverage.push(if ink[3] == 0 { 0.0 } else { strength });
            sampled.push(ink);
        }
        let solved = !preview && crate::brush::heal_region(
            &mut RasterViewMut::new(region[2], region[3], &mut output),
            &mut coverage.clone(), |index| sampled[index],
            (12_000_000 / coverage.len().max(1)).clamp(4, 200),
        );
        for (index, pixel) in output.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let strength = coverage[index];
            if strength == 0.0 || solved && pixel[3] != 0 { continue; }
            let ink = sampled[index];
            let base = f32::from(pixel[3]) / 255.0 * (1.0 - strength);
            let total = strength + base;
            for (into, from) in pixel[..3].iter_mut().zip(ink) {
                *into = ((f32::from(*into) * base + f32::from(from) * strength) / total).round() as u8;
            }
            pixel[3] = (255.0 * total).round() as u8;
        }
        if output == before { return None; }
        let mut pixels = self.pixels.clone();
        pixels.set_region(region, &output);
        let mut document = session.document.clone();
        super::pixel_brush::install_pixels(&mut document, &session.selection.ids, self.object.clone(), pixels, false);
        Some(document)
    }
}

fn corners([x, y, w, h]: [f32; 4]) -> [[f32; 2]; 4] {
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}

fn luminance([r, g, b, _]: [u8; 4]) -> f32 {
    0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)
}
