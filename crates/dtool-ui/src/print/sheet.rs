use std::sync::OnceLock;

use dtool_core::{Document, ObjectId};
use dtool_export::{pdf::{fold_mark_segments, trim_mark_segments}, raster::document_unit_points};
use resvg::{tiny_skia, usvg};

pub(super) type Region = (f32, f32, f32, f32);

#[derive(Clone, Copy, PartialEq)]
pub(super) enum PrintContent {
    Pages { first: usize, last: usize },
    Artboard(ObjectId),
}

impl PrintContent {
    pub fn pages(self) -> Option<Vec<usize>> {
        match self {
            Self::Pages { first, last } => Some((first..=last).collect()),
            Self::Artboard(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct PrintRegion {
    pub bounds: Region,
    pub content: PrintContent,
    pub number: usize,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct PrintMarks {
    pub crop: bool,
    pub fold: bool,
    pub registration: bool,
    pub color_bars: bool,
    pub page_info: Option<(usize, usize)>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Sheet {
    pub body: Region,
    pub media: Region,
    pub content: PrintContent,
    trim: Region,
    bleed: [f32; 4],
    marks: PrintMarks,
    units_per_point: f32,
}

impl Sheet {
    pub fn can_print_fills_as_vectors(self) -> bool {
        !matches!(self.content, PrintContent::Artboard(_)) && self.marks == PrintMarks::default() && self.body == self.trim
    }

    pub fn new(doc: &Document, region: PrintRegion, include_bleed: bool, mut marks: PrintMarks) -> Option<Self> {
        let bleed = &doc.setup.bleed;
        let valid = |value: f32| {
            if include_bleed && bleed.enabled && value.is_finite() { value.max(0.0) } else { 0.0 }
        };
        let [left, top, right, bottom] = [bleed.left, bleed.top, bleed.right, bleed.bottom].map(valid);
        let (x, y, w, h) = region.bounds;
        let body = (x - left, y - top, w + left + right, h + top + bottom);
        let units_per_point = 1.0 / document_unit_points(doc);
        if marks.fold {
            marks.fold = region.content.pages().is_some_and(|pages| {
                !fold_mark_segments(doc, &pages, extents(region.bounds), extents(body), units_per_point).is_empty()
            });
        }
        let margin = if marks.crop || marks.fold || marks.registration { 18.0 * units_per_point } else { 0.0 };
        let top_margin = if marks.color_bars { 26.0 * units_per_point } else { margin };
        let bottom_margin = if marks.page_info.is_some() { 28.0 * units_per_point } else { margin };
        let media = (body.0 - margin, body.1 - top_margin, body.2 + 2.0 * margin, body.3 + top_margin + bottom_margin);
        if ![media.0, media.1, media.2, media.3, media.0 + media.2, media.1 + media.3]
            .into_iter().all(f32::is_finite) || w <= 0.0 || h <= 0.0
        {
            return None;
        }
        Some(Self { body, media, content: region.content, trim: region.bounds, bleed: [left, top, right, bottom], marks, units_per_point })
    }

    pub fn render(self, doc: &Document, title: &str, size: [usize; 2]) -> Option<Vec<u8>> {
        if self.marks == PrintMarks::default() {
            let pixels = self.render_body(doc, size);
            return (pixels.len() == size[0] * size[1] * 4).then_some(pixels);
        }
        let [width, height] = size;
        let density = [width as f64 / f64::from(self.media.2), height as f64 / f64::from(self.media.3)];
        let origin = [self.media.0, self.media.1];
        let pixel = |axis: usize, value: f32| {
            ((f64::from(value) - f64::from(origin[axis])) * density[axis])
                .round().clamp(0.0, size[axis] as f64) as usize
        };
        let x0 = pixel(0, self.body.0);
        let y0 = pixel(1, self.body.1);
        let x1 = pixel(0, self.body.0 + self.body.2);
        let y1 = pixel(1, self.body.1 + self.body.3);
        let mut pixels = vec![0; width * height * 4];
        if x1 > x0 && y1 > y0 {
            let row_bytes = (x1 - x0) * 4;
            let content = self.render_body(doc, [x1 - x0, y1 - y0]);
            if content.len() != row_bytes * (y1 - y0) { return None; }
            for (row, source) in content.chunks_exact(row_bytes).enumerate() {
                let start = ((row + y0) * width + x0) * 4;
                pixels[start..start + row_bytes].copy_from_slice(source);
            }
        }
        let mut painter = MarkPainter { pixels: &mut pixels, size, density, origin, unit: self.units_per_point };
        self.draw_marks(doc, &mut painter);
        if let Some((page, total)) = self.marks.page_info {
            let mut label = format!("{page} / {total} · {title}");
            if let PrintContent::Artboard(id) = self.content && let Some(board) = doc.get_object(id) {
                label.push_str(" — ");
                label.push_str(&board.name);
            }
            let [left, _, right, bottom] = extents(self.body);
            painter.page_info([left, right], bottom, &label)?;
        }
        Some(pixels)
    }

    fn draw_marks(self, doc: &Document, painter: &mut MarkPainter<'_>) {
        let [left, top, right, bottom] = extents(self.body);
        let unit = self.units_per_point;
        if self.marks.crop {
            for [start, end] in trim_mark_segments(extents(self.trim), [left, top, right, bottom], unit) {
                painter.line(start, end);
            }
        }
        if self.marks.fold && let Some(pages) = self.content.pages() {
            for [start, end] in fold_mark_segments(doc, &pages, extents(self.trim), extents(self.body), unit) {
                painter.line(start, end);
            }
        }
        if self.marks.registration {
            let centre_x = (left + right) / 2.0;
            let centre_y = (top + bottom) / 2.0;
            let edge = 9.0 * unit;
            let top_edge = if self.marks.color_bars { 19.0 * unit } else { edge };
            for centre in [[left - edge, centre_y], [right + edge, centre_y],
                           [centre_x, top - top_edge], [centre_x, bottom + edge]] {
                painter.target(centre);
            }
        }
        if self.marks.color_bars {
            painter.color_bar([left, right], top);
        }
    }

    fn render_body(self, doc: &Document, size: [usize; 2]) -> Vec<u8> {
        match self.content {
            PrintContent::Artboard(id) => dtool_export::render_artboard_rgba_region(doc, id, self.body, size),
            content => content.pages()
                .map_or_else(Vec::new, |pages| dtool_export::render_facing_bleed_rgba_region(doc, &pages, self.body, size, self.bleed)
                    .unwrap_or_else(|| dtool_export::render_pages_rgba_region(doc, &pages, self.body, size))),
        }
    }
}

fn extents((x, y, w, h): Region) -> [f32; 4] {
    [x, y, x + w, y + h]
}

struct MarkPainter<'a> {
    pixels: &'a mut [u8],
    size: [usize; 2],
    origin: [f32; 2],
    density: [f64; 2],
    unit: f32,
}

impl MarkPainter<'_> {
    fn dots(&self, axis: usize, value: f32) -> f64 {
        (f64::from(value) - f64::from(self.origin[axis])) * self.density[axis]
    }

    fn pixel(&self, axis: usize, value: f32) -> usize {
        self.dots(axis, value).round().clamp(0.0, self.size[axis] as f64) as usize
    }

    fn fill(&mut self, [x0, y0, x1, y1]: [usize; 4], rgba: [u8; 4]) {
        let [width, height] = self.size;
        for row in y0..y1.min(height) {
            let region = (row * width + x0) * 4..(row * width + x1.min(width)) * 4;
            for pixel in self.pixels[region].chunks_exact_mut(4) {
                pixel.copy_from_slice(&rgba);
            }
        }
    }

    fn line(&mut self, start: [f32; 2], end: [f32; 2]) {
        let [x0, y0] = [self.pixel(0, start[0]), self.pixel(1, start[1])];
        let [mut x1, mut y1] = [self.pixel(0, end[0]), self.pixel(1, end[1])];
        let thickness = |axis: usize| (0.25 * f64::from(self.unit) * self.density[axis]).round().max(1.0) as usize;
        if start[0] == end[0] { x1 = x1.saturating_add(thickness(0)); }
        if start[1] == end[1] { y1 = y1.saturating_add(thickness(1)); }
        self.fill([x0, y0, x1, y1], [0, 0, 0, 255]);
    }

    fn target(&mut self, [x, y]: [f32; 2]) {
        let arm = 4.5 * self.unit;
        let radius = 3.0 * self.unit;
        let cx = self.dots(0, x);
        let cy = self.dots(1, y);
        let minimum = 0.55 / self.density[0].min(self.density[1]);
        let half_width = (0.25 * f64::from(self.unit)).max(minimum);
        for row in self.pixel(1, y - arm)..self.pixel(1, y + arm) {
            for col in self.pixel(0, x - arm)..self.pixel(0, x + arm) {
                let dx = (col as f64 + 0.5 - cx) / self.density[0];
                let dy = (row as f64 + 0.5 - cy) / self.density[1];
                if (dx.hypot(dy) - f64::from(radius)).abs() <= half_width {
                    let index = (row * self.size[0] + col) * 4;
                    self.pixels[index..index + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        self.line([x - arm, y], [x + arm, y]);
        self.line([x, y - arm], [x, y + arm]);
    }

    fn color_bar(&mut self, [left, right]: [f32; 2], top: f32) {
        // Composite output has RGB red, green and blue channels. The bars
        // provide visible color and neutral steps in that output space.
        const PATCHES: [[u8; 3]; 12] = [
            [0, 255, 255], [255, 0, 255], [255, 255, 0], [0, 0, 0],
            [255, 0, 0], [0, 255, 0], [0, 0, 255],
            [51, 51, 51], [102, 102, 102], [153, 153, 153], [204, 204, 204], [242, 242, 242],
        ];
        let width = ((right - left) * 0.82).min(120.0 * self.unit);
        let start = (left + right - width) / 2.0;
        let step = width / PATCHES.len() as f32;
        let y0 = self.pixel(1, top - 10.0 * self.unit);
        let y1 = self.pixel(1, top - 4.0 * self.unit).max(y0 + 1);
        for (index, [r, g, b]) in PATCHES.into_iter().enumerate() {
            let x0 = self.pixel(0, start + index as f32 * step);
            let x1 = self.pixel(0, start + (index + 1) as f32 * step);
            self.fill([x0, y0, x1, y1], [r, g, b, 255]);
        }
    }

    fn page_info(&mut self, [left, right]: [f32; 2], bottom: f32, label: &str) -> Option<()> {
        let [x0, x1] = [self.pixel(0, left), self.pixel(0, right)];
        let [y0, y1] = [self.pixel(1, bottom + 16.0 * self.unit), self.pixel(1, bottom + 27.0 * self.unit)];
        let [width, height] = [x1 - x0, y1 - y0];
        if width == 0 || height == 0 { return Some(()); }
        let width_pt = width as f64 / (self.density[0] * f64::from(self.unit));
        let label = label.chars().take(240).filter(|c| !c.is_control()).collect::<String>()
            .replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width_pt}" height="11"><text x="0" y="8" font-family="Pretendard" font-size="8">{label}</text></svg>"#,
        );
        static FONTS: OnceLock<usvg::Options<'static>> = OnceLock::new();
        let fonts = FONTS.get_or_init(|| {
            let mut options = usvg::Options::default();
            options.fontdb_mut().load_font_data(crate::fonts::PRETENDARD.to_vec());
            options
        });
        let tree = usvg::Tree::from_str(&svg, fonts).ok()?;
        let mut strip = tiny_skia::Pixmap::new(width as u32, height as u32)?;
        let scale = tiny_skia::Transform::from_scale(width as f32 / tree.size().width(), height as f32 / tree.size().height());
        resvg::render(&tree, scale, &mut strip.as_mut());
        for (row, pixels) in strip.data().chunks_exact(width * 4).enumerate() {
            let start = ((row + y0) * self.size[0] + x0) * 4;
            self.pixels[start..start + width * 4].copy_from_slice(pixels);
        }
        Some(())
    }
}
