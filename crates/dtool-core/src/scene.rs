use crate::commands::appearance::RowBlend;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::outline::{object_contours, OUTLINE_TOLERANCE};
use crate::paint::{Colour, Paint, PaintOrder};
use crate::text_lines::{is_text, text_lines, TextLine};
use crate::{CanvasFlags, Document, ViewMode};
use std::sync::OnceLock;

pub const MARK_ROLES: [&str; 6] = ["Footnote", "Endnote", "Sidenote", "NoteRef", "NoteMark", "IndexMark"];
pub const HAIRLINE: Colour = Colour::new(110, 110, 110, 255);
pub const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub type Quad = [[f32; 2]; 4];

#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceInk {
    pub contours: Vec<Vec<[f32; 2]>>,
    pub paint: Paint,
    pub blend: RowBlend,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Page { index: usize, rect: [f32; 4] },
    Fill { object: ObjectId, contours: Vec<Vec<[f32; 2]>>, origin: [f32; 2], paint: Paint, opacity: f32 },
    Appearance { object: ObjectId, inks: Vec<AppearanceInk>, origin: [f32; 2], opacity: f32, tint: Option<Colour> },
    Stroke { object: ObjectId, points: Vec<[f32; 2]>, closed: bool, width: f32, paint: Paint, opacity: f32 },
    Image { object: ObjectId, quad: Quad, opacity: f32 },
    Text { object: ObjectId, lines: Vec<TextLine>, opacity: f32 },
    Mark { object: ObjectId, rect: [f32; 4], role: String },
    Handle { object: ObjectId, at: [f32; 2] },
    Label { object: ObjectId, at: [f32; 2], angle_deg: f32, text: String, size: f32, paint: Paint, opacity: f32 },
    Clip { object: ObjectId, rect: Option<[f32; 4]> },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub version: u64,
    pub prims: Vec<Prim>,
    pub quads: Vec<(ObjectId, Quad)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneCounts {
    pub pages: usize,
    pub fills: usize,
    pub strokes: usize,
    pub images: usize,
    pub texts: usize,
    pub marks: usize,
    pub handles: usize,
    pub labels: usize,
    pub clips: usize,
}

impl Scene {
    pub fn counts(&self) -> SceneCounts {
        let mut c = SceneCounts::default();
        for prim in &self.prims {
            match prim {
                Prim::Page { .. } => c.pages += 1,
                Prim::Fill { .. } => c.fills += 1,
                Prim::Appearance { inks, .. } => c.fills += inks.len(),
                Prim::Stroke { .. } => c.strokes += 1,
                Prim::Image { .. } => c.images += 1,
                Prim::Text { .. } => c.texts += 1,
                Prim::Mark { .. } => c.marks += 1,
                Prim::Handle { .. } => c.handles += 1,
                Prim::Label { .. } => c.labels += 1,
                Prim::Clip { .. } => c.clips += 1,
            }
        }
        c
    }

    pub fn quad(&self, id: ObjectId) -> Option<Quad> {
        self.quads.iter().find(|(o, _)| *o == id).map(|(_, q)| *q)
    }
}

pub type BuildScene = fn(&Document, &CanvasFlags) -> Scene;
pub type RasterizeFill = fn(&Paint, &[Vec<[f32; 2]>], [f32; 2], [f32; 4], [usize; 2]) -> Option<Vec<u8>>;

static BUILD: OnceLock<BuildScene> = OnceLock::new();
static RASTER: OnceLock<RasterizeFill> = OnceLock::new();

pub fn register_scene(build: BuildScene, raster: RasterizeFill) -> bool {
    let a = BUILD.set(build).is_ok() || BUILD.get().is_some();
    let b = RASTER.set(raster).is_ok() || RASTER.get().is_some();
    a && b
}

pub fn scene_ready() -> bool {
    BUILD.get().is_some()
}

pub fn build_scene(doc: &Document, flags: &CanvasFlags) -> Scene {
    let mut scene = match BUILD.get() {
        Some(build) => build(doc, flags),
        None => estimate_scene(doc, flags),
    };
    crate::clip_mask::apply(doc, flags, &mut scene);
    scene
}

pub fn rasterize_fill(paint: &Paint, contours: &[Vec<[f32; 2]>], origin: [f32; 2], bounds: [f32; 4], size: [usize; 2]) -> Option<Vec<u8>> {
    RASTER.get().and_then(|r| r(paint, contours, origin, bounds, size))
}

pub fn apply_matrix(m: &[f32; 6], p: [f32; 2]) -> [f32; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

pub fn object_quad(object: &Object, matrix: &[f32; 6]) -> Quad {
    let [x, y] = object.pos;
    let [w, h] = object.size.map(f32::abs);
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| apply_matrix(matrix, p))
}

pub fn has_raster(object: &Object) -> bool {
    raster_len(object).is_some()
}

fn raster_len(object: &Object) -> Option<usize> {
    let data = object.raster_data.as_ref()?;
    let (w, h) = object.raster_size?;
    let count = (w as usize) * (h as usize);
    (count > 0 && (data.len() == count * 4 || data.len() == count * 3)).then_some(data.len() / count)
}

pub fn raster_rgba(object: &Object) -> Option<([usize; 2], Vec<u8>)> {
    let channels = raster_len(object)?;
    let data = object.raster_data.as_ref()?;
    let (w, h) = object.raster_size?;
    let size = [w as usize, h as usize];
    if channels == 4 {
        return Some((size, data.clone()));
    }
    let mut rgba = Vec::with_capacity(size[0] * size[1] * 4);
    for px in data.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Some((size, rgba))
}

pub fn page_rects(doc: &Document) -> Vec<[f32; 4]> {
    let flow = doc.layout_artboard_origins.is_some().then(|| crate::data_merge::layout::flow_origins(doc));
    doc.pages.iter().enumerate().map(|(index, page)| {
        match (&flow, page.artboard_id) {
            (Some(origins), None) => {
                let [x, y] = origins[index];
                [x, y, page.size[0], page.size[1]]
            }
            _ => crate::data_merge::layout::page_rect(doc, index),
        }
    }).collect()
}

pub fn along(points: &[[f32; 2]], t: f32) -> Option<[f32; 2]> {
    let lengths: Vec<f32> = points.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).collect();
    let total: f32 = lengths.iter().sum();
    if total <= 0.0 {
        return points.first().copied();
    }
    let mut left = total * t.clamp(0.0, 1.0);
    for (i, len) in lengths.iter().enumerate() {
        if left <= *len && *len > 0.0 {
            let k = left / len;
            let (a, b) = (points[i], points[i + 1]);
            return Some([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]);
        }
        left -= len;
    }
    points.last().copied()
}

pub fn doc_contours(object: &Object, matrix: &[f32; 6]) -> Vec<(Vec<[f32; 2]>, bool)> {
    object_contours(object, OUTLINE_TOLERANCE)
        .into_iter()
        .map(|(points, closed)| (points.into_iter().map(|p| apply_matrix(matrix, [p[0] + object.pos[0], p[1] + object.pos[1]])).collect(), closed))
        .collect()
}

fn display_contours(object: &Object, matrix: &[f32; 6], tolerance: f64) -> Vec<(Vec<[f32; 2]>, bool)> {
    if tolerance < OUTLINE_TOLERANCE && let Some(path) = crate::outline::object_outline(object) {
        let [x, y] = object.pos.map(f64::from);
        let path = kurbo::Affine::new(matrix.map(f64::from)) * kurbo::Affine::translate((x, y)) * path;
        return crate::outline::flatten_subpaths(&path, tolerance).into_iter()
            .map(|(points, closed)| (points.into_iter().map(|p| p.map(|v| v as f32)).collect(), closed))
            .collect();
    }
    doc_contours(object, matrix)
}

pub fn warped_contours(doc: &Document, object: &Object, matrix: &[f32; 6]) -> Option<Vec<(Vec<[f32; 2]>, bool)>> {
    if object.parent_id.is_none() || doc.export_defaults.get(crate::commands::warp::STORE_KEY).is_none() {
        return None;
    }
    let outline = crate::outline::object_outline(object)?;
    let [x, y] = object.pos.map(f64::from);
    let world = kurbo::Affine::new(matrix.map(f64::from)) * kurbo::Affine::translate((x, y)) * outline;
    let warped = crate::commands::warp::live_warp_outline(doc, object.id, &world)?;
    Some(crate::outline::flatten_subpaths(&warped, OUTLINE_TOLERANCE).into_iter().map(|(points, closed)| {
        (points.into_iter().map(|[x, y]| [x as f32, y as f32]).collect(), closed)
    }).collect())
}

pub fn text_path_guide(doc: &Document, id: ObjectId, path: &Object) -> Vec<(Vec<[f32; 2]>, bool)> {
    let contours = crate::outline::text_path_contours(path);
    let Some(warp) = crate::commands::warp::LiveWarp::for_object(doc, id) else { return contours };
    let mut outline = kurbo::BezPath::new();
    for (points, closed) in contours {
        let Some((first, rest)) = points.split_first() else { continue };
        outline.move_to((f64::from(first[0]), f64::from(first[1])));
        for &[x, y] in rest {
            outline.line_to((f64::from(x), f64::from(y)));
        }
        if closed {
            outline.close_path();
        }
    }
    crate::outline::flatten_subpaths(&warp.outline(&outline), OUTLINE_TOLERANCE)
        .into_iter().map(|(points, closed)| {
            (points.into_iter().map(|[x, y]| [x as f32, y as f32]).collect(), closed)
        }).collect()
}

pub struct ObjectPaint {
    pub fill: Option<Paint>,
    pub opacity: f32,
    pub matrix: [f32; 6],
    pub outline_only: bool,
    pub clip: Option<[f32; 4]>,
}

pub fn clip_bounds(clip: [f32; 4], matrix: &[f32; 6]) -> [f32; 4] {
    let [x, y, w, h] = clip;
    let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| apply_matrix(matrix, p));
    let b = contours_bounds(&[corners.to_vec()]).unwrap_or([x, y, x + w, y + h]);
    [b[0], b[1], b[2] - b[0], b[3] - b[1]]
}

pub fn push_object(doc: &Document, object: &Object, paint: &ObjectPaint, matrix_of: &dyn Fn(&Object) -> [f32; 6], scene: &mut Scene) {
    push_object_with_tolerance(doc, object, paint, matrix_of, scene, OUTLINE_TOLERANCE);
}

pub fn push_object_with_tolerance(doc: &Document, object: &Object, paint: &ObjectPaint,
    matrix_of: &dyn Fn(&Object) -> [f32; 6], scene: &mut Scene, tolerance: f64)
{
    if crate::tone::attached(doc, object) { return; }
    let clip = paint.clip.map(|c| clip_bounds(c, &paint.matrix));
    if let Some(rect) = clip {
        scene.prims.push(Prim::Clip { object: object.id, rect: Some(rect) });
    }
    push_object_body(doc, object, paint, matrix_of, scene, tolerance);
    if clip.is_some() {
        scene.prims.push(Prim::Clip { object: object.id, rect: None });
    }
}

fn push_dimension(object: &Object, paint: &ObjectPaint, scene: &mut Scene) {
    let Some(geometry) = crate::outline::dimension_geometry(object) else { return };
    let colour = if paint.outline_only { Paint::Solid { colour: HAIRLINE } } else { paint.fill.clone().unwrap_or(Paint::Solid { colour: Colour::new(0, 0, 0, 255) }) };
    let opacity = paint.opacity.clamp(0.0, 1.0);
    let width = if paint.outline_only { 0.0 } else { object.stroke.width.max(1.0) };
    let point = |p: [f64; 2]| apply_matrix(&paint.matrix, [p[0] as f32, p[1] as f32]);
    let mut segments = vec![geometry.line];
    segments.extend(geometry.extensions.iter().copied());
    segments.extend(geometry.ticks.iter().copied());
    for [a, b] in segments {
        scene.prims.push(Prim::Stroke { object: object.id, points: vec![point(a), point(b)], closed: false, width, paint: colour.clone(), opacity });
    }
    let m = &paint.matrix;
    let turn = m[1].atan2(m[0]).to_degrees();
    scene.prims.push(Prim::Label {
        object: object.id,
        at: point(geometry.label_at),
        angle_deg: geometry.label_angle_deg as f32 + turn,
        text: geometry.label.clone(),
        size: crate::outline::DIMENSION_LABEL_SIZE as f32,
        paint: colour,
        opacity,
    });
}

fn push_object_body(doc: &Document, object: &Object, paint: &ObjectPaint,
    matrix_of: &dyn Fn(&Object) -> [f32; 6], scene: &mut Scene, tolerance: f64)
{
    let id = object.id;
    let opacity = paint.opacity.clamp(0.0, 1.0);
    let outline_only = paint.outline_only;
    if !outline_only && let Some(ink) = crate::path_brush::Ink::new(doc, object) {
        let image = Prim::Image { object: id, quad: ink.quad(), opacity };
        let under = matches!(object.paint_order, PaintOrder::StrokeUnder);
        if under {
            scene.prims.push(image.clone());
        }
        let mut base = object.clone();
        base.path_brush = None;
        base.stroke.paint = Paint::None;
        push_object_body(doc, &base, paint, matrix_of, scene, tolerance);
        if !under {
            scene.prims.push(image);
        }
        return;
    }
    scene.quads.push((id, object_quad(object, &paint.matrix)));
    if matches!(object.kind, ObjectKind::Group) {
        return;
    }
    if matches!(object.kind, ObjectKind::Dimension(_)) {
        push_dimension(object, paint, scene);
        return;
    }
    if let ObjectKind::PathText(data) = &object.kind {
        if let Some(path) = doc.get_object(data.path) {
            let contours = crate::outline::path_text_layout_contours(object, path);
            let layout_to_doc = crate::geometry::text_matrix(object);
            if let Some((mut points, closed)) = contours.into_iter().next() {
                if closed { points.push(points[0]); }
                let warp = crate::commands::warp::LiveWarp::for_object(doc, id);
                for flow in &data.flows {
                    for t in [flow.start_pct, flow.end_pct] {
                        let t = if path.id == id && closed && flow.end_pct > 1.0 && t > 1.0 { t - 1.0 }
                            else if t.abs() > 1.0 { t / 100.0 } else { t };
                        if let Some(at) = along(&points, t) {
                            let at = crate::geometry::apply_matrix(&layout_to_doc, at);
                            let at = warp.as_ref().map_or(at, |warp| warp.point(at.map(f64::from)).map(|value| value as f32));
                            scene.prims.push(Prim::Handle { object: id, at });
                        }
                    }
                }
            }
        }
        push_text(doc, object, opacity, scene);
        return;
    }
    if !outline_only && crate::commands::appearance::has_layers(object)
        && let Some(paths) = crate::commands::appearance::vector_paths(doc, object, paint.fill.as_ref().unwrap_or(&Paint::None), false)
    {
        let isolated = opacity < 1.0 || paths.iter().any(|path| !path.blend.is_normal());
        let mut inks = Vec::new();
        for path in paths {
            let contours = if path.even_odd {
                crate::outline::flatten_subpaths(&path.path, tolerance)
                    .into_iter().map(|(points, _)| points.into_iter().map(|point| point.map(|value| value as f32)).collect()).collect()
            } else {
                crate::boolean::stroke_outline_contours(&path.path).unwrap_or_default()
            };
            if contours.is_empty() { continue; }
            let mut fill = path.paint;
            if let Paint::Gradient { geom, .. } = &mut fill {
                let [x, y] = object.pos.map(f64::from);
                *geom = geom.transform(kurbo::Affine::translate((-x, -y)).as_coeffs());
            }
            if isolated {
                inks.push(AppearanceInk { contours, paint: fill, blend: path.blend });
            } else {
                scene.prims.push(Prim::Fill { object: id, contours, origin: object.pos, paint: fill, opacity });
            }
        }
        if !inks.is_empty() {
            scene.prims.push(Prim::Appearance { object: id, inks, origin: object.pos, opacity, tint: None });
        }
        return;
    }
    let contours = frame_contours(doc, object, &paint.matrix)
        .unwrap_or_else(|| warped_contours(doc, object, &paint.matrix).unwrap_or_else(|| display_contours(object, &paint.matrix, tolerance)));
    let closed: Vec<Vec<[f32; 2]>> = contours.iter().filter(|(p, c)| *c && p.len() > 2).map(|(p, _)| p.clone()).collect();
    let mapped_paint = |mut value: Paint| {
        if paint.matrix != crate::geometry::IDENTITY {
            let origin = object.pos;
            let translation = [1.0, 0.0, 0.0, 1.0, origin[0], origin[1]];
            let inverse_translation = [1.0, 0.0, 0.0, 1.0, -origin[0], -origin[1]];
            let world = crate::geometry::compose(&paint.matrix, &translation);
            let transform = crate::geometry::compose(&inverse_translation, &world).map(f64::from);
            match &mut value {
                Paint::Gradient { geom, .. } => *geom = geom.transform(transform),
                Paint::Bitmap(bitmap) => {
                    bitmap.matrix = (kurbo::Affine::new(transform) * kurbo::Affine::new(bitmap.matrix)).as_coeffs();
                }
                _ => {}
            }
        }
        value
    };
    let mut strokes = Vec::new();
    if outline_only {
        for (points, closed) in &contours {
            strokes.push(Prim::Stroke { object: id, points: points.clone(), closed: *closed, width: 0.0, paint: Paint::Solid { colour: HAIRLINE }, opacity: 1.0 });
        }
    } else if object.stroke.width > 0.0 && !matches!(object.stroke.paint, Paint::None) {
        for (points, closed) in &contours {
            let paint = mapped_paint(object.stroke.paint.clone());
            let variable_width = !object.stroke.pressure.is_empty();
            let rounded_curve = matches!(object.kind, ObjectKind::Curve(_))
                && matches!(object.stroke.cap, crate::paint::Cap::Round)
                && matches!(paint, Paint::Solid { .. } | Paint::Gradient { .. });
            if (variable_width || rounded_curve)
                && let Some(outline) = solid_stroke_contours(object, points, *closed, object.stroke.width)
            {
                strokes.push(Prim::Fill { object: id, contours: outline, origin: object.pos, paint, opacity });
            } else {
                strokes.push(Prim::Stroke { object: id, points: points.clone(), closed: *closed, width: object.stroke.width, paint, opacity });
            }
            for head in crate::commands::stroke::arrowheads(&object.stroke, points, *closed, object.stroke.width) {
                let paint = mapped_paint(object.stroke.paint.clone());
                strokes.push(if head.filled {
                    Prim::Fill { object: id, contours: vec![head.points], origin: object.pos, paint, opacity }
                } else {
                    Prim::Stroke { object: id, points: head.points, closed: false, width: head.line_width, paint, opacity }
                });
            }
        }
    }
    let under = matches!(object.paint_order, PaintOrder::StrokeUnder);
    if under {
        scene.prims.append(&mut strokes);
    }
    if let Some(fill) = paint.fill.clone().filter(|p| !outline_only && !closed.is_empty() && !matches!(p, Paint::None)) {
        scene.prims.push(Prim::Fill { object: id, contours: closed, origin: object.pos, paint: mapped_paint(fill), opacity });
    }
    if crate::raster_source::dimensions(doc, object).is_some() && !outline_only {
        match crate::commands::picture_frame::frame_geometry(doc, object) {
            Some(geometry) => {
                let quad = geometry.content_corners().map(|p| apply_matrix(&paint.matrix, p));
                let frame = object_quad(object, &paint.matrix);
                let b = contours_bounds(&[frame.to_vec()]).unwrap_or([frame[0][0], frame[0][1], frame[2][0], frame[2][1]]);
                scene.prims.push(Prim::Clip { object: id, rect: Some([b[0], b[1], b[2] - b[0], b[3] - b[1]]) });
                scene.prims.push(Prim::Image { object: id, quad, opacity });
                scene.prims.push(Prim::Clip { object: id, rect: None });
            }
            None => scene.prims.push(Prim::Image { object: id, quad: object_quad(object, &paint.matrix), opacity }),
        }
    }
    if is_text(&object.kind) {
        push_text(doc, object, opacity, scene);
    }
    if !under {
        scene.prims.append(&mut strokes);
    }
}

fn push_text(doc: &Document, object: &Object, opacity: f32, scene: &mut Scene) {
    let lines = text_lines(doc, object.id);
    if let Some(ink) = warped_text_ink(doc, object, &lines, opacity) {
        scene.prims.extend(ink);
    }
    if !lines.is_empty() {
        // Keep layout information for canvas annotations and text input.
        scene.prims.push(Prim::Text { object: object.id, lines, opacity });
    }
}

pub fn has_live_text_warp(doc: &Document, id: ObjectId) -> bool {
    doc.export_defaults.get(crate::commands::warp::STORE_KEY).is_some()
        && doc.get_object(id).is_some_and(|object| is_warpable_text(&object.kind))
        && crate::commands::warp::LiveWarp::for_object(doc, id).is_some()
}

fn is_warpable_text(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_))
}

pub fn warped_text_ink(doc: &Document, object: &Object, lines: &[crate::text_lines::TextLine], opacity: f32) -> Option<Vec<Prim>> {
    if !is_warpable_text(&object.kind) {
        return None;
    }
    let warp = crate::commands::warp::LiveWarp::for_object(doc, object.id)?;
    let mut ink = Vec::new();
    let mut push = |outline: kurbo::BezPath, paint: Paint| {
        let warped = warp.outline(&outline);
        let contours: Vec<Vec<_>> = crate::outline::flatten_subpaths(&warped, OUTLINE_TOLERANCE)
            .into_iter().map(|(points, _)| points.into_iter().map(|[x, y]| [x as f32, y as f32]).collect()).collect();
        if !contours.is_empty() {
            ink.push(Prim::Fill { object: object.id, contours, origin: object.pos, paint, opacity });
        }
    };
    let decorations = crate::text_lines::text_decorations(doc, object.id);
    for under in [true, false] {
        for decoration in decorations.iter().filter(|decoration| decoration.under_text == under) {
            use kurbo::Shape;
            let [x, y, w, h] = decoration.rect.map(f64::from);
            let matrix = kurbo::Affine::new(crate::geometry::text_matrix(object).map(f64::from));
            push(matrix * kurbo::Rect::new(x, y, x + w, y + h).to_path(OUTLINE_TOLERANCE), decoration.paint.clone());
        }
        if under {
            let ink = crate::text_lines::text_ink(doc, object.id).unwrap_or_else(|| {
                let outlines = crate::text_lines::text_outlines(doc, object.id).unwrap_or_default();
                lines.iter().zip(glyph_groups(lines, outlines)).map(|(line, contours)| {
                    crate::text_lines::TextInk { paint: line.paint.clone(), contours }
                }).collect()
            });
            for crate::text_lines::TextInk { paint, contours } in ink {
                let mut outline = kurbo::BezPath::new();
                for contour in contours {
                    for (index, [x, y]) in contour.into_iter().enumerate() {
                        let point = (f64::from(x), f64::from(y));
                        if index == 0 { outline.move_to(point); } else { outline.line_to(point); }
                    }
                    outline.close_path();
                }
                push(outline, paint);
            }
        }
    }
    Some(ink)
}

pub fn scene_with_pages(doc: &Document) -> Scene {
    Scene {
        version: doc.version,
        prims: page_rects(doc).into_iter().enumerate().map(|(index, rect)| Prim::Page { index, rect }).collect(),
        quads: Vec::new(),
    }
}



pub fn isolate_allows(doc: &Document, flags: &CanvasFlags, id: ObjectId) -> bool {
    let Some(target) = flags.isolate_target else {
        return true;
    };
    if doc.get_object(target).is_none() {
        return true;
    }
    let mut current = Some(id);
    let mut seen = std::collections::HashSet::new();
    while let Some(item) = current {
        if !seen.insert(item) {
            return false;
        }
        if item == target {
            return true;
        }
        current = doc.get_object(item).and_then(|object| object.parent_id);
    }
    false
}

pub fn is_drawn(doc: &Document, id: ObjectId) -> bool {
    effective_pickable(doc, id)
}

pub fn is_drawn_with_flags(doc: &Document, flags: &CanvasFlags, id: ObjectId) -> bool {
    is_drawn(doc, id) && isolate_allows(doc, flags, id)
}

pub fn is_drawn_in_view(doc: &Document, view: &crate::view_state::ViewState, id: ObjectId) -> bool {
    is_drawn_with_flags(doc, &view.canvas_flags, id)
}

pub fn effective_pickable(doc: &Document, id: ObjectId) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut current = Some(id);
    while let Some(id) = current {
        if !seen.insert(id) {
            return false;
        }
        let Some(object) = doc.get_object(id) else {
            return false;
        };
        if !object.pickable {
            return false;
        }
        current = object.parent_id;
    }
    true
}

pub fn effective_opacity(doc: &Document, id: ObjectId) -> f32 {
    let mut seen = std::collections::HashSet::new();
    let mut current = Some(id);
    let mut opacity = 1.0f32;
    while let Some(id) = current {
        if !seen.insert(id) {
            break;
        }
        let Some(object) = doc.get_object(id) else {
            break;
        };
        if object.opacity.is_finite() {
            opacity *= object.opacity.clamp(0.0, 1.0);
        } else {
            opacity = 0.0;
            break;
        }
        current = object.parent_id;
    }
    opacity
}

pub fn estimate_scene(doc: &Document, flags: &CanvasFlags) -> Scene {
    let outline_only = matches!(flags.view_mode, ViewMode::Outline | ViewMode::Wireframe);
    let mut scene = scene_with_pages(doc);
    for object in &doc.objects {
        if !is_drawn_with_flags(doc, flags, object.id) {
            continue;
        }
        let clip = object.vector_crop.map(|crop| [object.pos[0] + crop.rect[0], object.pos[1] + crop.rect[1], crop.rect[2], crop.rect[3]]);
        let fill = if matches!(object.kind, ObjectKind::Dimension(_)) { [&object.stroke.paint, &object.fill].into_iter().find(|p| !matches!(p, Paint::None)).cloned() } else { Some(object.fill.clone()) };
        let paint = ObjectPaint { fill, opacity: effective_opacity(doc, object.id), matrix: IDENTITY, outline_only, clip };
        push_object(doc, object, &paint, &|_| IDENTITY, &mut scene);
    }
    crate::paint::display_scene(doc, &mut scene);
    scene
}

pub fn contours_bounds(contours: &[Vec<[f32; 2]>]) -> Option<[f32; 4]> {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in contours.iter().flatten() {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    (b[0] <= b[2] && b[1] <= b[3]).then_some(b)
}

pub fn is_convex(points: &[[f32; 2]]) -> bool {
    let n = points.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        let c = points[(i + 2) % n];
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn references_resource(paint: &Paint, id: &str) -> bool {
    matches!(paint, Paint::Bitmap(bitmap) if bitmap.resource_id == id)
}

fn resource_source(doc: &Document, resource_id: &str) -> Option<([usize; 2], Vec<u8>)> {
    let resource = doc.resources.iter().find(|r| r.id.0.to_string() == resource_id || (!r.name.is_empty() && r.name == resource_id))?;
    crate::path_brush::resource_pixels(doc, resource)
}

pub fn bitmap_source(doc: &Document, resource_id: &str) -> Option<([usize; 2], Vec<u8>)> {
    resource_source(doc, resource_id).or_else(|| {
        doc.objects().iter()
            .filter(|object| references_resource(&object.fill, resource_id) || references_resource(&object.stroke.paint, resource_id) || object.name == resource_id)
            .find_map(|object| crate::raster_source::rgba(doc, object))
    })
        .filter(|([w, h], data)| *w > 0 && *h > 0 && data.len() == w * h * 4)
}

pub fn rasterize_bitmap(doc: &Document, bitmap: &crate::paint::BitmapPaint, contours: &[Vec<[f32; 2]>], origin: [f32; 2], bounds: [f32; 4], size: [usize; 2]) -> Option<Vec<u8>> {
    if size[0] == 0 || size[1] == 0 {
        return None;
    }
    let ([bw, bh], data) = bitmap_source(doc, &bitmap.resource_id)?;
    let [a, b, c, d, e, f] = bitmap.matrix;
    let det = a * d - b * c;
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let [x0, y0, x1, y1] = bounds;
    let sx = (x1 - x0) / size[0] as f32;
    let sy = (y1 - y0) / size[1] as f32;
    let mut rgba = vec![0u8; size[0] * size[1] * 4];
    for py in 0..size[1] {
        for px in 0..size[0] {
            let x = x0 + (px as f32 + 0.5) * sx;
            let y = y0 + (py as f32 + 0.5) * sy;
            if !inside(contours, x, y) {
                continue;
            }
            let lx = f64::from(x - origin[0]) - e;
            let ly = f64::from(y - origin[1]) - f;
            let u = (d * lx - c * ly) / det;
            let v = (-b * lx + a * ly) / det;
            let ix = (u.floor() as i64).rem_euclid(bw as i64) as usize;
            let iy = (v.floor() as i64).rem_euclid(bh as i64) as usize;
            let src = (iy * bw + ix) * 4;
            let o = (py * size[0] + px) * 4;
            rgba[o..o + 4].copy_from_slice(&data[src..src + 4]);
        }
    }
    Some(rgba)
}

pub fn simple_solid(paint: &Paint, contours: &[Vec<[f32; 2]>]) -> Option<Colour> {
    match paint {
        Paint::Solid { colour } if contours.len() == 1 && is_convex(&contours[0]) => Some(*colour),
        _ => None,
    }
}

pub fn paint_supported(paint: &Paint) -> bool {
    matches!(paint, Paint::Solid { .. } | Paint::Gradient { .. } | Paint::Hatch(_))
}

pub fn representative_colour(paint: &Paint) -> Option<Colour> {
    match paint {
        Paint::Solid { colour } => Some(*colour),
        Paint::Gradient { stops, .. } => stops.first().map(|s| s.colour),
        Paint::Hatch(h) => Some(h.colour),
        _ => None,
    }
}

pub fn inside(contours: &[Vec<[f32; 2]>], x: f32, y: f32) -> bool {
    let mut hit = false;
    for poly in contours {
        let n = poly.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = (poly[i][0], poly[i][1]);
            let (xj, yj) = (poly[j][0], poly[j][1]);
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                hit = !hit;
            }
            j = i;
        }
    }
    hit
}

fn intersect_rect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
}

fn in_clip(clip: Option<[f32; 4]>, x: f32, y: f32) -> bool {
    let Some(c) = clip else {
        return true;
    };
    x >= c[0] && y >= c[1] && x <= c[0] + c[2] && y <= c[1] + c[3]
}

pub(crate) fn doc_point(bounds: [f32; 4], width: u32, height: u32, px: u32, py: u32, view: Option<&crate::view_state::ViewState>) -> [f32; 2] {
    let point = [
        bounds[0] + (px as f32 + 0.5) * bounds[2] / width as f32,
        bounds[1] + (py as f32 + 0.5) * bounds[3] / height as f32,
    ];
    match view {
        Some(state) => crate::view::view_to_doc(state, point),
        None => point,
    }
}

fn raster_span(min: f32, max: f32, origin: f32, step: f32, count: u32) -> std::ops::Range<u32> {
    if !step.is_finite() || step <= 0.0 {
        return 0..count;
    }
    let start = ((min - origin) / step).floor() as u32;
    let end = ((max - origin) / step).ceil() as u32;
    start.min(count)..end.min(count)
}

fn image_window(
    quad: Quad, clip: Option<[f32; 4]>, bounds: [f32; 4], size: [u32; 2],
    view: Option<&crate::view_state::ViewState>,
) -> [std::ops::Range<u32>; 2] {
    let edges = |points: Quad| {
        points.into_iter().fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
            |[left, top, right, bottom], [x, y]| [left.min(x), top.min(y), right.max(x), bottom.max(y)])
    };
    let mut area = edges(quad);
    if let Some([x, y, w, h]) = clip {
        area = [area[0].max(x), area[1].max(y), area[2].min(x + w), area[3].min(y + h)];
    }
    if area[0] > area[2] || area[1] > area[3] {
        return [0..0, 0..0];
    }
    if let Some(view) = view {
        let [left, top, right, bottom] = area;
        let corners = [[left, top], [right, top], [right, bottom], [left, bottom]];
        area = edges(corners.map(|point| crate::view::doc_to_view(view, point)));
    }
    if !area.iter().all(|value| value.is_finite()) {
        return size.map(|count| 0..count);
    }
    [0, 1].map(|axis| {
        let step = bounds[axis + 2] / size[axis] as f32;
        // The source and clip tests still decide exact coverage at the existing
        // pixel centres. Leave a pixel of slack for transformed boundary rounding.
        raster_span(area[axis] - step, area[axis + 2] + step, bounds[axis], step, size[axis])
    })
}

fn blend(pixels: &mut [u8], offset: usize, colour: Colour, opacity: f32) {
    if offset + 3 >= pixels.len() {
        return;
    }
    let alpha = (f32::from(colour.a) / 255.0) * opacity.clamp(0.0, 1.0);
    if !(alpha > 0.0) {
        return;
    }
    let old_a = f32::from(pixels[offset + 3]) / 255.0;
    let out_a = alpha + old_a * (1.0 - alpha);
    if out_a > 0.0 {
        for (i, value) in [colour.r, colour.g, colour.b].iter().enumerate() {
            let s = f32::from(*value) * alpha;
            let d = f32::from(pixels[offset + i]) * old_a * (1.0 - alpha);
            pixels[offset + i] = ((s + d) / out_a).round() as u8;
        }
    }
    pixels[offset + 3] = (out_a * 255.0).round() as u8;
}

fn sample_uv_quad(quad: &Quad, p: [f32; 2]) -> Option<[f32; 2]> {
    let [a, b, c, d] = *quad;
    barycentric(p, a, b, c)
        .or_else(|| barycentric(p, a, c, d))
}

fn barycentric(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> Option<[f32; 2]> {
    let v0 = [c[0] - a[0], c[1] - a[1]];
    let v1 = [b[0] - a[0], b[1] - a[1]];
    let v2 = [p[0] - a[0], p[1] - a[1]];
    let den = v0[0] * v1[1] - v1[0] * v0[1];
    if den.abs() < 1e-8 {
        return None;
    }
    let u = (v2[0] * v1[1] - v1[0] * v2[1]) / den;
    let v = (v0[0] * v2[1] - v2[0] * v0[1]) / den;
    if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
        Some([v, u])
    } else {
        None
    }
}

fn fill_paint(
    doc: &Document,
    pixels: &mut [u8],
    contours: &[Vec<[f32; 2]>],
    origin: [f32; 2],
    paint: &Paint,
    opacity: f32,
    bounds: [f32; 4],
    width: u32,
    height: u32,
    view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>,
    mode: RowBlend,
) {
    let opacity = opacity.clamp(0.0, 1.0);
    sample_fill(doc, contours, origin, paint, bounds, [width, height], view, clip, |offset, colour| {
        match mode {
            RowBlend::Standard(mode) => crate::blend::blend_pixel(pixels, offset,
                [colour.r, colour.g, colour.b, colour.a], opacity, mode),
            RowBlend::Operator(_) => {
                let keep = 1.0 - f32::from(colour.a) / 255.0 * opacity;
                pixels[offset + 3] = (f32::from(pixels[offset + 3]) * keep).round() as u8;
            }
        }
    });
}

fn sample_fill(
    doc: &Document, contours: &[Vec<[f32; 2]>], origin: [f32; 2], paint: &Paint,
    bounds: [f32; 4], [width, height]: [u32; 2], view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>, mut sample: impl FnMut(usize, Colour),
) {
    let Some(region) = contours_bounds(contours) else { return };
    let w = width as usize;
    let step = [bounds[2] / width as f32, bounds[3] / height as f32];
    let (columns, rows) = if view.is_none() {
        (
            raster_span(region[0], region[2], bounds[0], step[0], width),
            raster_span(region[1], region[3], bounds[1], step[1], height),
        )
    } else {
        (0..width, 0..height)
    };
    if columns.is_empty() || rows.is_empty() {
        return;
    }
    let render = |window: [f32; 4], size: [usize; 2]| match paint {
        Paint::Bitmap(bitmap) => rasterize_bitmap(doc, bitmap, contours, origin, window, size),
        _ => rasterize_fill(paint, contours, origin, window, size),
    };
    // Keep the original pixel grid, but rasterize and blend only this path's window.
    // A multi-line text object calls this once per line with a shared destination.
    let size = [columns.len(), rows.len()];
    let rendered = if view.is_none() {
        let window = [
            bounds[0] + columns.start as f32 * step[0],
            bounds[1] + rows.start as f32 * step[1],
            bounds[0] + columns.end as f32 * step[0],
            bounds[1] + rows.end as f32 * step[1],
        ];
        render(window, size)
    } else {
        None
    };
    if let Some(rgba) = rendered {
        if rgba.len() == size[0] * size[1] * 4 {
            for (row, py) in rows.clone().enumerate() {
                for (column, px) in columns.clone().enumerate() {
                    let [x, y] = doc_point(bounds, width, height, px, py, view);
                    if !in_clip(clip, x, y) {
                        continue;
                    }
                    let src = (row * size[0] + column) * 4;
                    if rgba[src + 3] == 0 {
                        continue;
                    }
                    let colour = Colour::new(rgba[src], rgba[src + 1], rgba[src + 2], rgba[src + 3]);
                    sample((py as usize * w + px as usize) * 4, colour);
                }
            }
            return;
        }
    }
    let fallback = representative_colour(paint);
    for py in rows {
        for px in columns.clone() {
            let [x, y] = doc_point(bounds, width, height, px, py, view);
            if x < region[0] || y < region[1] || x > region[2] || y > region[3] || !in_clip(clip, x, y) || !inside(contours, x, y) {
                continue;
            }
            let sampled = view.and_then(|_| render([x - 0.25, y - 0.25, x + 0.25, y + 0.25], [1, 1])).filter(|rgba| rgba.len() == 4 && rgba[3] > 0);
            let colour = match sampled {
                Some(rgba) => Colour::new(rgba[0], rgba[1], rgba[2], rgba[3]),
                None => match fallback.clone() {
                    Some(colour) => colour,
                    None => continue,
                },
            };
            sample(((py * width + px) * 4) as usize, colour);
        }
    }
}

pub(crate) fn blend_single_fill<'a>(
    doc: &Document, prims: impl Iterator<Item = &'a Prim>, bounds: [f32; 4], size: [u32; 2],
    pixels: &mut [u8], blend: (crate::blend::BlendMode, Option<&crate::layer_style::BlendRanges>),
) -> bool {
    if size.contains(&0) || bounds[2] <= 0.0 || bounds[3] <= 0.0 || !bounds.iter().all(|value| value.is_finite()) {
        return false;
    }
    let mut clips = Vec::new();
    let mut fill = None;
    for prim in prims {
        match prim {
            Prim::Clip { rect: Some(rect), .. } => {
                let clipped = clips.last().map_or(*rect, |current| intersect_rect(*current, *rect));
                clips.push(clipped);
            }
            Prim::Clip { rect: None, .. } => { clips.pop(); }
            Prim::Page { .. } | Prim::Mark { .. } | Prim::Handle { .. } => {}
            Prim::Fill { contours, origin, paint, opacity, .. } if fill.is_none() => {
                fill = Some((contours, *origin, paint, opacity.clamp(0.0, 1.0), clips.last().copied()));
            }
            _ => return false,
        }
    }
    let Some((contours, origin, paint, opacity, clip)) = fill else { return false };
    sample_fill(doc, contours, origin, paint, bounds, size, None, clip, |offset, colour| {
        // The regular compositor stores a rounded RGBA sample before blending.
        // Use the same two operations here without allocating a viewport-sized layer.
        let mut source = [0; 4];
        crate::blend::blend_pixel_normal(&mut source, 0, [colour.r, colour.g, colour.b, colour.a], opacity);
        crate::blend_composite::composite_sample(pixels, offset, &source, &blend, 1.0);
    });
    true
}

fn blit_image(
    pixels: &mut [u8],
    doc: &Document,
    object: &Object,
    quad: Quad,
    opacity: f32,
    bounds: [f32; 4],
    width: u32,
    height: u32,
    view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>,
) {
    let frame = crate::commands::picture_frame::frame_geometry(doc, object);
    let brush = crate::path_brush::Ink::new(doc, object);
    let w = object.size[0].abs();
    let h = object.size[1].abs();
    let x0 = object.pos[0].min(object.pos[0] + object.size[0]);
    let y0 = object.pos[1].min(object.pos[1] + object.size[1]);
    let extent = if frame.is_some() {
        quad
    } else if let Some(ink) = &brush {
        let [x, y] = quad[0];
        let [_, _, w, h] = ink.bounds;
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
    } else if w > 0.0 && h > 0.0 {
        [[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h]]
    } else {
        quad
    };
    let [columns, rows] = image_window(extent, clip, bounds, [width, height], view);
    if columns.is_empty() || rows.is_empty() {
        return;
    }
    let source = if let Some(ink) = brush.as_ref() {
        let scale = view.map(|v| v.zoom).unwrap_or_else(|| {
            (width as f32 / bounds[2]).max(height as f32 / bounds[3])
        });
        let size = ink.pixel_size(scale);
        let data = if clip.is_some() { ink.render_clipped(doc, object.id, size) } else { ink.render(size) };
        Some((size, data))
    } else {
        crate::raster_source::rgba(doc, object)
    };
    let Some(([rw, rh], data)) = source else {
        return;
    };
    if rw == 0 || rh == 0 || data.len() != rw * rh * 4 {
        return;
    }
    if let Some(geometry) = frame {
        blit_framed(pixels, &data, [rw, rh], geometry, quad, opacity, bounds, width, height, view, clip, [columns, rows]);
        return;
    }
    let local_sampling = if view.is_none() && brush.is_none()
        && doc.pixel_layers.contains(object.id) && w > 0.0 && h > 0.0
    {
        // Sample in source coordinates so moving the image and its preview together
        // cannot change which side of a source pixel boundary is sampled.
        let origin = [
            (f64::from(bounds[0]) - f64::from(x0)) * rw as f64 / f64::from(w),
            (f64::from(bounds[1]) - f64::from(y0)) * rh as f64 / f64::from(h),
        ];
        let step = [
            (f64::from(bounds[2]) / f64::from(w)) * (rw as f64 / f64::from(width)),
            (f64::from(bounds[3]) / f64::from(h)) * (rh as f64 / f64::from(height)),
        ];
        Some((origin, step))
    } else {
        None
    };
    for py in rows {
        for px in columns.clone() {
            let p = doc_point(bounds, width, height, px, py, view);
            if !in_clip(clip, p[0], p[1]) {
                continue;
            }
            let (sx, sy) = if let Some((origin, step)) = local_sampling {
                let sx = origin[0] + (f64::from(px) + 0.5) * step[0];
                let sy = origin[1] + (f64::from(py) + 0.5) * step[1];
                if !(0.0..rw as f64).contains(&sx) || !(0.0..rh as f64).contains(&sy) {
                    continue;
                }
                (sx as usize, sy as usize)
            } else {
                let uv = if let Some(brush) = &brush {
                    let [_, _, width, height] = brush.bounds;
                    [(p[0] - quad[0][0]) / width, (p[1] - quad[0][1]) / height]
                } else if w > 0.0 && h > 0.0 {
                    [(p[0] - x0) / w, (p[1] - y0) / h]
                } else {
                    match sample_uv_quad(&quad, p) {
                        Some(uv) => uv,
                        None => continue,
                    }
                };
                if !(0.0..1.0).contains(&uv[0]) || !(0.0..1.0).contains(&uv[1]) {
                    continue;
                }
                (((uv[0] * rw as f32) as usize).min(rw - 1), ((uv[1] * rh as f32) as usize).min(rh - 1))
            };
            let src = (sy * rw + sx) * 4;
            let colour = Colour::new(data[src], data[src + 1], data[src + 2], data[src + 3]);
            blend(pixels, ((py * width + px) * 4) as usize, colour, opacity);
        }
    }
}

fn frame_contours(doc: &Document, object: &Object, matrix: &[f32; 6]) -> Option<Vec<(Vec<[f32; 2]>, bool)>> {
    if !matches!(object.kind, ObjectKind::PictureFrame) {
        return None;
    }
    let frame = doc.picture_frames.iter().find(|frame| frame.object == object.id)?;
    if frame.shape != crate::commands::picture_frame::FrameShape::Ellipse {
        return None;
    }
    let w = f64::from(object.size[0].abs());
    let h = f64::from(object.size[1].abs());
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let path = crate::geometry::ellipse_to_bez_path(w * 0.5, h * 0.5, w * 0.5, h * 0.5);
    Some(
        crate::outline::flatten_subpaths(&path, OUTLINE_TOLERANCE)
            .into_iter()
            .map(|(points, closed)| (points.into_iter().map(|p| apply_matrix(matrix, [p[0] as f32 + object.pos[0], p[1] as f32 + object.pos[1]])).collect(), closed))
            .collect(),
    )
}

pub fn parallelogram_uv(quad: &Quad, p: [f32; 2]) -> Option<[f32; 2]> {
    let [a, b, _, d] = *quad;
    let e1 = [b[0] - a[0], b[1] - a[1]];
    let e2 = [d[0] - a[0], d[1] - a[1]];
    let det = e1[0] * e2[1] - e1[1] * e2[0];
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    let r = [p[0] - a[0], p[1] - a[1]];
    Some([(r[0] * e2[1] - r[1] * e2[0]) / det, (e1[0] * r[1] - e1[1] * r[0]) / det])
}

#[allow(clippy::too_many_arguments)]
fn blit_framed(
    pixels: &mut [u8],
    data: &[u8],
    [rw, rh]: [usize; 2],
    geometry: crate::commands::picture_frame::FrameGeometry,
    quad: Quad,
    opacity: f32,
    bounds: [f32; 4],
    width: u32,
    height: u32,
    view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>,
    [columns, rows]: [std::ops::Range<u32>; 2],
) {
    let [_, _, cw, ch] = geometry.content;
    let [_, _, fw, fh] = geometry.frame;
    if !(cw > 0.0 && ch > 0.0 && fw > 0.0 && fh > 0.0) {
        return;
    }
    for py in rows {
        for px in columns.clone() {
            let p = doc_point(bounds, width, height, px, py, view);
            if !in_clip(clip, p[0], p[1]) {
                continue;
            }
            let Some(uv) = parallelogram_uv(&quad, p) else { return };
            if !(0.0..1.0).contains(&uv[0]) || !(0.0..1.0).contains(&uv[1]) {
                continue;
            }
            if !geometry.shows_content(uv) {
                continue;
            }
            let sx = ((uv[0] * rw as f32) as usize).min(rw - 1);
            let sy = ((uv[1] * rh as f32) as usize).min(rh - 1);
            let src = (sy * rw + sx) * 4;
            let colour = Colour::new(data[src], data[src + 1], data[src + 2], data[src + 3]);
            blend(pixels, ((py * width + px) * 4) as usize, colour, opacity);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn composite_text_glyphs(
    doc: &Document,
    pixels: &mut [u8],
    object: ObjectId,
    lines: &[crate::text_lines::TextLine],
    opacity: f32,
    bounds: [f32; 4],
    width: u32,
    height: u32,
    view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>,
) -> bool {
    let Some(contours) = crate::text_lines::text_outlines(doc, object).filter(|_| !lines.is_empty()) else { return false };
    let Some(mut area) = contours_bounds(&contours) else { return true };
    if let Some(view) = view {
        let [left, top, right, bottom] = area;
        let corners = [[left, top], [right, top], [right, bottom], [left, bottom]]
            .map(|point| crate::view::doc_to_view(view, point));
        let Some(mapped) = contours_bounds(&[corners.to_vec()]) else { return true };
        area = mapped;
    }
    // Align the glyph window to the destination grid so cropped exports and
    // previews sample the same edges without allocating a page-sized texture.
    let step = [bounds[2] / width as f32, bounds[3] / height as f32];
    let columns = raster_span(area[0], area[2], bounds[0], step[0], width);
    let rows = raster_span(area[1], area[3], bounds[1], step[1], height);
    let size = [columns.len(), rows.len()];
    if size.contains(&0) {
        return true;
    }
    let window = [
        bounds[0] + columns.start as f32 * step[0],
        bounds[1] + rows.start as f32 * step[1],
        size[0] as f32 * step[0],
        size[1] as f32 * step[1],
    ];
    let Some(glyphs) = rasterize_glyphs(doc, object, lines, contours, window, size, view, clip) else { return false };
    for (row, y) in rows.enumerate() {
        for (column, x) in columns.clone().enumerate() {
            let src = (row * size[0] + column) * 4;
            let colour = Colour::new(glyphs[src], glyphs[src + 1], glyphs[src + 2], glyphs[src + 3]);
            blend(pixels, (y as usize * width as usize + x as usize) * 4, colour, opacity);
        }
    }
    true
}

pub fn text_glyph_bounds(doc: &Document, object: ObjectId) -> Option<[f32; 4]> {
    crate::text_lines::text_outlines(doc, object).and_then(|contours| contours_bounds(&contours))
}

pub fn text_glyph_image(doc: &Document, object: ObjectId, lines: &[crate::text_lines::TextLine], area: [f32; 4], size: [usize; 2]) -> Option<Vec<u8>> {
    let contours = crate::text_lines::text_outlines(doc, object)?;
    text_glyph_image_from_outlines(doc, object, lines, area, size, contours)
}

pub fn text_glyph_image_from_outlines(doc: &Document, object: ObjectId, lines: &[crate::text_lines::TextLine], area: [f32; 4], size: [usize; 2], contours: Vec<Vec<[f32; 2]>>) -> Option<Vec<u8>> {
    let [w, h] = size;
    let bounds = [area[0], area[1], area[2] - area[0], area[3] - area[1]];
    if w == 0 || h == 0 || !(bounds[2] > 0.0) || !(bounds[3] > 0.0) || !bounds.iter().all(|v| v.is_finite()) || lines.is_empty() {
        return None;
    }
    rasterize_glyphs(doc, object, lines, contours, bounds, size, None, None)
}

#[allow(clippy::too_many_arguments)]
fn rasterize_glyphs(
    doc: &Document,
    object: ObjectId,
    lines: &[crate::text_lines::TextLine],
    contours: Vec<Vec<[f32; 2]>>,
    bounds: [f32; 4],
    size: [usize; 2],
    view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>,
) -> Option<Vec<u8>> {
    let pixel_count = size[0].checked_mul(size[1])?;
    // Subpixel coverage keeps thin glyphs and tab rules visible at low zoom.
    let samples = match pixel_count {
        0..=262_144 => 4,
        262_145..=1_048_576 => 2,
        _ => 1,
    };
    let width = u32::try_from(size[0].checked_mul(samples)?).ok()?;
    let height = u32::try_from(size[1].checked_mul(samples)?).ok()?;
    let mut pixels = vec![0u8; pixel_count.checked_mul(samples * samples * 4)?];
    let origin = doc.get_object(object).map_or([0.0, 0.0], |item| item.pos);
    if let Some(ink) = crate::text_lines::text_ink(doc, object) {
        for run in ink {
            fill_paint(doc, &mut pixels, &run.contours, origin, &run.paint, 1.0, bounds, width, height, view, clip, Default::default());
        }
    } else {
        for (line, group) in lines.iter().zip(glyph_groups(lines, contours)) {
            fill_paint(doc, &mut pixels, &group, origin, &line.paint, 1.0, bounds, width, height, view, clip, Default::default());
        }
    }
    if samples == 1 {
        return Some(pixels);
    }
    Some(resolve_glyph_coverage(&pixels, size, samples))
}

fn resolve_glyph_coverage(pixels: &[u8], [width, height]: [usize; 2], samples: usize) -> Vec<u8> {
    let source_stride = width * samples * 4;
    let sample_count = (samples * samples) as u32;
    let mut result = vec![0; width * height * 4];
    for (row, output) in result.chunks_exact_mut(width * 4).enumerate() {
        for (column, target) in output.chunks_exact_mut(4).enumerate() {
            let mut alpha = 0;
            let mut colour = [0; 3];
            for y in row * samples..(row + 1) * samples {
                let start = y * source_stride + column * samples * 4;
                for sample in pixels[start..start + samples * 4].chunks_exact(4) {
                    let coverage = u32::from(sample[3]);
                    alpha += coverage;
                    for channel in 0..3 {
                        colour[channel] += u32::from(sample[channel]) * coverage;
                    }
                }
            }
            if alpha > 0 {
                for channel in 0..3 {
                    target[channel] = ((colour[channel] + alpha / 2) / alpha) as u8;
                }
                target[3] = ((alpha + sample_count / 2) / sample_count) as u8;
            }
        }
    }
    result
}

pub fn glyph_groups(lines: &[crate::text_lines::TextLine], contours: Vec<Vec<[f32; 2]>>) -> Vec<Vec<Vec<[f32; 2]>>> {
    let mut groups: Vec<Vec<Vec<[f32; 2]>>> = vec![Vec::new(); lines.len()];
    if lines.is_empty() {
        return groups;
    }
    for contour in contours.into_iter().filter(|contour| contour.len() > 2) {
        let Some(b) = contours_bounds(std::slice::from_ref(&contour)) else { continue };
        let cy = (b[1] + b[3]) * 0.5;
        let gap = |line: &crate::text_lines::TextLine| {
            let top = line.origin[1] - line.size;
            let bottom = line.origin[1] + line.size * 0.3;
            if cy < top { top - cy } else if cy > bottom { cy - bottom } else { 0.0 }
        };
        let best = (0..lines.len()).min_by(|a, b| gap(&lines[*a]).total_cmp(&gap(&lines[*b]))).unwrap_or(0);
        groups[best].push(contour);
    }
    groups
}

fn fill_text_line(
    pixels: &mut [u8],
    line: &crate::text_lines::TextLine,
    opacity: f32,
    bounds: [f32; 4],
    width: u32,
    height: u32,
    view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>,
) {
    let Some(colour) = representative_colour(&line.paint) else {
        return;
    };
    let x0 = line.origin[0];
    let y1 = line.origin[1];
    let x1 = x0 + line.width.max(line.size * 0.5);
    let y0 = y1 - line.size;
    for py in 0..height {
        for px in 0..width {
            let [x, y] = doc_point(bounds, width, height, px, py, view);
            if x < x0 || x > x1 || y < y0 || y > y1 || !in_clip(clip, x, y) {
                continue;
            }
            blend(pixels, ((py * width + px) * 4) as usize, colour, opacity);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_text_decorations(
    doc: &Document,
    pixels: &mut [u8],
    matrix: &[f32; 6],
    decorations: &[crate::text_lines::TextDecoration],
    under_text: bool,
    opacity: f32,
    bounds: [f32; 4],
    width: u32,
    height: u32,
    view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>,
) {
    if width == 0 || height == 0 || !(bounds[2] > 0.0) || !(bounds[3] > 0.0) {
        return;
    }
    for decoration in decorations.iter().filter(|d| d.under_text == under_text) {
        let Some(colour) = representative_colour(&decoration.paint) else { continue };
        let [x0, y0, w, h] = decoration.rect;
        if ![x0, y0, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
            continue;
        }
        if *matrix != crate::geometry::IDENTITY {
            let contour = crate::geometry::rect_quad(decoration.rect, matrix).to_vec();
            fill_paint(doc, pixels, &[contour], [x0, y0], &decoration.paint, opacity, bounds, width, height, view, clip, Default::default());
            continue;
        }
        let (x1, y1) = (x0 + w, y0 + h);
        let (px0, px1, py0, py1) = if view.is_some() {
            (0, width, 0, height)
        } else {
            let sx = width as f32 / bounds[2];
            let sy = height as f32 / bounds[3];
            let lo = |v: f32, s: f32, o: f32, max: u32| (((v - o) * s - 0.5).floor().max(0.0) as u32).min(max);
            let hi = |v: f32, s: f32, o: f32, max: u32| (((v - o) * s + 0.5).ceil().max(0.0) as u32).min(max);
            (lo(x0, sx, bounds[0], width), hi(x1, sx, bounds[0], width), lo(y0, sy, bounds[1], height), hi(y1, sy, bounds[1], height))
        };
        let thin_x = (x1 - x0) * width as f32 / bounds[2] < 1.0;
        let thin_y = (y1 - y0) * height as f32 / bounds[3] < 1.0;
        let half_x = if thin_x { bounds[2] / width as f32 * 0.5 } else { 0.0 };
        let half_y = if thin_y { bounds[3] / height as f32 * 0.5 } else { 0.0 };
        let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        for py in py0..py1 {
            for px in px0..px1 {
                let [x, y] = doc_point(bounds, width, height, px, py, view);
                let inside_x = if thin_x { (x - cx).abs() <= half_x } else { x >= x0 && x < x1 };
                let inside_y = if thin_y { (y - cy).abs() <= half_y } else { y >= y0 && y < y1 };
                if inside_x && inside_y && in_clip(clip, x, y) {
                    blend(pixels, ((py * width + px) * 4) as usize, colour, opacity);
                }
            }
        }
    }
}

fn stroke_points(
    pixels: &mut [u8],
    points: &[[f32; 2]],
    closed: bool,
    stroke_w: f32,
    paint: &Paint,
    origin: [f32; 2],
    opacity: f32,
    bounds: [f32; 4],
    width: u32,
    height: u32,
    view: Option<&crate::view_state::ViewState>,
    clip: Option<[f32; 4]>,
) {
    let Some(colour) = representative_colour(paint) else {
        return;
    };
    let radius = if stroke_w <= 0.0 { 0.5 } else { stroke_w * 0.5 };
    if points.len() < 2 {
        return;
    }
    let count = if closed { points.len() } else { points.len() - 1 };
    for py in 0..height {
        for px in 0..width {
            let p = doc_point(bounds, width, height, px, py, view);
            if !in_clip(clip, p[0], p[1]) {
                continue;
            }
            let mut hit = false;
            for i in 0..count {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                if distance_to_seg(p, a, b) <= radius {
                    hit = true;
                    break;
                }
            }
            if hit {
                let colour = match paint {
                    Paint::Gradient { kind, stops, geom } => {
                        let t = crate::paint::gradient_parameter(
                            *kind, f64::from(p[0] - origin[0]), f64::from(p[1] - origin[1]), geom,
                        );
                        crate::paint::interpolate_stops(stops, t)
                    }
                    _ => colour,
                };
                blend(pixels, ((py * width + px) * 4) as usize, colour, opacity);
            }
        }
    }
}

/// The filled outline shared by raster composition and vector printing for a centred stroke.
pub fn solid_stroke_contours(
    object: &Object,
    points: &[[f32; 2]],
    closed: bool,
    width: f32,
) -> Option<Vec<Vec<[f32; 2]>>> {
    use crate::boolean::{DEFAULT_TOLERANCE, MAX_BOOLEAN_POINTS, expand_stroke};
    use crate::paint::StrokeAlign;

    let style = &object.stroke;
    if !matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Compound(_))
        || width <= 0.0 || width != style.width || !width.is_finite()
        || style.align != StrokeAlign::Centre
        || (style.pressure.is_empty() && (style.start_arrow.is_some() || style.end_arrow.is_some()))
        || !(2..=MAX_BOOLEAN_POINTS).contains(&points.len())
        || !points.iter().flatten().all(|point| point.is_finite())
    {
        return None;
    }
    let mut path = kurbo::BezPath::new();
    let [x, y] = points[0].map(f64::from);
    path.move_to((x, y));
    for &[x, y] in &points[1..] {
        path.line_to((f64::from(x), f64::from(y)));
    }
    if closed { path.close_path(); }
    if !style.pressure.is_empty() {
        let outline = crate::outline::pressure_stroke_path(&path, style, f64::from(width))?;
        return crate::boolean::stroke_outline_contours(&outline);
    }
    let mut style = style.clone();
    if !style.dash.is_empty() {
        let pattern: Vec<_> = style.dash.drain(..).map(f64::from).collect();
        if pattern.iter().any(|dash| !dash.is_finite() || *dash <= 0.0) {
            return None;
        }
        let shortest = pattern.iter().copied().fold(f64::INFINITY, f64::min);
        let length = path.segments().map(|segment| {
            use kurbo::ParamCurveArclen;
            segment.arclen(DEFAULT_TOLERANCE)
        }).sum::<f64>();
        if length / shortest > MAX_BOOLEAN_POINTS as f64 / 2.0 {
            return None;
        }
        path = kurbo::dash(path.into_iter(), 0.0, &pattern).collect();
    }
    let curves = expand_stroke(&path, &style, DEFAULT_TOLERANCE).ok()?;
    let contours = curves.into_iter().flat_map(|curve| std::iter::once(curve.contour).chain(curve.holes))
        .map(|contour| contour.points.into_iter().map(|point| point.pos.map(|coord| coord as f32)).collect())
        .collect::<Vec<_>>();
    (!contours.is_empty()).then_some(contours)
}

fn distance_to_seg(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 <= 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0)
    };
    let x = a[0] + ab[0] * t - p[0];
    let y = a[1] + ab[1] * t - p[1];
    (x * x + y * y).sqrt()
}

/// Image and text scene clips carry a coarse rectangle for culling. Apply the
/// actual path to their ink before passing it to the shared compositor.
fn draw_vector_masked_ink(
    doc: &Document, pixels: &mut [u8], object: ObjectId, rect: Option<[f32; 4]>,
    bounds: [f32; 4], size: [u32; 2], view: Option<&crate::view_state::ViewState>,
    draw: impl FnOnce(&mut [u8]),
) {
    let Some(rings) = rect.and_then(|_| crate::clip_mask::clip_region(doc, object)) else {
        draw(pixels);
        return;
    };
    if rings.is_empty() {
        return;
    }
    let [width, height] = size;
    let contours: Vec<_> = rings.into_iter().map(|ring| ring.into_iter().map(|point| point.map(|value| value as f32)).collect()).collect();
    let mut mask = vec![0; pixels.len()];
    let white = Paint::Solid { colour: Colour::new(255, 255, 255, 255) };
    fill_paint(doc, &mut mask, &contours, [0.0, 0.0], &white, 1.0, bounds, width, height, view, rect, Default::default());
    let mut ink = vec![0; pixels.len()];
    draw(&mut ink);
    for (pixel, coverage) in ink.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
        pixel[3] = ((u16::from(pixel[3]) * u16::from(coverage[3]) + 127) / 255) as u8;
    }
    crate::blend_composite::composite(pixels, &ink, None, 1.0);
}

pub fn rasterize_scene(
    doc: &Document,
    scene: &Scene,
    bounds: [f32; 4],
    width: u32,
    height: u32,
    view: Option<&crate::view_state::ViewState>,
) -> Vec<u8> {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    if width == 0 || height == 0 || !(bounds[2] > 0.0) || !(bounds[3] > 0.0) || !bounds.iter().all(|value| value.is_finite()) {
        return pixels;
    }
    // B29-T007/T008: object blend modes + blend ranges (bot06 C08-BLEND); None = plain path below.
    if let Some(blended) = crate::blend_composite::rasterize(doc, scene, bounds, width, height, view, rasterize_scene) {
        return blended;
    }
    let mut clips: Vec<Option<[f32; 4]>> = Vec::new();
    for prim in &scene.prims {
        match prim {
            Prim::Clip { rect, .. } => match rect {
                Some(rect) => {
                    let next = clips.last().copied().flatten().map(|cur| intersect_rect(cur, *rect)).unwrap_or(*rect);
                    clips.push(Some(next));
                }
                None => {
                    clips.pop();
                }
            },
            Prim::Page { .. } | Prim::Mark { .. } | Prim::Handle { .. } => {}
            Prim::Fill { contours, origin, paint, opacity, .. } => {
                fill_paint(doc, &mut pixels, contours, *origin, paint, *opacity, bounds, width, height, view, clips.last().copied().flatten(), Default::default());
            }
            Prim::Appearance { inks, origin, opacity, tint, .. } => {
                let mut layer = vec![0; pixels.len()];
                let clip = clips.last().copied().flatten();
                for ink in inks {
                    fill_paint(doc, &mut layer, &ink.contours, *origin, &ink.paint, 1.0, bounds, width, height, view, clip, ink.blend);
                }
                if let Some(tint) = tint {
                    for pixel in layer.chunks_exact_mut(4) {
                        if pixel[3] != 0 {
                            pixel[0] = tint.r;
                            pixel[1] = tint.g;
                            pixel[2] = tint.b;
                            pixel[3] = ((u16::from(pixel[3]) * u16::from(tint.a) + 127) / 255) as u8;
                        }
                    }
                }
                crate::blend_composite::composite(&mut pixels, &layer, None, *opacity);
            }
            Prim::Image { object, quad, opacity } => {
                if let Some(object) = doc.get_object(*object) {
                    let clip = clips.last().copied().flatten();
                    draw_vector_masked_ink(doc, &mut pixels, object.id, clip, bounds, [width, height], view, |target| {
                        blit_image(target, doc, object, *quad, *opacity, bounds, width, height, view, clip);
                    });
                }
            }
            Prim::Text { object, lines, opacity, .. } => {
                if has_live_text_warp(doc, *object) { continue; }
                let path_text = doc.get_object(*object).is_some_and(|item| matches!(item.kind, ObjectKind::PathText(_)));
                let clip = clips.last().copied().flatten();
                let decorations = crate::text_lines::text_decorations(doc, *object);
                let matrix = doc.get_object(*object).map_or(crate::geometry::IDENTITY, crate::geometry::text_matrix);
                draw_vector_masked_ink(doc, &mut pixels, *object, clip, bounds, [width, height], view, |target| {
                    fill_text_decorations(doc, target, &matrix, &decorations, true, *opacity, bounds, width, height, view, clip);
                    if !composite_text_glyphs(doc, target, *object, lines, *opacity, bounds, width, height, view, clip) && !path_text {
                        for line in lines {
                            fill_text_line(target, line, *opacity, bounds, width, height, view, clip);
                        }
                    }
                    fill_text_decorations(doc, target, &matrix, &decorations, false, *opacity, bounds, width, height, view, clip);
                });
            }
            Prim::Stroke { object, points, closed, width: stroke_w, paint, opacity } => {
                let clip = clips.last().copied().flatten();
                let item = doc.get_object(*object);
                if matches!(paint, Paint::Solid { .. } | Paint::Gradient { .. })
                    && let Some(item) = item
                    && let Some(contours) = solid_stroke_contours(item, points, *closed, *stroke_w)
                {
                    fill_paint(doc, &mut pixels, &contours, item.pos, paint, *opacity, bounds, width, height, view, clip, Default::default());
                } else {
                    stroke_points(&mut pixels, points, *closed, *stroke_w, paint, item.map_or([0.0; 2], |object| object.pos), *opacity, bounds, width, height, view, clip);
                }
            }
            Prim::Label { at, text, size, paint, opacity, .. } => {
                let line = crate::text_lines::TextLine {
                    object: ObjectId(0),
                    origin: [at[0], at[1]],
                    text: text.clone(),
                    size: *size,
                    width: text.chars().count() as f32 * (*size * 0.5).max(0.1),
                    para_end: false,
                    paint: paint.clone(),
                };
                fill_text_line(&mut pixels, &line, *opacity, bounds, width, height, view, clips.last().copied().flatten());
            }
        }
    }
    pixels
}
