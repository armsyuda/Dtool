use std::collections::HashMap;
use std::fs;
use std::path::Path;

use dtool_core::blend::BlendMode;
use dtool_core::boolean::{BoolOp, FillRule, DEFAULT_TOLERANCE, boolean, boolean_operands, normalize_filled_path};
use dtool_core::curve::Curve;
use dtool_core::document::{Document, Units};
use dtool_core::layer_style::{EffectKind, EffectParams, LayerEffect};
use dtool_core::object::{ClipRole, Object, ObjectId, ObjectKind};
use dtool_core::paint::{BitmapPaint, Cap, Colour, GradientGeom, GradientKind, GradientSpread, GradientStop, Join, Paint, PaintOrder, StrokeStyle};
use dtool_core::raster_source::LayerMask;
use dtool_core::resources::{ImageFormat, ImageResource, ResourceId};
use dtool_core::selection::PixelSelection;
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape, Vec2};
use roxmltree::Document as XmlDoc;

use crate::import_report::{ImportLoss, ImportReport, LossKind};
use crate::{IoError, IoErrorKind};

const MAX_SVG_SIZE: u64 = 10 * 1024 * 1024;
const MAX_IMAGE_DEPTH: usize = 3;
const MAX_CLIP_DEPTH: usize = 64;
const PX_TO_PT_SCALE: f32 = 0.75;
const MAX_TILE_SIDE: f64 = 2048.0;
const MAX_TILE_PIXELS: u32 = 16 * 1024 * 1024;
const MAX_MASK_SIDE: f64 = 2048.0;
const MAX_MASK_PIXELS: u32 = 16 * 1024 * 1024;

#[derive(Default)]
struct PatternTiles {
    resources: HashMap<(*const usvg::Pattern, [u32; 2], u32), ResourceId>,
    pixels: u32,
}

impl PatternTiles {
    fn bitmap(&mut self, doc: &mut Document, pattern: &usvg::Pattern, opacity: f32, transform: Affine) -> Option<(BitmapPaint, bool)> {
        use resvg::tiny_skia::{Pixmap, Transform};

        let rect = pattern.rect();
        let extent = [f64::from(rect.width()), f64::from(rect.height())];
        let transform = transform * affine(pattern.transform());
        if !transform.is_finite() {
            return None;
        }
        let [a, b, c, d, _, _] = transform.as_coeffs();
        let scale = [a.hypot(b), c.hypot(d)];
        let exact_png = if opacity == 1.0 { exact_png_tile(pattern) } else { None };
        let size = exact_png.map_or_else(
            || std::array::from_fn(|axis| (extent[axis] * scale[axis].max(1.0) * 2.0)
                .ceil().clamp(1.0, MAX_TILE_SIDE) as u32),
            |(_, size)| size,
        );
        let step = [extent[0] / f64::from(size[0]), extent[1] / f64::from(size[1])];
        let matrix = transform * Affine::translate((f64::from(rect.x()), f64::from(rect.y())))
            * Affine::scale_non_uniform(step[0], step[1]);
        if !matrix.is_finite() || matrix.determinant().abs() < 1e-12 {
            return None;
        }
        let key = (std::ptr::from_ref(pattern), size, opacity.to_bits());
        let resource = if let Some(&resource) = self.resources.get(&key) {
            resource
        } else {
            let pixels = size[0] * size[1];
            if self.pixels + pixels > MAX_TILE_PIXELS {
                return None;
            }
            let data = if let Some((png, _)) = exact_png {
                png.to_vec()
            } else {
                let mut tile = Pixmap::new(size[0], size[1])?;
                let node = usvg::Node::Group(Box::new(pattern.root().clone()));
                if let Some(bounds) = node.abs_layer_bounding_box() {
                    // Undo render_node's automatic shift to the bounding box of the content.
                    let raster = Transform::from_scale(step[0].recip() as f32, step[1].recip() as f32)
                        .pre_translate(bounds.x(), bounds.y());
                    resvg::render_node(&node, raster, &mut tile.as_mut())?;
                }
                if opacity < 1.0 {
                    for channel in tile.data_mut() {
                        *channel = (f32::from(*channel) * opacity).round() as u8;
                    }
                }
                tile.encode_png().ok()?
            };
            let mut image = ImageResource::new_embedded(ImageFormat::Png, size, 96.0, data);
            image.name = pattern.id().to_owned();
            let resource = doc.add_image_resource(image);
            self.resources.insert(key, resource);
            self.pixels += pixels;
            resource
        };
        let paint = BitmapPaint { resource_id: resource.0.to_string(), matrix: matrix.as_coeffs(), seed: None };
        Some((paint, exact_png.is_none()))
    }
}

fn exact_png_tile(pattern: &usvg::Pattern) -> Option<(&[u8], [u32; 2])> {
    // usvg wraps SVG image elements in a group that owns their placement transform.
    let mut children = pattern.root().children();
    while let [usvg::Node::Group(group)] = children {
        if group.should_isolate() || !group.transform().is_identity() {
            return None;
        }
        children = group.children();
    }
    let [usvg::Node::Image(image)] = children else { return None };
    let usvg::ImageKind::PNG(data) = image.kind() else { return None };
    let size = image.size();
    let rect = pattern.rect();
    if !image.is_visible() || image.abs_transform() != usvg::Transform::identity()
        || rect.width() != size.width() || rect.height() != size.height()
        || f64::from(size.width().max(size.height())) > MAX_TILE_SIDE
    {
        return None;
    }
    Some((data.as_slice(), [size.width() as u32, size.height() as u32]))
}

struct SvgImport {
    doc: Document,
    report: ImportReport,
    patterns: PatternTiles,
    mask_pixels_remaining: u32,
}

fn colour_from_usvg(color: usvg::Color, alpha: f32) -> Colour {
    let a = (alpha * 255.0).clamp(0.0, 255.0) as u8;
    Colour::new(color.red, color.green, color.blue, a)
}

fn affine(t: usvg::Transform) -> Affine {
    Affine::new([t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].map(f64::from))
}

fn gradient_stops(gradient: &usvg::BaseGradient, opacity: f32) -> Vec<GradientStop> {
    gradient.stops().iter().map(|stop| GradientStop {
        offset: stop.offset().get(),
        colour: colour_from_usvg(stop.color(), stop.opacity().get() * opacity),
    }).collect()
}

fn linear_gradient(gradient: &usvg::LinearGradient, transform: Affine) -> (GradientKind, GradientGeom) {
    let first = Point::new(f64::from(gradient.x1()), f64::from(gradient.y1()));
    let last = Point::new(f64::from(gradient.x2()), f64::from(gradient.y2()));
    let direction = last - first;
    let start = transform * first;
    let [a, b, c, d, _, _] = transform.inverse().as_coeffs();
    // A transformed gradient's normal follows the inverse transpose, not the path's transform.
    let normal = Vec2::new(a * direction.x + b * direction.y, c * direction.x + d * direction.y);
    let end = start + normal * (direction.hypot2() / normal.hypot2());
    (GradientKind::Linear, GradientGeom { start: [start.x, start.y], end: [end.x, end.y], ..GradientGeom::default() })
}

fn radial_gradient(gradient: &usvg::RadialGradient, transform: Affine) -> (GradientKind, GradientGeom, bool) {
    let center = transform * Point::new(f64::from(gradient.cx()), f64::from(gradient.cy()));
    let [a, b, c, d, _, _] = transform.as_coeffs();
    let xx = a * a + c * c;
    let yy = b * b + d * d;
    let xy = a * b + c * d;
    // The eigenvectors of the transformed circle give the ellipse's orthogonal axes.
    let major_squared = (xx + yy + (xx - yy).hypot(2.0 * xy)) * 0.5;
    let radius = f64::from(gradient.r().get()) * major_squared.sqrt();
    let ratio = (transform.determinant().abs() / major_squared).min(1.0);
    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    let end = center + radius * Vec2::from_angle(angle);
    let kind = if (ratio - 1.0).abs() < 1e-6 { GradientKind::Radial } else { GradientKind::Elliptical };
    let mut geom = GradientGeom { start: [center.x, center.y], end: [end.x, end.y], ratio, ..GradientGeom::default() };
    let focus = transform * Point::new(f64::from(gradient.fx()), f64::from(gradient.fy()));
    let unit_focus = Affine::new(geom.to_matrix()).inverse() * focus;
    let inner = f64::from(gradient.fr().get()) / f64::from(gradient.r().get());
    let contained = unit_focus.to_vec2().hypot() + inner < 1.0;
    if contained {
        geom.focus = (focus != center).then_some([unit_focus.x, unit_focus.y]);
        geom.focus_radius = inner;
    }
    (kind, geom, !contained)
}

fn paint_from_usvg(paint: &usvg::Paint, opacity: f32, transform: Affine, state: &mut SvgImport, context: &str) -> Paint {
    let (gradient, kind, mut geom, approximate_focus) = match paint {
        usvg::Paint::Color(color) => return Paint::Solid { colour: colour_from_usvg(*color, opacity) },
        usvg::Paint::LinearGradient(gradient) => {
            let (kind, geom) = linear_gradient(gradient, transform * affine(gradient.transform()));
            (&***gradient, kind, geom, false)
        }
        usvg::Paint::RadialGradient(gradient) => {
            let (kind, geom, approximate_focus) = radial_gradient(gradient, transform * affine(gradient.transform()));
            (&***gradient, kind, geom, approximate_focus)
        }
        usvg::Paint::Pattern(pattern) => {
            return match state.patterns.bitmap(&mut state.doc, pattern, opacity, transform) {
                Some((paint, rasterized)) => {
                    if rasterized {
                        state.report.add_loss(LossKind::Pattern, 1, false, format!("pattern tile rasterized in {context}"));
                    }
                    Paint::Bitmap(paint)
                }
                None => {
                    state.report.add_loss(LossKind::Pattern, 1, false, format!("pattern could not be rasterized in {context}"));
                    Paint::None
                }
            };
        }
    };
    let stops = gradient_stops(gradient, opacity);
    if geom.start.iter().chain(&geom.end).any(|coordinate| !coordinate.is_finite())
        || geom.start == geom.end || geom.ratio <= 0.0
    {
        state.report.add_loss(LossKind::Gradient, 1, false, format!("degenerate gradient approximated in {context}"));
        return stops.last().map_or(Paint::None, |stop| Paint::Solid { colour: stop.colour });
    }
    geom.spread = match gradient.spread_method() {
        usvg::SpreadMethod::Pad => GradientSpread::Pad,
        usvg::SpreadMethod::Repeat => GradientSpread::Repeat,
        usvg::SpreadMethod::Reflect => GradientSpread::Reflect,
    };
    if approximate_focus {
        state.report.add_loss(LossKind::Gradient, 1, false, format!("radial gradient focal circle approximated in {context}"));
    }
    Paint::Gradient { kind, stops, geom }
}

fn fill_to_paint(fill: &usvg::Fill, transform: Affine, state: &mut SvgImport, context: &str) -> Paint {
    paint_from_usvg(fill.paint(), fill.opacity().get(), transform, state, context)
}

fn stroke_to_style(stroke: &usvg::Stroke, transform: Affine, state: &mut SvgImport, context: &str) -> StrokeStyle {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    let x = Vec2::new(a, b);
    let y = Vec2::new(c, d);
    let scale = transform.determinant().abs().sqrt() as f32;
    let max_length_squared = x.hypot2().max(y.hypot2());
    let uniform = (x.hypot2() - y.hypot2()).abs().max(x.dot(y).abs()) <= 1e-6 * max_length_squared;
    if !uniform {
        state.report.add_loss(LossKind::Other, 1, false, format!("non-uniform stroke scaling approximated in {context}"));
    }
    let cap = match stroke.linecap() {
        usvg::LineCap::Butt => Cap::Butt,
        usvg::LineCap::Round => Cap::Round,
        usvg::LineCap::Square => Cap::Square,
    };
    let join = match stroke.linejoin() {
        usvg::LineJoin::Miter => Join::Miter { limit: stroke.miterlimit().get() },
        usvg::LineJoin::MiterClip => {
            state.report.add_loss(LossKind::Other, 1, false, format!("miter-clip stroke join approximated in {context}"));
            Join::Miter { limit: stroke.miterlimit().get() }
        }
        usvg::LineJoin::Round => Join::Round,
        usvg::LineJoin::Bevel => Join::Bevel,
    };
    StrokeStyle {
        paint: paint_from_usvg(stroke.paint(), stroke.opacity().get(), transform, state, context),
        width: stroke.width().get() * scale,
        dash: stroke.dasharray().unwrap_or_default().iter().map(|length| length * scale).collect(),
        dash_offset: stroke.dashoffset() * scale,
        cap,
        join,
        ..StrokeStyle::default()
    }
}

fn object_bounds(node_bbox: usvg::Rect) -> ([f32; 2], [f32; 2]) {
    (
        [node_bbox.x(), node_bbox.y()],
        [node_bbox.width(), node_bbox.height()],
    )
}

fn path_geometry(path: &usvg::Path, transform: Affine) -> (Curve, [f32; 2], [f32; 2]) {
    curve_geometry(path_outline(path, transform))
}

fn path_outline(path: &usvg::Path, transform: Affine) -> BezPath {
    use usvg::tiny_skia_path::PathSegment;

    let point = |p: usvg::tiny_skia_path::Point| transform * Point::new(f64::from(p.x), f64::from(p.y));
    let mut outline = BezPath::new();
    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(p) => outline.move_to(point(p)),
            PathSegment::LineTo(p) => outline.line_to(point(p)),
            PathSegment::QuadTo(c, p) => outline.quad_to(point(c), point(p)),
            PathSegment::CubicTo(a, b, p) => outline.curve_to(point(a), point(b), point(p)),
            PathSegment::Close => outline.close_path(),
        }
    }
    outline
}

fn curve_geometry(outline: BezPath) -> (Curve, [f32; 2], [f32; 2]) {
    let bounds = outline.bounding_box();
    let pos = [bounds.x0 as f32, bounds.y0 as f32];
    let size = [bounds.width() as f32, bounds.height() as f32];
    let local = Affine::translate(Vec2::new(-f64::from(pos[0]), -f64::from(pos[1]))) * outline;
    (Curve::from_kurbo(&local), pos, size)
}

fn closed_clip_outline(path: &usvg::Path, transform: Affine) -> BezPath {
    let mut outline = BezPath::new();
    let mut open = false;
    for element in path_outline(path, transform).elements().iter().copied() {
        if open && matches!(element, PathEl::MoveTo(_)) {
            outline.close_path();
        }
        outline.push(element);
        open = element != PathEl::ClosePath;
    }
    if open {
        outline.close_path();
    }
    outline
}

fn clip_leaf(path: &usvg::Path, transform: Affine) -> Result<BezPath, dtool_core::CommandError> {
    let outline = closed_clip_outline(path, transform);
    if path.fill().is_some_and(|fill| fill.rule() == usvg::FillRule::EvenOdd) {
        return Ok(outline);
    }
    let nonzero = normalize_filled_path(&outline, FillRule::NonZero, DEFAULT_TOLERANCE)?;
    let evenodd = normalize_filled_path(&outline, FillRule::EvenOdd, DEFAULT_TOLERANCE)?;
    // Native vector masks use even-odd coverage. Retain the original control points
    // when that rule covers the same region as SVG's nonzero winding rule.
    if nonzero == evenodd {
        Ok(outline)
    } else {
        Ok(nonzero.map_or_else(BezPath::new, |curve| curve.to_kurbo()))
    }
}

fn clip_paths(curves: Vec<Curve>) -> BezPath {
    let mut outline = BezPath::new();
    for curve in curves {
        outline.extend(curve.to_kurbo().elements().iter().copied());
    }
    outline
}

fn intersect_clips(a: BezPath, b: BezPath) -> Result<BezPath, dtool_core::CommandError> {
    let a = normalize_filled_path(&a, FillRule::EvenOdd, DEFAULT_TOLERANCE)?;
    let b = normalize_filled_path(&b, FillRule::EvenOdd, DEFAULT_TOLERANCE)?;
    match (a, b) {
        (Some(a), Some(b)) => boolean(&[a.to_kurbo()], &[b.to_kurbo()], BoolOp::Intersect,
            FillRule::EvenOdd, DEFAULT_TOLERANCE).map(clip_paths),
        _ => Ok(BezPath::new()),
    }
}

fn clip_outline(clip: &usvg::ClipPath, transform: Affine, depth: usize) -> Result<BezPath, dtool_core::CommandError> {
    let own = clip_children(clip.root(), transform * affine(clip.transform()), depth)?;
    if let Some(link) = clip.clip_path() {
        // Linked clip paths use the same referencing coordinate system.
        intersect_clips(own, clip_outline(link, transform, depth + 1)?)
    } else {
        Ok(own)
    }
}

fn clip_children(group: &usvg::Group, transform: Affine, depth: usize) -> Result<BezPath, dtool_core::CommandError> {
    if depth >= MAX_CLIP_DEPTH {
        return Err(dtool_core::CommandError::new("REQUEST_TOO_LARGE"));
    }
    let mut outlines = Vec::new();
    for child in group.children() {
        let path = match child {
            usvg::Node::Path(path) if path.is_visible() => clip_leaf(path, transform)?,
            usvg::Node::Text(text) => clip_children(text.flattened(), transform, depth + 1)?,
            usvg::Node::Group(inner) => {
                let basis = transform * affine(inner.transform());
                let own = clip_children(inner, basis, depth + 1)?;
                if let Some(clip) = inner.clip_path() {
                    intersect_clips(own, clip_outline(clip, basis, depth + 1)?)?
                } else {
                    own
                }
            }
            _ => continue,
        };
        if !path.elements().is_empty() {
            outlines.push(path);
        }
    }
    if outlines.len() <= 1 {
        return Ok(outlines.pop().unwrap_or_default());
    }
    let mut operands = Vec::new();
    for path in outlines {
        if let Some(curve) = normalize_filled_path(&path, FillRule::EvenOdd, DEFAULT_TOLERANCE)? {
            operands.push(vec![curve.to_kurbo()]);
        }
    }
    if operands.len() <= 1 {
        return Ok(operands.pop().and_then(|mut paths| paths.pop()).unwrap_or_default());
    }
    boolean_operands(&operands, BoolOp::Add, FillRule::EvenOdd, DEFAULT_TOLERANCE).map(clip_paths)
}

/// A single opaque path can act as a hard mask, including SVG exported by DTool.
fn hard_mask_outline(mask: &usvg::Mask, transform: Affine) -> Result<Option<BezPath>, dtool_core::CommandError> {
    if mask.mask().is_some() {
        return Ok(None);
    }
    let path = match mask.root().children() {
        [] => return Ok(Some(BezPath::new())),
        [usvg::Node::Path(path)] => path,
        _ => return Ok(None),
    };
    if !path.is_visible() {
        return Ok(Some(BezPath::new()));
    }
    let Some(fill) = path.fill() else { return Ok(None) };
    let usvg::Paint::Color(colour) = fill.paint() else { return Ok(None) };
    if path.stroke().is_some() || fill.opacity().get() != 1.0
        || (mask.kind() == usvg::MaskType::Luminance && *colour != usvg::Color::white())
    {
        return Ok(None);
    }
    let mut outline = clip_leaf(path, Affine::IDENTITY)?;
    let extent = mask.rect();
    let rect = kurbo::Rect::new(f64::from(extent.left()), f64::from(extent.top()), f64::from(extent.right()), f64::from(extent.bottom()));
    let ink = outline.bounding_box();
    if ink.x0 < rect.x0 || ink.y0 < rect.y0 || ink.x1 > rect.x1 || ink.y1 > rect.y1 {
        outline = intersect_clips(outline, rect.to_path(0.0))?;
    }
    Ok(Some(transform * outline))
}

fn raster_mask(
    mask: Option<&usvg::Mask>, transform: Affine, pos: [f32; 2], size: [f32; 2],
    clips: &[BezPath], budget: &mut u32,
) -> Option<PixelSelection> {
    use resvg::tiny_skia::{Color, FillRule as SkiaFill, Mask, MaskType, PathBuilder, Pixmap, Transform};

    if !size.iter().all(|size| size.is_finite() && *size > 0.0) || !transform.is_finite() {
        return None;
    }
    let [w, h] = size.map(f64::from);
    let scale = 2.0_f64.min(MAX_MASK_SIDE / w.max(h));
    let [width, height] = [w, h].map(|side| (side * scale).ceil().clamp(1.0, MAX_MASK_SIDE) as u32);
    let pixels = width * height;
    if pixels > *budget {
        return None;
    }
    let world_to_pixels = Affine::scale_non_uniform(f64::from(width) / w, f64::from(height) / h)
        * Affine::translate((-f64::from(pos[0]), -f64::from(pos[1])));
    let [a, b, c, d, x, y] = (world_to_pixels * transform).as_coeffs().map(|n| n as f32);
    let raster = Transform::from_row(a, b, c, d, x, y);
    if !raster.is_finite() {
        return None;
    }
    let mut coverage = vec![255; pixels as usize];
    let mut region = Mask::new(width, height)?;
    let multiply = |coverage: &mut [u8], mask: &Mask| {
        for (alpha, &value) in coverage.iter_mut().zip(mask.data()) {
            *alpha = ((u16::from(*alpha) * u16::from(value) + 127) / 255) as u8;
        }
    };
    for clip in clips {
        *budget = budget.checked_sub(pixels)?;
        let mut path = PathBuilder::new();
        let point = |p: Point| {
            let p = world_to_pixels * p;
            [p.x as f32, p.y as f32]
        };
        for element in clip.elements() {
            match *element {
                PathEl::MoveTo(p) => { let [x, y] = point(p); path.move_to(x, y); }
                PathEl::LineTo(p) => { let [x, y] = point(p); path.line_to(x, y); }
                PathEl::QuadTo(c, p) => { let [a, b] = point(c); let [x, y] = point(p); path.quad_to(a, b, x, y); }
                PathEl::CurveTo(c, d, p) => {
                    let [a, b] = point(c); let [e, f] = point(d); let [x, y] = point(p);
                    path.cubic_to(a, b, e, f, x, y);
                }
                PathEl::ClosePath => path.close(),
            }
        }
        region.clear();
        if let Some(path) = path.finish() {
            region.fill_path(&path, SkiaFill::EvenOdd, true, Transform::identity());
        }
        multiply(&mut coverage, &region);
    }
    if let Some(mut mask) = mask {
        let mut layer = Pixmap::new(width, height)?;
        for depth in 0..MAX_CLIP_DEPTH {
            *budget = budget.checked_sub(pixels)?;
            layer.fill(Color::TRANSPARENT);
            let node = usvg::Node::Group(Box::new(mask.root().clone()));
            if let Some(bounds) = node.abs_layer_bounding_box() {
                // Keep the mask in its owner's coordinate system when rendering the subtree.
                let render = raster.pre_translate(bounds.x(), bounds.y());
                resvg::render_node(&node, render, &mut layer.as_mut())?;
            }
            region.clear();
            region.fill_path(&PathBuilder::from_rect(mask.rect().to_rect()), SkiaFill::Winding, true, raster);
            layer.apply_mask(&region);
            let kind = match mask.kind() {
                usvg::MaskType::Alpha => MaskType::Alpha,
                usvg::MaskType::Luminance => MaskType::Luminance,
            };
            multiply(&mut coverage, &Mask::from_pixmap(layer.as_ref(), kind));
            match mask.mask() {
                Some(next) if depth + 1 < MAX_CLIP_DEPTH => mask = next,
                Some(_) => return None,
                None => break,
            }
        }
    }
    Some(PixelSelection::from_coverage(width, height, coverage))
}

fn vector_mask_outline(
    mask: Option<&usvg::Mask>, transform: Affine, pos: [f32; 2], size: [f32; 2], clips: &[BezPath],
) -> Option<Curve> {
    let mut outline = match mask {
        Some(mask) => hard_mask_outline(mask, transform).ok()??,
        None => clips.first()?.clone(),
    };
    for clip in &clips[usize::from(mask.is_none())..] {
        outline = intersect_clips(outline, clip.clone()).ok()?;
    }
    let [x, y] = pos.map(f64::from);
    let [w, h] = size.map(f64::from);
    let bounds = Rect::new(x, y, x + w, y + h);
    if !bounds.contains_rect(outline.bounding_box()) {
        outline = intersect_clips(outline, bounds.to_path(0.0)).ok()?;
    }
    if outline.elements().is_empty() {
        return None;
    }
    let local = Affine::scale_non_uniform(1.0 / w, 1.0 / h) * Affine::translate((-x, -y));
    Some(Curve::from_kurbo(&(local * outline)))
}

fn add_clip_mask(doc: &mut Document, owner: ObjectId, name: &str, outline: BezPath) {
    let (curve, pos, size) = curve_geometry(outline);
    doc.create_object(Object {
        kind: ObjectKind::Curve(curve),
        name: name.to_owned(),
        parent_id: Some(owner),
        clip_role: ClipRole::Mask,
        fill: Paint::Solid { colour: Colour::new(255, 255, 255, 255) },
        pos,
        size,
        ..Object::default()
    });
}

fn parse_length(value: &str) -> Result<f32, ()> {
    let trimmed = value.trim();
    let unit_prefixes = ["px", "pt", "mm", "cm", "in", "pc", "em", "ex", "%"];
    let mut number_part = trimmed;
    let mut scale = 1.0f32;
    for unit in unit_prefixes {
        if let Some(stripped) = trimmed.strip_suffix(unit) {
            number_part = stripped.trim();
            scale = match unit {
                "pt" => 1.0 / PX_TO_PT_SCALE,
                "in" => 96.0,
                "mm" => 96.0 / 25.4,
                "cm" => 96.0 / 2.54,
                "pc" => 16.0,
                "%" => 1.0,
                _ => 1.0,
            };
            break;
        }
    }
    number_part.parse::<f32>().map(|n| n * scale).map_err(|_| ())
}

pub fn admit_svg(path: &Path) -> Result<crate::ImportProfile, IoError> {
    let metadata = fs::metadata(path).map_err(|e| IoError::from_io(&e))?;
    if metadata.len() > MAX_SVG_SIZE {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let content = fs::read_to_string(path).map_err(|e| IoError::from_io(&e))?;

    let doc = XmlDoc::parse(&content).map_err(|_| IoError::new(IoErrorKind::UnsupportedProfile))?;

    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let width_attr = root.attribute("width");
    let height_attr = root.attribute("height");
    let view_box_attr = root.attribute("viewBox");

    let mut size_found = false;
    let mut width = 0.0f32;
    let mut height = 0.0f32;

    if let Some(w) = width_attr {
        if let Some(h) = height_attr {
            if let Ok(w_val) = parse_length(w) {
                if let Ok(h_val) = parse_length(h) {
                    width = w_val;
                    height = h_val;
                    size_found = true;
                }
            }
        }
    }

    if !size_found {
        if let Some(vb) = view_box_attr {
            let parts: Vec<&str> = vb.split_whitespace().collect();
            if parts.len() == 4 {
                if let (Ok(w), Ok(h)) = (parts[2].parse::<f32>(), parts[3].parse::<f32>()) {
                    width = w;
                    height = h;
                    size_found = true;
                }
            }
        }
    }

    if !size_found {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let _admitted_width = width;
    let _admitted_height = height;
    let version = root.attribute("version").unwrap_or("1.1").to_string();

    Ok(crate::ImportProfile {
        format: crate::ImportFormat::Svg,
        version,
        encrypted: false,
        pages: 1,
        supported: true,
        reason_key: None,
    })
}

pub fn import_svg(path: &Path) -> Result<(Document, ImportReport), IoError> {
    let profile = admit_svg(path)?;
    if !profile.supported {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let content = fs::read_to_string(path).map_err(|e| IoError::from_io(&e))?;
    import_svg_data(&content, path.parent())
}

pub fn import_svg_data(content: &str, resources_dir: Option<&Path>) -> Result<(Document, ImportReport), IoError> {
    let options = usvg::Options { resources_dir: resources_dir.map(Path::to_path_buf), font_size: 16.0, ..Default::default() };
    import_svg_with_options(content, &options)
}

pub fn import_clipboard_svg(content: &str) -> Result<(Document, ImportReport), IoError> {
    if content.len() as u64 > MAX_SVG_SIZE {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }
    let mut options = usvg::Options { font_size: 16.0, ..Default::default() };
    // Clipboard SVG has no user-selected source directory. Allow embedded images only.
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    import_svg_with_options(content, &options)
}

fn import_svg_with_options(content: &str, options: &usvg::Options<'_>) -> Result<(Document, ImportReport), IoError> {
    let tree = usvg::Tree::from_data(content.as_bytes(), options).map_err(|_| {
        IoError::new(IoErrorKind::InvalidFormat)
    })?;

    let size = tree.size();
    let mut state = SvgImport {
        doc: Document::new([size.width(), size.height()], Units::Px, 96.0, 1),
        report: ImportReport::default(),
        patterns: PatternTiles::default(),
        mask_pixels_remaining: MAX_MASK_PIXELS,
    };

    process_group(tree.root(), &mut state, None, 0, true);
    Ok((state.doc, state.report))
}

fn group_blend(group: &usvg::Group) -> BlendMode {
    match group.blend_mode() {
        usvg::BlendMode::Normal if !group.should_isolate() => BlendMode::PassThrough,
        usvg::BlendMode::Normal => BlendMode::Normal,
        usvg::BlendMode::Multiply => BlendMode::Multiply,
        usvg::BlendMode::Screen => BlendMode::Screen,
        usvg::BlendMode::Overlay => BlendMode::Overlay,
        usvg::BlendMode::Darken => BlendMode::Darken,
        usvg::BlendMode::Lighten => BlendMode::Lighten,
        usvg::BlendMode::ColorDodge => BlendMode::ColorDodge,
        usvg::BlendMode::ColorBurn => BlendMode::ColorBurn,
        usvg::BlendMode::HardLight => BlendMode::HardLight,
        usvg::BlendMode::SoftLight => BlendMode::SoftLight,
        usvg::BlendMode::Difference => BlendMode::Difference,
        usvg::BlendMode::Exclusion => BlendMode::Exclusion,
        usvg::BlendMode::Hue => BlendMode::Hue,
        usvg::BlendMode::Saturation => BlendMode::Saturation,
        usvg::BlendMode::Color => BlendMode::Color,
        usvg::BlendMode::Luminosity => BlendMode::Luminosity,
    }
}

fn svg_rect(rect: usvg::Rect) -> Rect {
    Rect::new(rect.left().into(), rect.top().into(), rect.right().into(), rect.bottom().into())
}

struct SvgShadow {
    sigma: [f32; 2],
    offset: Vec2,
    colour: usvg::Color,
    opacity: f32,
}

fn svg_shadow(primitives: &[usvg::filter::Primitive]) -> Option<SvgShadow> {
    use usvg::filter::{CompositeOperator, Input, Kind};

    if let [primitive] = primitives
        && let Kind::DropShadow(shadow) = primitive.kind()
        && *shadow.input() == Input::SourceGraphic
    {
        return Some(SvgShadow {
            sigma: [shadow.std_dev_x().get(), shadow.std_dev_y().get()],
            offset: Vec2::new(shadow.dx().into(), shadow.dy().into()),
            colour: shadow.color(), opacity: shadow.opacity().get(),
        });
    }
    let [blur, offset, flood, composite, merge] = primitives else { return None };
    let (Kind::GaussianBlur(soften), Kind::Offset(move_shadow), Kind::Flood(colour), Kind::Composite(mask), Kind::Merge(over)) =
        (blur.kind(), offset.kind(), flood.kind(), composite.kind(), merge.kind()) else { return None };
    let reference = |input: &Input, source: &usvg::filter::Primitive| {
        matches!(input, Input::Reference(name) if name == source.result())
    };
    let [shadow, Input::SourceGraphic] = over.inputs() else { return None };
    // Match the equivalent SVG drop shadow graph without interpreting arbitrary filters.
    if *soften.input() != Input::SourceAlpha || !reference(move_shadow.input(), blur)
        || mask.operator() != CompositeOperator::In || !reference(mask.input1(), flood)
        || !reference(mask.input2(), offset) || !reference(shadow, composite)
        || primitives.iter().any(|primitive| primitive.rect() != merge.rect())
    {
        return None;
    }
    Some(SvgShadow {
        sigma: [soften.std_dev_x().get(), soften.std_dev_y().get()],
        offset: Vec2::new(move_shadow.dx().into(), move_shadow.dy().into()),
        colour: colour.color(), opacity: colour.opacity().get(),
    })
}

fn transformed_gaussian([sx, sy]: [f32; 2], transform: Affine) -> [f32; 3] {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    let scale_x = a.hypot(b);
    let scale_y = c.hypot(d);
    let (sx, sy) = (f64::from(sx), f64::from(sy));
    let (x, y) = (sx * scale_x, sy * scale_y);
    let dot = (a * c + b * d).abs();
    if (x - y).abs() <= x.max(y) * 1e-5 && dot <= scale_x * scale_y * 1e-5 {
        let radius = (2.0 * x) as f32;
        return [radius, radius, 0.0];
    }
    if a.abs() <= scale_x * 1e-7 && d.abs() <= scale_y * 1e-7 {
        return [(2.0 * y) as f32, (2.0 * x) as f32, 0.0];
    }
    let (ux, uy) = (a * sx, b * sx);
    let (vx, vy) = (c * sy, d * sy);
    let (xx, yy, xy) = (ux * ux + vx * vx, uy * uy + vy * vy, ux * uy + vx * vy);
    if xy.abs() <= (xx * yy).sqrt() * 1e-7 {
        [(2.0 * xx.sqrt()) as f32, (2.0 * yy.sqrt()) as f32, 0.0]
    } else if dot <= scale_x * scale_y * 1e-7 {
        [(2.0 * x) as f32, (2.0 * y) as f32,
            b.atan2(a).to_degrees().rem_euclid(180.0) as f32]
    } else {
        // The covariance's eigenvectors give orthogonal passes after shear.
        let major = (xx + yy + (xx - yy).hypot(2.0 * xy)) * 0.5;
        let minor = (ux * vy - uy * vx).powi(2) / major;
        let angle = 0.5 * (2.0 * xy).atan2(xx - yy).to_degrees();
        [(2.0 * major.sqrt()) as f32, (2.0 * minor.sqrt()) as f32, angle.rem_euclid(180.0) as f32]
    }
}

fn svg_filter_effect(group: &usvg::Group, transform: Affine) -> Option<(LayerEffect, Rect, Option<Rect>)> {
    use usvg::filter::{Input, Kind};

    let [filter] = group.filters() else { return None };
    let primitives = filter.primitives();
    let last = primitives.last()?;
    let (kind, opacity, params, [radius, y, blur_angle], shift) = if primitives.len() == 1
        && let Kind::GaussianBlur(blur) = last.kind()
        && *blur.input() == Input::SourceGraphic
    {
        let [radius, y, angle] = transformed_gaussian([blur.std_dev_x().get(), blur.std_dev_y().get()], transform);
        let params = EffectParams::Blur {
            radius, radius_y: (radius != y).then_some(y), angle: (angle != 0.0).then_some(angle), preserve_alpha: false,
        };
        (EffectKind::GaussianBlur, 1.0, params, [radius, y, angle], Vec2::ZERO)
    } else {
        let shadow = svg_shadow(primitives)?;
        let [radius, y, blur_angle] = transformed_gaussian(shadow.sigma, transform);
        let shift = transform * shadow.offset.to_point() - transform * Point::ORIGIN;
        let params = EffectParams::Shadow {
            radius, radius_y: (radius != y).then_some(y), blur_angle: (blur_angle != 0.0).then_some(blur_angle),
            offset: shift.hypot() as f32,
            angle: shift.y.atan2(-shift.x).to_degrees().rem_euclid(360.0) as f32,
            intensity: 0.0, colour: colour_from_usvg(shadow.colour, 1.0), knock_out: false,
        };
        (EffectKind::OuterShadow, shadow.opacity, params, [radius, y, blur_angle], shift)
    };
    let effect = LayerEffect { kind, enabled: true, opacity, blend: BlendMode::Normal, params, band_pass: None };
    if !effect.is_valid() {
        return None;
    }

    let (sin, cos) = blur_angle.to_radians().sin_cos();
    let margin = [
        1.5 * (radius * cos.abs() + y * sin.abs()),
        1.5 * (radius * sin.abs() + y * cos.abs()),
    ];
    let source = svg_rect(group.abs_stroke_bounding_box());
    let ink = source.union((source + shift).inflate(margin[0].into(), margin[1].into()));
    let region = svg_rect(filter.rect().to_rect()).intersect(svg_rect(last.rect().to_rect()));
    let inverse = transform.inverse();
    if !inverse.is_finite() {
        return None;
    }
    let boundary = (!region.contains_rect(inverse.transform_rect_bbox(ink))).then_some(region);
    Some((effect, ink, boundary))
}

struct SvgFilterGroup {
    effect: LayerEffect,
    pos: [f32; 2],
    size: [f32; 2],
    mask: Option<(PixelSelection, Option<Curve>)>,
}

fn svg_filtered_group(group: &usvg::Group, budget: &mut u32) -> Option<SvgFilterGroup> {
    let transform = affine(group.abs_transform());
    let (effect, ink, boundary) = svg_filter_effect(group, transform)?;
    let source = object_bounds(group.abs_stroke_bounding_box());
    if boundary.is_none() && group.clip_path().is_none() && group.mask().is_none() {
        return Some(SvgFilterGroup { effect, pos: source.0, size: source.1, mask: None });
    }
    let visible = boundary.map_or(ink, |r| transform.transform_rect_bbox(r).intersect(ink));
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        let mask = Some((PixelSelection::empty(1, 1), None));
        return Some(SvgFilterGroup { effect, pos: source.0, size: source.1, mask });
    }
    let pos = [visible.x0 as f32, visible.y0 as f32];
    let size = [visible.width() as f32, visible.height() as f32];
    let [a, b, c, d, ..] = transform.as_coeffs();
    let axis_aligned = (b == 0.0 && c == 0.0) || (a == 0.0 && d == 0.0);
    let mut clips = Vec::new();
    if let Some(region) = boundary.filter(|_| !axis_aligned) {
        clips.push(transform * region.to_path(0.0));
    }
    if let Some(clip) = group.clip_path() {
        clips.push(clip_outline(clip, transform, 0).ok()?);
    }
    // Group pixel masks multiply the filtered result. Vector masks clip its input.
    let mask = if clips.is_empty() && group.mask().is_none() {
        (PixelSelection::all(1, 1), None)
    } else {
        let coverage = raster_mask(group.mask(), transform, pos, size, &clips, budget)?;
        let outline = vector_mask_outline(group.mask(), transform, pos, size, &clips);
        (coverage, outline)
    };
    Some(SvgFilterGroup { effect, pos, size, mask: Some(mask) })
}

fn process_group(group: &usvg::Group, state: &mut SvgImport, parent: Option<ObjectId>, image_depth: usize, root: bool) {
    let group_name = group.id();
    let children = group.children();
    let filtered = (!children.is_empty()).then(|| svg_filtered_group(group, &mut state.mask_pixels_remaining)).flatten();
    let filter_applied = filtered.is_some();
    if !group.filters().is_empty() && !filter_applied {
        state.report.losses.push(ImportLoss::new(
            LossKind::Filter,
            1,
            false,
            format!("filter omitted from group {group_name}"),
        ));
    }

    let compositing = group_blend(group);
    let structural = filter_applied || group.clip_path().is_some() || group.mask().is_some()
        || (!root && (!group_name.is_empty() || children.len() > 1));
    let parent = if !children.is_empty() && (structural || compositing != BlendMode::PassThrough) {
        // The imported children already have world coordinates. Keep the layer hierarchy
        // without applying the SVG transform a second time.
        let (effects, pos, size, mask) = filtered.map_or_else(|| {
            let (pos, size) = object_bounds(group.abs_stroke_bounding_box());
            (Vec::new(), pos, size, None)
        }, |group| (vec![group.effect], group.pos, group.size, group.mask));
        let owner = state.doc.create_object(Object {
            kind: ObjectKind::Group,
            name: group_name.to_owned(),
            opacity: group.opacity().get(),
            blend_mode: compositing,
            effects,
            pos,
            size,
            parent_id: parent,
            ..Object::default()
        });
        if let Some((coverage, vector_outline)) = mask {
            state.doc.layer_masks.push(LayerMask { object: owner, coverage, vector_outline });
        }
        Some(owner)
    } else {
        parent
    };
    let child_start = state.doc.objects.len();
    for child in children {
        process_node(child, state, parent, image_depth);
    }
    if filter_applied {
        return;
    }
    let transform = affine(group.abs_transform());
    if let Some(owner) = parent
        && (group.clip_path().is_some() || group.mask().is_some())
        && state.doc.objects[child_start..].iter().any(|object| !object.effects.is_empty())
    {
        let clips = group.clip_path().map(|clip| clip_outline(clip, transform, 0)).transpose();
        if let Ok(clip) = clips {
            // The parent's mask must cover the descendant's filtered layer, including
            // ink outside its original geometry. A vector mask would clip that input.
            let (pos, size) = object_bounds(group.abs_layer_bounding_box().to_rect());
            let mut budget = state.mask_pixels_remaining;
            if let Some(coverage) = raster_mask(group.mask(), transform, pos, size, clip.as_slice(), &mut budget)
                && let Some(layer) = state.doc.find_object_mut(owner.0)
            {
                layer.pos = pos;
                layer.size = size;
                let vector_outline = vector_mask_outline(group.mask(), transform, pos, size, clip.as_slice());
                state.doc.layer_masks.push(LayerMask { object: owner, coverage, vector_outline });
                state.mask_pixels_remaining = budget;
                return;
            }
        }
        state.report.add_loss(LossKind::Mask, 1, false,
            format!("mask on group {group_name} could not be applied after descendant filters"));
    }
    if let (Some(clip), Some(owner)) = (group.clip_path(), parent) {
        match clip_outline(clip, transform, 0) {
            Ok(outline) => add_clip_mask(&mut state.doc, owner, clip.id(), outline),
            Err(error) => state.report.add_loss(LossKind::Mask, 1, false,
                format!("clip path {} omitted from group {group_name}: {}", clip.id(), error.code)),
        }
    }
    if let (Some(mask), Some(owner)) = (group.mask(), parent) {
        match hard_mask_outline(mask, transform) {
            Ok(Some(outline)) => add_clip_mask(&mut state.doc, owner, mask.id(), outline),
            Ok(None) | Err(_) => {
                let (pos, size) = object_bounds(group.abs_stroke_bounding_box());
                if let Some(coverage) = raster_mask(Some(mask), transform, pos, size, &[], &mut state.mask_pixels_remaining) {
                    state.doc.layer_masks.push(LayerMask::new(owner, coverage));
                } else {
                    state.report.add_loss(LossKind::Mask, 1, false,
                        format!("mask {} omitted from group {group_name}", mask.id()));
                }
            }
        }
    }
}

fn process_node(node: &usvg::Node, state: &mut SvgImport, parent: Option<ObjectId>, image_depth: usize) {
    match node {
        usvg::Node::Group(g) => {
            process_group(g, state, parent, image_depth, false);
        }
        usvg::Node::Path(p) => {
            let transform = affine(p.abs_transform());
            let (curve, pos, size) = path_geometry(p, transform);
            let local_paint = Affine::translate((-f64::from(pos[0]), -f64::from(pos[1]))) * transform;
            let mut obj = Object::default();
            obj.kind = ObjectKind::Curve(curve);
            obj.name = p.id().to_string();
            obj.pos = pos;
            obj.size = size;
            obj.parent_id = parent;
            if p.paint_order() == usvg::PaintOrder::StrokeAndFill {
                obj.paint_order = PaintOrder::StrokeUnder;
            }
            if let Some(fill) = p.fill() {
                obj.fill = fill_to_paint(fill, local_paint, state, "path");
                obj.fill_rule = match fill.rule() {
                    usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                    usvg::FillRule::NonZero => FillRule::NonZero,
                };
            }
            if let Some(stroke) = p.stroke() {
                obj.stroke = stroke_to_style(stroke, local_paint, state, "path");
            }
            state.doc.create_object(obj);
            state.report.objects += 1;
        }
        usvg::Node::Text(t) => {
            for chunk in t.chunks() {
                let chunk_text = chunk.text().to_string();
                let mut obj = Object::default();
                obj.kind = ObjectKind::Text(chunk_text);
                obj.name = t.id().to_string();
                let (pos, size) = object_bounds(t.bounding_box());
                obj.pos = pos;
                obj.size = size;
                obj.parent_id = parent;
                let local_paint = Affine::translate((-f64::from(pos[0]), -f64::from(pos[1])));
                if let Some(span) = chunk.spans().first() {
                    if let Some(fill) = span.fill() {
                        obj.fill = fill_to_paint(fill, local_paint, state, "text");
                    }
                    if let Some(stroke) = span.stroke() {
                        obj.stroke = stroke_to_style(stroke, local_paint, state, "text");
                    }
                    let _font_size = span.font_size().get();
                    let _font = span.font();
                }
                state.doc.create_object(obj);
                state.report.texts += 1;
            }
        }
        usvg::Node::Image(img) => {
            if image_depth >= MAX_IMAGE_DEPTH {
                state.report.losses.push(ImportLoss::new(
                    LossKind::Image,
                    1,
                    false,
                    "max image depth exceeded",
                ));
                return;
            }
            match img.kind() {
                usvg::ImageKind::JPEG(data)
                | usvg::ImageKind::PNG(data)
                | usvg::ImageKind::GIF(data)
                | usvg::ImageKind::WEBP(data) => {
                    let (pos, size) = object_bounds(img.bounding_box());
                    let mut obj = Object::default();
                    obj.kind = ObjectKind::Shape;
                    obj.name = img.id().to_string();
                    obj.pos = pos;
                    obj.size = size;
                    obj.parent_id = parent;
                    obj.raster_data = Some(data.as_ref().clone());
                    let image_size = img.size();
                    obj.raster_size = Some((image_size.width() as u32, image_size.height() as u32));
                    state.doc.create_object(obj);
                    state.report.images += 1;
                }
                usvg::ImageKind::SVG(nested) => {
                    if image_depth + 1 >= MAX_IMAGE_DEPTH {
                        state.report.losses.push(ImportLoss::new(
                            LossKind::Image,
                            1,
                            false,
                            "nested svg image exceeded max image depth",
                        ));
                    } else {
                        process_group(nested.root(), state, parent, image_depth + 1, true);
                    }
                }
            }
        }
    }
}
