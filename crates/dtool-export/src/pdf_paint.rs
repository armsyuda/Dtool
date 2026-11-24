use dtool_core::document::Document;
use dtool_core::paint::{
    Colour, DiffusionPaint, GradientKind, GradientStop, Hatch, MeshPaint, Paint, PatternPaint,
    StrokeStyle, BitmapPaint,
};
use krilla::color::luma;
use krilla::geom::{Path, PathBuilder, Rect, Size, Transform};
use krilla::mask::{Mask, MaskType};
use krilla::num::NormalizedF32;
use krilla::paint::{
    Fill, FillRule, LineCap, LineJoin, LinearGradient, Paint as KrillaPaint, Pattern,
    RadialGradient, SpreadMethod, Stop, Stroke, StrokeDash, SweepGradient,
};
use krilla::surface::Surface;
use crate::pdf::{OutputColour, PdfOptions};

pub trait PdfPaintResources {
    fn resource_bytes(&self, id: &str) -> Option<&[u8]>;
    fn document(&self) -> Option<&Document> {
        None
    }
    fn resource_raster_size(&self, id: &str) -> Option<(u32, u32)> {
        let _ = id;
        None
    }
}

impl PdfPaintResources for () {
    fn resource_bytes(&self, id: &str) -> Option<&[u8]> {
        let _ = id;
        None
    }
}

impl PdfPaintResources for Document {
    fn document(&self) -> Option<&Document> {
        Some(self)
    }

    fn resource_bytes(&self, id: &str) -> Option<&[u8]> {
        for object in self.objects() {
            if resource_id_of(&object.fill).is_some_and(|value| value == id) {
                if let Some(bytes) = object.raster_data.as_deref() {
                    if !bytes.is_empty() {
                        return Some(bytes);
                    }
                }
            }
            if resource_id_of(&object.stroke.paint).is_some_and(|value| value == id) {
                if let Some(bytes) = object.raster_data.as_deref() {
                    if !bytes.is_empty() {
                        return Some(bytes);
                    }
                }
            }
            if object.name == id {
                if let Some(bytes) = object.raster_data.as_deref() {
                    if !bytes.is_empty() {
                        return Some(bytes);
                    }
                }
            }
        }
        self.resources.iter().find(|resource| resource.id.0.to_string() == id)
            .and_then(|resource| resource.data.as_deref())
    }

    fn resource_raster_size(&self, id: &str) -> Option<(u32, u32)> {
        for object in self.objects() {
            let matches = resource_id_of(&object.fill).is_some_and(|value| value == id)
                || resource_id_of(&object.stroke.paint).is_some_and(|value| value == id)
                || object.name == id;
            if matches {
                if let Some(size) = object.raster_size {
                    if size.0 > 0 && size.1 > 0 {
                        return Some(size);
                    }
                }
            }
        }
        None
    }
}

pub struct SliceResources<'a> {
    pub entries: &'a [(&'a str, &'a [u8])],
}

impl PdfPaintResources for SliceResources<'_> {
    fn resource_bytes(&self, id: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|(name, _)| *name == id)
            .map(|(_, bytes)| *bytes)
    }
}

fn resource_id_of(paint: &Paint) -> Option<&str> {
    match paint {
        Paint::Bitmap(bitmap) => Some(bitmap.resource_id.as_str()),
        Paint::Pattern(pattern) => Some(pattern.resource_id.as_str()),
        _ => None,
    }
}

fn colour_opacity(colour: Colour) -> NormalizedF32 {
    NormalizedF32::new((f32::from(colour.a) / 255.0).clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE)
}

fn combined_opacity(colour: Colour, opacity: f32) -> NormalizedF32 {
    let value = (f32::from(colour.a) / 255.0 * opacity).clamp(0.0, 1.0);
    NormalizedF32::new(value).unwrap_or(NormalizedF32::ONE)
}

fn unit_opacity(opacity: f32) -> NormalizedF32 {
    NormalizedF32::new(opacity.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE)
}

fn affine(matrix: [f64; 6]) -> Transform {
    Transform::from_row(
        matrix[0] as f32,
        matrix[1] as f32,
        matrix[2] as f32,
        matrix[3] as f32,
        matrix[4] as f32,
        matrix[5] as f32,
    )
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    let rect = Rect::from_xywh(x, y, w, h)?;
    let mut builder = PathBuilder::new();
    builder.push_rect(rect);
    builder.finish()
}

fn triangle_path(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> Option<Path> {
    let mut builder = PathBuilder::new();
    builder.move_to(a[0], a[1]);
    builder.line_to(b[0], b[1]);
    builder.line_to(c[0], c[1]);
    builder.close();
    builder.finish()
}

fn line_path(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Path> {
    let mut builder = PathBuilder::new();
    builder.move_to(x0, y0);
    builder.line_to(x1, y1);
    builder.finish()
}

fn gradient_stops(stops: &[GradientStop], output: &OutputColour) -> Vec<Stop> {
    let mut converted: Vec<Stop> = stops
        .iter()
        .filter_map(|stop| {
            Some(Stop {
                offset: NormalizedF32::new(stop.offset.clamp(0.0, 1.0))?,
                color: output.colour(stop.colour),
                opacity: colour_opacity(stop.colour),
            })
        })
        .collect();
    converted.sort_by(|a, b| a.offset.get().total_cmp(&b.offset.get()));
    if converted.is_empty() {
        converted.push(Stop {
            offset: NormalizedF32::ZERO,
            color: output.colour(Colour::new(0, 0, 0, 255)),
            opacity: NormalizedF32::ONE,
        });
        converted.push(Stop {
            offset: NormalizedF32::ONE,
            color: output.colour(Colour::new(255, 255, 255, 255)),
            opacity: NormalizedF32::ONE,
        });
    } else if converted.len() == 1 {
        let only = converted[0].clone();
        converted = vec![
            Stop {
                offset: NormalizedF32::ZERO,
                color: only.color.clone(),
                opacity: only.opacity,
            },
            Stop {
                offset: NormalizedF32::ONE,
                color: only.color,
                opacity: only.opacity,
            },
        ];
    }
    converted
}

fn mask_stops(stops: &[GradientStop]) -> Vec<Stop> {
    let mut converted: Vec<Stop> = stops
        .iter()
        .filter_map(|stop| {
            let opacity = (f32::from(stop.colour.a) / 255.0).clamp(0.0, 1.0);
            let channel = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;
            Some(Stop {
                offset: NormalizedF32::new(stop.offset.clamp(0.0, 1.0))?,
                color: luma::Color::new(channel).into(),
                opacity: NormalizedF32::ONE,
            })
        })
        .collect();
    converted.sort_by(|a, b| a.offset.get().total_cmp(&b.offset.get()));
    if converted.is_empty() {
        converted.push(Stop {
            offset: NormalizedF32::ZERO,
            color: luma::Color::new(255).into(),
            opacity: NormalizedF32::ONE,
        });
        converted.push(Stop {
            offset: NormalizedF32::ONE,
            color: luma::Color::new(0).into(),
            opacity: NormalizedF32::ONE,
        });
    } else if converted.len() == 1 {
        let only = converted[0].clone();
        converted = vec![
            Stop {
                offset: NormalizedF32::ZERO,
                color: only.color.clone(),
                opacity: only.opacity,
            },
            Stop {
                offset: NormalizedF32::ONE,
                color: only.color,
                opacity: only.opacity,
            },
        ];
    }
    converted
}

fn gradient_paint(kind: &GradientKind, stops: &[GradientStop], matrix: [f64; 6], output: &OutputColour) -> KrillaPaint {
    let stops = gradient_stops(stops, output);
    let transform = affine(matrix);
    match kind {
        GradientKind::Linear => LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 0.0,
            transform,
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
        GradientKind::Radial => RadialGradient {
            fx: 0.0,
            fy: 0.0,
            fr: 0.0,
            cx: 0.0,
            cy: 0.0,
            cr: 1.0,
            transform,
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
        GradientKind::Elliptical => RadialGradient {
            fx: 0.5,
            fy: 0.5,
            fr: 0.0,
            cx: 0.5,
            cy: 0.5,
            cr: 0.5,
            transform,
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
        GradientKind::Conical => SweepGradient {
            cx: 0.5,
            cy: 0.5,
            start_angle: 180.0,
            end_angle: 540.0,
            transform,
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
    }
}

fn mask_gradient_paint(kind: &GradientKind, stops: &[GradientStop], matrix: [f64; 6]) -> KrillaPaint {
    let stops = mask_stops(stops);
    let transform = affine(matrix);
    match kind {
        GradientKind::Linear => LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 0.0,
            transform,
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
        GradientKind::Radial | GradientKind::Elliptical => RadialGradient {
            fx: 0.0,
            fy: 0.0,
            fr: 0.0,
            cx: 0.0,
            cy: 0.0,
            cr: 1.0,
            transform,
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
        GradientKind::Conical => SweepGradient {
            cx: 0.0,
            cy: 0.0,
            start_angle: 0.0,
            end_angle: 360.0,
            transform,
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
    }
}

fn hatch_to_paint(surface: &mut Surface<'_>, hatch: &Hatch, output: &OutputColour) -> KrillaPaint {
    let spacing = hatch
        .lines
        .iter()
        .map(|line| line.spacing.abs())
        .fold(0.0_f64, f64::max);
    let size = spacing.max(hatch.weight.abs()).max(1.0) as f32;
    let width = size;
    let height = size;
    let stroke_width = (hatch.weight.abs() as f32).max(0.25);
    let mut builder = surface.stream_builder();
    {
        let mut tile = builder.surface();
        if let Some(background) = hatch.background {
            tile.set_stroke(None);
            tile.set_fill(Some(Fill {
                paint: output.colour(background).into(),
                opacity: colour_opacity(background),
                rule: FillRule::NonZero,
            }));
            if let Some(path) = rect_path(0.0, 0.0, width, height) {
                tile.draw_path(&path);
            }
        }
        tile.set_fill(None);
        for line in &hatch.lines {
            let dash = if line.dash.is_empty() {
                None
            } else {
                Some(StrokeDash {
                    array: line.dash.iter().map(|value| *value as f32).collect(),
                    offset: line.shift as f32,
                })
            };
            tile.set_stroke(Some(Stroke {
                paint: output.colour(hatch.colour).into(),
                width: stroke_width,
                miter_limit: 10.0,
                line_cap: LineCap::Butt,
                line_join: LineJoin::Miter,
                opacity: colour_opacity(hatch.colour),
                dash,
            }));
            let cos = line.angle_rad.cos() as f32;
            let sin = line.angle_rad.sin() as f32;
            let span = (width * width + height * height).sqrt() * 2.0;
            let origin_x = line.origin[0] as f32;
            let origin_y = line.origin[1] as f32;
            let step = line.spacing.abs().max(hatch.weight.abs()).max(1.0) as f32;
            let normal_x = -sin;
            let normal_y = cos;
            let replicas = ((span / step).ceil() as i32).max(1);
            for replica in -replicas..=replicas {
                let ox = origin_x + normal_x * step * replica as f32;
                let oy = origin_y + normal_y * step * replica as f32;
                if let Some(path) = line_path(ox - cos * span, oy - sin * span, ox + cos * span, oy + sin * span)
                {
                    tile.draw_path(&path);
                }
            }
        }
        tile.finish();
    }
    let stream = builder.finish();
    Pattern {
        stream,
        transform: affine(hatch.matrix),
        width,
        height,
    }
    .into()
}

fn image_from_bytes(bytes: &[u8]) -> Option<krilla::image::Image> {
    let data = bytes.to_vec().into();
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        krilla::image::Image::from_png(data, true).ok()
    } else if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        krilla::image::Image::from_jpeg(data, true).ok()
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        krilla::image::Image::from_gif(data, true).ok()
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        krilla::image::Image::from_webp(data, true).ok()
    } else {
        None
    }
}

fn rgba_image(bytes: &[u8], size: (u32, u32)) -> Option<krilla::image::Image> {
    let expected = size.0 as usize * size.1 as usize * 4;
    if size.0 == 0 || size.1 == 0 || bytes.len() != expected {
        return None;
    }
    Some(krilla::image::Image::from_rgba8(bytes.to_vec(), size.0, size.1))
}

fn tiled_image_paint(
    surface: &mut Surface<'_>,
    bytes: Option<&[u8]>,
    raster_size: Option<(u32, u32)>,
    matrix: [f64; 6],
    converted: Option<krilla::image::Image>,
) -> KrillaPaint {
    let mut width = 1.0_f32;
    let mut height = 1.0_f32;
    let decoded = converted.or_else(|| bytes.and_then(image_from_bytes)).or_else(|| {
        bytes.and_then(|data| raster_size.and_then(|size| rgba_image(data, size)))
    });
    let mut builder = surface.stream_builder();
    {
        let mut tile = builder.surface();
        if let Some(image) = decoded {
            let (iw, ih) = image.size();
            if iw > 0 && ih > 0 {
                width = iw as f32;
                height = ih as f32;
                if let Some(size) = Size::from_wh(width, height) {
                    tile.draw_image(image, size);
                }
            }
        }
        tile.finish();
    }
    let stream = builder.finish();
    Pattern {
        stream,
        transform: affine(matrix),
        width,
        height,
    }
    .into()
}

fn bitmap_to_paint<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    bitmap: &BitmapPaint,
    resources: &R,
    output: &OutputColour,
) -> KrillaPaint {
    tiled_image_paint(
        surface,
        resources.resource_bytes(&bitmap.resource_id),
        resources.resource_raster_size(&bitmap.resource_id),
        bitmap.matrix,
        resources.document().and_then(|doc| output.bitmap_image(doc, &bitmap.resource_id)),
    )
}

fn pattern_to_paint<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    pattern: &PatternPaint,
    resources: &R,
) -> KrillaPaint {
    tiled_image_paint(
        surface,
        resources.resource_bytes(&pattern.resource_id),
        resources.resource_raster_size(&pattern.resource_id),
        pattern.matrix,
        None,
    )
}

fn map_mesh_point(matrix: [f64; 6], point: [f64; 2]) -> [f32; 2] {
    [
        (matrix[0] * point[0] + matrix[2] * point[1] + matrix[4]) as f32,
        (matrix[1] * point[0] + matrix[3] * point[1] + matrix[5]) as f32,
    ]
}

fn mesh_to_paint(surface: &mut Surface<'_>, mesh: &MeshPaint, output: &OutputColour) -> KrillaPaint {
    let mut min_x = 0.0_f32;
    let mut min_y = 0.0_f32;
    let mut max_x = 1.0_f32;
    let mut max_y = 1.0_f32;
    if !mesh.vertices.is_empty() {
        let mapped: Vec<[f32; 2]> = mesh
            .vertices
            .iter()
            .map(|point| map_mesh_point(mesh.matrix, *point))
            .collect();
        min_x = mapped.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        min_y = mapped.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        max_x = mapped.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
        max_y = mapped.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
        if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
            min_x = 0.0;
            min_y = 0.0;
            max_x = 1.0;
            max_y = 1.0;
        }
    }
    let width = (max_x - min_x).abs().max(1.0);
    let height = (max_y - min_y).abs().max(1.0);
    let mut builder = surface.stream_builder();
    {
        let mut tile = builder.surface();
        tile.set_stroke(None);
        let triangles = mesh.indices.chunks_exact(3);
        for tri in triangles {
            let a = mesh.vertices.get(tri[0]).copied().unwrap_or([0.0, 0.0]);
            let b = mesh.vertices.get(tri[1]).copied().unwrap_or([0.0, 0.0]);
            let c = mesh.vertices.get(tri[2]).copied().unwrap_or([0.0, 0.0]);
            let pa = map_mesh_point(mesh.matrix, a);
            let pb = map_mesh_point(mesh.matrix, b);
            let pc = map_mesh_point(mesh.matrix, c);
            let ca = mesh.colours.get(tri[0]).copied().unwrap_or(Colour::new(0, 0, 0, 255));
            let cb = mesh.colours.get(tri[1]).copied().unwrap_or(Colour::new(0, 0, 0, 255));
            let cc = mesh.colours.get(tri[2]).copied().unwrap_or(Colour::new(0, 0, 0, 255));
            let colour = Colour::new(
                ((u16::from(ca.r) + u16::from(cb.r) + u16::from(cc.r)) / 3) as u8,
                ((u16::from(ca.g) + u16::from(cb.g) + u16::from(cc.g)) / 3) as u8,
                ((u16::from(ca.b) + u16::from(cb.b) + u16::from(cc.b)) / 3) as u8,
                ((u16::from(ca.a) + u16::from(cb.a) + u16::from(cc.a)) / 3) as u8,
            );
            tile.set_fill(Some(Fill {
                paint: output.colour(colour).into(),
                opacity: colour_opacity(colour),
                rule: FillRule::NonZero,
            }));
            let shifted_a = [pa[0] - min_x, pa[1] - min_y];
            let shifted_b = [pb[0] - min_x, pb[1] - min_y];
            let shifted_c = [pc[0] - min_x, pc[1] - min_y];
            if let Some(path) = triangle_path(shifted_a, shifted_b, shifted_c) {
                tile.draw_path(&path);
            }
        }
        tile.finish();
    }
    let stream = builder.finish();
    Pattern {
        stream,
        transform: Transform::from_translate(min_x, min_y),
        width,
        height,
    }
    .into()
}

fn seeded_noise(seed: u64, x: u32, y: u32) -> f64 {
    let mut h = seed ^ (x as u64).wrapping_mul(0x9E3779B97F4A7C15);
    h ^= (y as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h = h.wrapping_mul(0x94D049BB133111EB);
    h ^= h >> 32;
    (h as f64) / (u64::MAX as f64)
}

fn diffusion_to_paint(surface: &mut Surface<'_>, diffusion: &DiffusionPaint, output: &OutputColour) -> KrillaPaint {
    let cells = match diffusion.mode {
        dtool_core::paint::DiffusionCurveCreationMode::Automatic => 12_u32,
        dtool_core::paint::DiffusionCurveCreationMode::Manual => 8_u32,
    };
    let width = 32.0_f32;
    let height = 32.0_f32;
    let cell_w = width / cells as f32;
    let cell_h = height / cells as f32;
    let seed = diffusion.seed.unwrap_or(0);
    let mut builder = surface.stream_builder();
    {
        let mut tile = builder.surface();
        tile.set_stroke(None);
        for y in 0..cells {
            for x in 0..cells {
                let noise = seeded_noise(seed, x, y) as f32;
                let colour = Colour::new(
                    255,
                    (noise * 255.0).round().clamp(0.0, 255.0) as u8,
                    ((1.0 - noise) * 255.0).round().clamp(0.0, 255.0) as u8,
                    255,
                );
                tile.set_fill(Some(Fill {
                    paint: output.colour(colour).into(),
                    opacity: NormalizedF32::ONE,
                    rule: FillRule::NonZero,
                }));
                if let Some(path) = rect_path(x as f32 * cell_w, y as f32 * cell_h, cell_w, cell_h) {
                    tile.draw_path(&path);
                }
            }
        }
        tile.finish();
    }
    let stream = builder.finish();
    Pattern {
        stream,
        transform: affine(diffusion.matrix),
        width,
        height,
    }
    .into()
}

pub fn paint_to_krilla<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    paint: &Paint,
    opts: &PdfOptions,
    resources: &R,
) -> Option<KrillaPaint> {
    let output = OutputColour::new(resources.document(), opts).ok()?;
    paint_for_output(surface, paint, &output, resources)
}

fn paint_for_output<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    paint: &Paint,
    output: &OutputColour,
    resources: &R,
) -> Option<KrillaPaint> {
    match paint {
        Paint::None => None,
        Paint::Solid { colour } => Some(output.colour(*colour).into()),
        Paint::Gradient { kind, stops, geom } => Some(gradient_paint(kind, stops, geom.to_matrix(), output)),
        Paint::Hatch(hatch) => Some(hatch_to_paint(surface, hatch, output)),
        Paint::Bitmap(bitmap) => Some(bitmap_to_paint(surface, bitmap, resources, output)),
        Paint::Pattern(pattern) => Some(pattern_to_paint(surface, pattern, resources)),
        Paint::Mesh(mesh) => Some(mesh_to_paint(surface, mesh, output)),
        Paint::Diffusion(diffusion) => Some(diffusion_to_paint(surface, diffusion, output)),
    }
}

pub fn paint_to_fill<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    paint: &Paint,
    opacity: f32,
    opts: &PdfOptions,
    resources: &R,
) -> Option<Fill> {
    let output = OutputColour::new(resources.document(), opts).ok()?;
    fill_for_output(surface, paint, opacity, &output, resources)
}

pub(crate) fn fill_for_output<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    paint: &Paint,
    opacity: f32,
    output: &OutputColour,
    resources: &R,
) -> Option<Fill> {
    let krilla_paint = paint_for_output(surface, paint, output, resources)?;
    let fill_opacity = match paint {
        Paint::Solid { colour } => combined_opacity(*colour, opacity),
        _ => unit_opacity(opacity),
    };
    Some(Fill {
        paint: krilla_paint,
        opacity: fill_opacity,
        rule: FillRule::NonZero,
    })
}

pub fn glyph_paint_to_krilla<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    paint: &Paint,
    opts: &PdfOptions,
    resources: &R,
) -> Option<KrillaPaint> {
    paint_to_krilla(surface, paint, opts, resources)
}

pub fn glyph_paint_to_fill<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    paint: &Paint,
    opacity: f32,
    opts: &PdfOptions,
    resources: &R,
) -> Option<Fill> {
    paint_to_fill(surface, paint, opacity, opts, resources)
}

fn stroke_line_cap(cap: dtool_core::paint::Cap) -> LineCap {
    match cap {
        dtool_core::paint::Cap::Butt => LineCap::Butt,
        dtool_core::paint::Cap::Round => LineCap::Round,
        dtool_core::paint::Cap::Square => LineCap::Square,
    }
}

fn stroke_line_join(join: &dtool_core::paint::Join) -> LineJoin {
    match join {
        dtool_core::paint::Join::Miter { .. } => LineJoin::Miter,
        dtool_core::paint::Join::Round => LineJoin::Round,
        dtool_core::paint::Join::Bevel => LineJoin::Bevel,
    }
}

fn stroke_miter(join: &dtool_core::paint::Join) -> f32 {
    match join {
        dtool_core::paint::Join::Miter { limit } => *limit,
        _ => 10.0,
    }
}

pub fn paint_to_stroke<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    stroke: &StrokeStyle,
    opacity: f32,
    opts: &PdfOptions,
    resources: &R,
) -> Option<Stroke> {
    let output = OutputColour::new(resources.document(), opts).ok()?;
    stroke_for_output(surface, stroke, opacity, &output, resources)
}

pub(crate) fn stroke_for_output<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    stroke: &StrokeStyle,
    opacity: f32,
    output: &OutputColour,
    resources: &R,
) -> Option<Stroke> {
    if !stroke.width.is_finite() || stroke.width <= 0.0 {
        return None;
    }
    let paint = paint_for_output(surface, &stroke.paint, output, resources)?;
    let stroke_opacity = match &stroke.paint {
        Paint::Solid { colour } => combined_opacity(*colour, opacity),
        _ => unit_opacity(opacity),
    };
    Some(Stroke {
        paint,
        width: stroke.width,
        miter_limit: stroke_miter(&stroke.join),
        line_cap: stroke_line_cap(stroke.cap),
        line_join: stroke_line_join(&stroke.join),
        opacity: stroke_opacity,
        dash: if stroke.dash.is_empty() {
            None
        } else {
            Some(StrokeDash {
                array: stroke.dash.clone(),
                offset: 0.0,
            })
        },
    })
}

pub fn glyph_paint_to_stroke<R: PdfPaintResources>(
    surface: &mut Surface<'_>,
    stroke: &StrokeStyle,
    opacity: f32,
    opts: &PdfOptions,
    resources: &R,
) -> Option<Stroke> {
    paint_to_stroke(surface, stroke, opacity, opts, resources)
}

pub fn transparency_soft_mask(
    surface: &mut Surface<'_>,
    kind: &GradientKind,
    stops: &[GradientStop],
    matrix: [f64; 6],
    bounds: [f32; 4],
    _opts: &PdfOptions,
) -> Option<Mask> {
    let [x, y, w, h] = bounds;
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let path = rect_path(x, y, w, h)?;
    let mut builder = surface.stream_builder();
    {
        let mut mask_surface = builder.surface();
        mask_surface.set_stroke(None);
        mask_surface.set_fill(Some(Fill {
            paint: mask_gradient_paint(kind, stops, matrix),
            opacity: NormalizedF32::ONE,
            rule: FillRule::NonZero,
        }));
        mask_surface.draw_path(&path);
        mask_surface.finish();
    }
    let stream = builder.finish();
    Some(Mask::new(stream, MaskType::Luminosity))
}

pub fn push_transparency_soft_mask(
    surface: &mut Surface<'_>,
    kind: &GradientKind,
    stops: &[GradientStop],
    matrix: [f64; 6],
    bounds: [f32; 4],
    opts: &PdfOptions,
) -> bool {
    if let Some(mask) = transparency_soft_mask(surface, kind, stops, matrix, bounds, opts) {
        surface.push_mask(mask);
        true
    } else {
        false
    }
}

pub(crate) fn appearance_erase_mask(
    surface: &mut Surface<'_>,
    path: &Path,
    paint: &Paint,
    even_odd: bool,
    bounds: [f32; 4],
    output: &OutputColour,
    doc: &Document,
) -> Option<Mask> {
    let [left, top, right, bottom] = bounds;
    let coverage = rect_path(left, top, right - left, bottom - top)?;
    let mut alpha = surface.stream_builder();
    {
        let mut alpha_surface = alpha.surface();
        let mut fill = fill_for_output(&mut alpha_surface, paint, 1.0, output, doc)?;
        fill.rule = if even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
        alpha_surface.set_fill(Some(fill));
        alpha_surface.draw_path(path);
        alpha_surface.finish();
    }
    let alpha = Mask::new(alpha.finish(), MaskType::Alpha);
    let mut inverse = surface.stream_builder();
    {
        let mut mask_surface = inverse.surface();
        let luma = |value| Fill {
            paint: luma::Color::new(value).into(),
            opacity: NormalizedF32::ONE,
            rule: FillRule::NonZero,
        };
        mask_surface.set_fill(Some(luma(255)));
        mask_surface.draw_path(&coverage);
        mask_surface.push_mask(alpha);
        mask_surface.set_fill(Some(luma(0)));
        mask_surface.draw_path(&coverage);
        mask_surface.pop();
        mask_surface.finish();
    }
    Some(Mask::new(inverse.finish(), MaskType::Luminosity))
}
