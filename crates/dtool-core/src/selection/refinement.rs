use super::{brush_mask, PixelSelection, RefineSettings, SelectionOp};
use crate::raster_view::RasterView;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefineBrush {
    #[default]
    Matte,
    Foreground,
    Background,
    Feather,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefineStroke {
    pub brush: RefineBrush,
    pub width: f32,
    pub points: Vec<[f32; 3]>,
}

pub fn refine(
    selection: &PixelSelection, source: &RasterView, settings: &RefineSettings,
    strokes: &[RefineStroke], edge_shift: f32,
) -> PixelSelection {
    let (width, height) = (selection.width, selection.height);
    let mut matte: Option<PixelSelection> = None;
    for stroke in strokes.iter().filter(|stroke| stroke.brush == RefineBrush::Matte) {
        let mask = brush_mask(width, height, &stroke.points, stroke.width, 1.0);
        if let Some(matte) = &mut matte { matte.combine(&mask, SelectionOp::Add); }
        else { matte = Some(mask); }
    }
    let mut result = selection.clone();
    if (settings.border_width > 0.0 || matte.is_some()) && source.width == width && source.height == height
        && source.rgba.len() == selection.coverage.len() * 4
    {
        refine_band(selection, source, settings.border_width, matte.as_ref(), &mut result);
    }
    if settings.smooth > 0.0 {
        blur(&mut result, settings.smooth);
        result.coverage.iter_mut().for_each(|value| *value = if *value >= 128 { 255 } else { 0 });
    }
    if settings.feather > 0.0 { blur(&mut result, settings.feather); }
    if settings.ramp != 1.0 || edge_shift != 0.0 {
        for coverage in result.coverage.iter_mut().filter(|value| **value > 0 && **value < 255) {
            let value = f32::from(*coverage) / 255.0;
            *coverage = ((value.powf(settings.ramp) + edge_shift).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    for stroke in strokes.iter().filter(|stroke| stroke.brush != RefineBrush::Matte) {
        let mask = brush_mask(width, height, &stroke.points, stroke.width, 0.8);
        match stroke.brush {
            RefineBrush::Foreground => result.combine(&mask, SelectionOp::Add),
            RefineBrush::Background => result.combine(&mask, SelectionOp::Subtract),
            RefineBrush::Feather => {
                let mut softened = result.clone();
                blur(&mut softened, (stroke.width * 0.2).max(1.0));
                for ((alpha, softened), amount) in result.coverage.iter_mut().zip(softened.coverage).zip(mask.coverage) {
                    *alpha = blend(*alpha, softened, amount);
                }
            }
            RefineBrush::Matte => {}
        }
    }
    if result.coverage != selection.coverage { result.clear_outside_canvas(); }
    result
}

fn refine_band(
    selection: &PixelSelection, source: &RasterView, border: f32,
    matte: Option<&PixelSelection>, result: &mut PixelSelection,
) {
    let Some([x, y, w, h]) = selection.bounds() else { return; };
    let radius = (border * 0.5).ceil().max(1.0) as i32;
    let directions = [[1, 0], [0, 1], [-1, 0], [0, -1], [1, 1], [-1, 1], [1, -1], [-1, -1]];
    let padding = radius as u32;
    let [mut min_x, mut min_y] = [x.saturating_sub(padding), y.saturating_sub(padding)];
    let [mut max_x, mut max_y] = [(x + w + padding).min(selection.width), (y + h + padding).min(selection.height)];
    if let Some([x, y, w, h]) = matte.and_then(PixelSelection::bounds) {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + w);
        max_y = max_y.max(y + h);
    }
    for y in min_y..max_y {
        for x in min_x..max_x {
            let alpha = selection.get(x, y);
            let dab = matte.is_some_and(|mask| mask.get(x, y) != 0);
            if !dab && border <= 0.0 { continue; }
            let search = if dab { radius.max(12) } else { radius };
            let offset_at = |dx: i32, dy: i32, distance: i32| {
                let nx = (x as i32 + dx * distance).clamp(0, selection.width as i32 - 1) as u32;
                let ny = (y as i32 + dy * distance).clamp(0, selection.height as i32 - 1) as u32;
                (nx, ny)
            };
            if !dab && matches!(alpha, 0 | 255) && directions.iter().all(|&[dx, dy]| {
                let (nx, ny) = offset_at(dx, dy, search);
                selection.get(nx, ny) == alpha
            }) { continue; }
            let Some(colour) = source.pixel(x, y) else { continue; };
            let mut foreground = u32::MAX;
            let mut background = u32::MAX;
            let mut nearest_foreground = None;
            let mut nearest_background = None;
            for [dx, dy] in directions {
                for distance in [search * 2, search, (search / 2).max(1), 1] {
                    let (nx, ny) = offset_at(dx, dy, distance);
                    let certainty = selection.get(nx, ny);
                    if certainty < 250 && certainty > 5 { continue; }
                    let inset = (search / 3).max(1) as u32;
                    let sure = |x: u32, y: u32| {
                        let coverage = selection.get(x, y);
                        if certainty >= 250 { coverage >= 250 } else { coverage <= 5 }
                    };
                    if !sure(nx.saturating_sub(inset), ny) || !sure((nx + inset).min(selection.width - 1), ny)
                        || !sure(nx, ny.saturating_sub(inset)) || !sure(nx, (ny + inset).min(selection.height - 1))
                    {
                        continue;
                    }
                    let Some(sample) = source.pixel(nx, ny) else { continue; };
                    let distance_to_colour = colour_distance(colour, sample);
                    if certainty >= 250 && distance_to_colour < foreground {
                        foreground = distance_to_colour;
                        nearest_foreground = Some(sample);
                    } else if certainty <= 5 && distance_to_colour < background {
                        background = distance_to_colour;
                        nearest_background = Some(sample);
                    }
                }
            }
            if let (Some(fg), Some(bg)) = (nearest_foreground, nearest_background)
                && colour_distance(fg, bg) > 256 && foreground + background > 0
            {
                let alpha = (background * 255 + (foreground + background) / 2) / (foreground + background);
                result.coverage[y as usize * result.width as usize + x as usize] = alpha as u8;
            }
        }
    }
}

fn colour_distance(a: [u8; 4], b: [u8; 4]) -> u32 {
    let premultiplied = |pixel: [u8; 4], channel: usize| {
        if channel == 3 { i32::from(pixel[3]) }
        else { i32::from(pixel[channel]) * i32::from(pixel[3]) / 255 }
    };
    (0..4).map(|channel| (premultiplied(a, channel) - premultiplied(b, channel)).pow(2) as u32).sum()
}

fn blend(from: u8, to: u8, amount: u8) -> u8 {
    let amount = u32::from(amount);
    ((u32::from(from) * (255 - amount) + u32::from(to) * amount + 127) / 255) as u8
}

fn blur(selection: &mut PixelSelection, radius: f32) {
    let width = selection.width as usize;
    let height = selection.height as usize;
    if width == 0 || height == 0 { return; }
    let radius = (radius * 0.5).ceil() as usize;
    let mut temporary = vec![0; selection.coverage.len()];
    for y in 0..height {
        blur_line(&selection.coverage, &mut temporary, y * width, 1, width, radius);
    }
    for x in 0..width {
        blur_line(&temporary, &mut selection.coverage, x, width, height, radius);
    }
}

fn blur_line(src: &[u8], dest: &mut [u8], offset: usize, stride: usize, length: usize, radius: usize) {
    let divisor = (2 * radius + 1) as u32;
    let at = |position: usize| u32::from(src[offset + position.min(length - 1) * stride]);
    let mut total = at(0) * radius as u32 + (0..=radius).map(at).sum::<u32>();
    for position in 0..length {
        dest[offset + position * stride] = ((total + divisor / 2) / divisor) as u8;
        total += at(position + radius + 1);
        total -= at(position.saturating_sub(radius));
    }
}
