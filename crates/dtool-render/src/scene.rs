use crate::display::{gradient_parameter, interpolate_stops};
use dtool_core::paint::{Colour, Hatch, LineStyle, Paint, Transparency};
use dtool_core::scene::{apply_matrix, clip_bounds, paint_supported, push_object_with_tolerance, register_scene, scene_with_pages, ObjectPaint, Prim, Scene, MARK_ROLES};
use dtool_core::{CanvasFlags, Document, ObjectId, ViewMode};
use std::collections::HashSet;

pub use crate::display_list::node_matrix;

pub fn build_scene(doc: &Document, flags: &CanvasFlags) -> Scene {
    build_scene_with_tolerance(doc, flags, dtool_core::outline::OUTLINE_TOLERANCE)
}

pub fn build_scene_with_tolerance(doc: &Document, flags: &CanvasFlags, tolerance: f64) -> Scene {
    if doc.stories.is_empty() {
        return build_scene_detail(doc, flags, None, tolerance);
    }
    let library = dtool_text::FontLibrary::shared();
    dtool_text::frame::with_doc_layout_reuse(doc, &library, || build_scene_detail(doc, flags, None, tolerance))
}

pub(crate) fn build_scene_for(doc: &Document, flags: &CanvasFlags, only: Option<ObjectId>) -> Scene {
    build_scene_detail(doc, flags, only, dtool_core::outline::OUTLINE_TOLERANCE)
}

fn build_scene_detail(doc: &Document, flags: &CanvasFlags, only: Option<ObjectId>, tolerance: f64) -> Scene {
    let outline_only = matches!(flags.view_mode, ViewMode::Outline | ViewMode::Wireframe);
    let mut scene = scene_with_pages(doc);
    let list = crate::build_display_list(doc, flags);
    let mut done = HashSet::new();
    let mut table_text = HashSet::new();
    for item in &list.items {
        if only.is_some_and(|id| id != item.object_id) {
            continue;
        }
        let [x, y, w, h] = item.bounds;
        if MARK_ROLES.contains(&item.role.as_str()) {
            scene.prims.push(Prim::Mark { object: item.object_id, rect: [x, y, x + w, y + h], role: item.role.clone() });
            continue;
        }
        if item.role.starts_with("Table") {
            if let Some(object) = doc.get_object(item.object_id) {
                if item.role == "TableFrame" {
                    scene.quads.push((object.id, dtool_core::scene::object_quad(object, &item.matrix)));
                }
                append_table_item(doc, object, item, &mut table_text, &mut scene.prims);
            }
            continue;
        }
        if !done.insert(item.object_id) {
            continue;
        }
        let Some(object) = doc.get_object(item.object_id) else { continue };
        let paint = ObjectPaint { fill: item.fill.clone(), opacity: item.opacity, matrix: item.matrix, outline_only: outline_only || item.outline_only, clip: item.clip };
        push_object_with_tolerance(doc, object, &paint, &node_matrix, &mut scene, tolerance);
    }
    dtool_core::paint::display_scene(doc, &mut scene);
    if let Some(step) = pixel_step(doc, flags) {
        snap_scene(&mut scene, step);
    }
    scene
}

/// Vector geometry used by the table canvas and document exporters.
pub fn table_prims(doc: &Document, object: &dtool_core::Object) -> Vec<Prim> {
    let mut list = crate::DisplayList {
        generation: doc.version,
        items: Vec::new(),
        font_generation: dtool_core::text_lines::font_generation(),
    };
    crate::table::emit_table(&mut list, doc, object, &CanvasFlags::default());
    let mut output = Vec::new();
    let mut text = std::collections::HashSet::new();
    let matrix = node_matrix(object);
    let opacity = dtool_core::scene::effective_opacity(doc, object.id);
    for item in &mut list.items {
        item.matrix = matrix;
        item.opacity = opacity;
        append_table_item(doc, object, item, &mut text, &mut output);
    }
    output
}

fn append_table_item(
    doc: &Document, object: &dtool_core::Object, item: &crate::DisplayItem,
    text: &mut std::collections::HashSet<ObjectId>, output: &mut Vec<Prim>,
) {
    let origin = [item.bounds[0], item.bounds[1]];
    let opacity = item.opacity.clamp(0.0, 1.0);
    let clip = item.clip.map(|clip| clip_bounds(clip, &item.matrix));
    let mut fills = Vec::new();
    if item.role == "TableGlyph" {
        let dtool_core::ObjectKind::Table(table) = &object.kind else { return; };
        if text.insert(object.id) {
            let library = dtool_text::FontLibrary::shared();
            for (paint, mut contours) in dtool_text::table_layout::table_text_contours(&library, doc, table) {
                for point in contours.iter_mut().flatten() {
                    *point = apply_matrix(&item.matrix, [point[0] + object.pos[0], point[1] + object.pos[1]]);
                }
                fills.push(Prim::Fill { object: item.object_id, contours, origin: object.pos, paint, opacity });
            }
        }
    } else if let Some(paint) = &item.fill {
        let [x, y, w, h] = item.bounds;
        let mut contours = match item.table_edge {
            Some(pattern) => table_edge_contours(item.bounds, object.pos, pattern),
            None => vec![vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]],
        };
        for point in contours.iter_mut().flatten() {
            *point = apply_matrix(&item.matrix, *point);
        }
        if !contours.is_empty() {
            fills.push(Prim::Fill { object: item.object_id, contours, origin, paint: paint.clone(), opacity });
        }
    }
    if fills.is_empty() {
        return;
    }
    if clip.is_some() {
        output.push(Prim::Clip { object: item.object_id, rect: clip });
    }
    output.extend(fills);
    if clip.is_some() {
        output.push(Prim::Clip { object: item.object_id, rect: None });
    }
}

fn table_edge_contours(
    bounds: [f32; 4],
    origin: [f32; 2],
    pattern: crate::display_list::TableEdgePattern,
) -> Vec<Vec<[f32; 2]>> {
    let axis = usize::from(!pattern.horizontal);
    let across = 1 - axis;
    let length = bounds[axis + 2];
    let width = bounds[across + 2];
    if !length.is_finite() || !width.is_finite() || length <= 0.0 || width <= 0.0 {
        return Vec::new();
    }
    let [dash, gap] = pattern.style.preset_dash() else { return Vec::new(); };
    let ink = width * dash;
    // Bound tessellation when a very long edge has a nearly zero-width stroke.
    let period = (ink + gap * width).max(length / 512.0);
    let phase = (bounds[axis] - origin[axis]).rem_euclid(period);
    let point = |along, cross| {
        let mut point = [bounds[0], bounds[1]];
        point[axis] += along;
        point[across] += cross;
        point
    };
    let count = ((length + phase) / period).ceil() as usize;
    (0..count).filter_map(|index| {
        let start = index as f32 * period - phase;
        let from = start.max(0.0);
        let to = (start + ink).min(length);
        if from >= to { return None; }
        Some(if pattern.style == LineStyle::Dotted {
            let radius = width * 0.5;
            (0..12).map(|step| {
                let angle = step as f32 * std::f32::consts::TAU / 12.0;
                point((start + radius * (1.0 + angle.cos())).clamp(from, to), radius * (1.0 + angle.sin()))
            }).collect()
        } else {
            vec![point(from, 0.0), point(to, 0.0), point(to, width), point(from, width)]
        })
    }).collect()
}


pub fn pixel_step(doc: &Document, flags: &CanvasFlags) -> Option<f32> {
    let px = dtool_core::rulers::units_per_px(doc.units, doc.dpi);
    let step = match flags.view_mode {
        ViewMode::Pixel => px,
        ViewMode::Pixel2x => px * 0.5,
        _ => return None,
    };
    (step.is_finite() && step > 0.0).then_some(step)
}

fn snap_point(p: &mut [f32; 2], step: f32) {
    p[0] = (p[0] / step).round() * step;
    p[1] = (p[1] / step).round() * step;
}

fn snap_rect(r: &mut [f32; 4], step: f32) {
    for v in r.iter_mut() {
        *v = (*v / step).round() * step;
    }
}

pub fn snap_scene(scene: &mut Scene, step: f32) {
    for prim in &mut scene.prims {
        match prim {
            Prim::Page { rect, .. } | Prim::Mark { rect, .. } => snap_rect(rect, step),
            Prim::Fill { contours, origin, .. } => {
                contours.iter_mut().flatten().for_each(|p| snap_point(p, step));
                snap_point(origin, step);
            }
            Prim::Appearance { inks, origin, .. } => {
                for ink in inks {
                    ink.contours.iter_mut().flatten().for_each(|p| snap_point(p, step));
                }
                snap_point(origin, step);
            }
            Prim::Stroke { points, width, .. } => {
                points.iter_mut().for_each(|p| snap_point(p, step));
                *width = ((*width / step).round() * step).max(step);
            }
            Prim::Image { quad, .. } => quad.iter_mut().for_each(|p| snap_point(p, step)),
            Prim::Handle { at, .. } | Prim::Label { at, .. } => snap_point(at, step),
            Prim::Clip { rect: Some(rect), .. } => snap_rect(rect, step),
            Prim::Text { .. } | Prim::Clip { rect: None, .. } => {}
        }
    }
    for (_, quad) in &mut scene.quads {
        quad.iter_mut().for_each(|p| snap_point(p, step));
    }
}

fn hatch_colour(hatch: &Hatch, x: f64, y: f64) -> Option<Colour> {
    let half = (hatch.weight * 0.5).max(0.25);
    for line in &hatch.lines {
        if line.spacing <= 0.0 {
            continue;
        }
        let dx = x - line.origin[0];
        let dy = y - line.origin[1];
        let (sin, cos) = line.angle_rad.sin_cos();
        let perp = (-dx * sin + dy * cos).rem_euclid(line.spacing);
        if perp <= half || line.spacing - perp <= half {
            return Some(hatch.colour);
        }
    }
    hatch.background
}

fn sample(paint: &Paint, local: [f64; 2]) -> Option<Colour> {
    match paint {
        Paint::Solid { colour } => Some(*colour),
        Paint::Gradient { kind, stops, geom } => Some(interpolate_stops(stops, gradient_parameter(*kind, local[0], local[1], geom))),
        Paint::Hatch(hatch) => hatch_colour(hatch, local[0], local[1]),
        _ => None,
    }
}

thread_local! {
    static ANTIALIAS_FILLS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Use edge coverage for on-screen rendering without changing export sampling.
pub fn with_fill_antialiasing<T>(render: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ANTIALIAS_FILLS.set(self.0);
        }
    }
    let _restore = Restore(ANTIALIAS_FILLS.replace(true));
    render()
}

pub fn rasterize_fill_with_transparency(
    paint: &Paint,
    transparency: &Transparency,
    opacity: f32,
    contours: &[Vec<[f32; 2]>],
    origin: [f32; 2],
    bounds: [f32; 4],
    size: [usize; 2],
) -> Option<Vec<u8>> {
    if !paint_supported(paint) || size[0] == 0 || size[1] == 0 {
        return None;
    }
    let opacity = opacity.clamp(0.0, 1.0);
    let [x0, y0, x1, y1] = bounds;
    let sx = (x1 - x0) / size[0] as f32;
    let sy = (y1 - y0) / size[1] as f32;
    if !(sx > 0.0 && sy > 0.0 && sx.is_finite() && sy.is_finite()) {
        return None;
    }
    let mut rgba = vec![0u8; size[0] * size[1] * 4];
    let mut crossings = Vec::new();
    let mut coverage = vec![0.0_f32; size[0]];
    // A text line can carry hundreds of separate contours. Only contours
    // spanning the current sample row can contribute an edge crossing.
    let contour_rows: Vec<_> = contours.iter().filter(|contour| contour.len() >= 3)
        .map(|contour| {
            let (top, bottom) = contour.iter().fold((f32::INFINITY, f32::NEG_INFINITY),
                |(top, bottom), point| (top.min(point[1]), bottom.max(point[1])));
            (top, bottom, contour)
        }).collect();
    let antialias = ANTIALIAS_FILLS.get();
    let subrows = if antialias { 4 } else { 1 };
    for py in 0..size[1] {
        coverage.fill(0.0);
        for subrow in 0..subrows {
            let y = y0 + (py as f32 + (subrow as f32 + 0.5) / subrows as f32) * sy;
            crossings.clear();
            for &(top, bottom, contour) in &contour_rows {
                if y < top || y >= bottom {
                    continue;
                }
                let mut previous = contour[contour.len() - 1];
                for &[xi, yi] in contour {
                    let [xj, yj] = previous;
                    previous = [xi, yi];
                    if (yi > y) != (yj > y) {
                        let x = (xj - xi) * (y - yi) / (yj - yi) + xi;
                        if !x.is_nan() {
                            crossings.push(x);
                        }
                    }
                }
            }
            crossings.sort_unstable_by(f32::total_cmp);
            if !antialias {
                break;
            }
            // Pair crossings for the even-odd rule and integrate each span over the pixels it covers.
            for span in crossings.chunks_exact(2) {
                let left = ((span[0] - x0) / sx).clamp(0.0, size[0] as f32);
                let right = ((span[1] - x0) / sx).clamp(0.0, size[0] as f32);
                let start = left.floor() as usize;
                let end = right.ceil() as usize;
                for (px, covered) in coverage[start..end].iter_mut().enumerate() {
                    let x = (start + px) as f32;
                    *covered += right.min(x + 1.0) - left.max(x);
                }
            }
        }
        for (px, covered) in coverage.iter().copied().enumerate() {
            let x = x0 + (px as f32 + 0.5) * sx;
            let covered = if antialias {
                covered / subrows as f32
            } else {
                let to_right = crossings.len() - crossings.partition_point(|edge| !(x < *edge));
                (to_right % 2) as f32
            };
            if covered <= 0.0 {
                continue;
            }
            let y = y0 + (py as f32 + 0.5) * sy;
            let local = [f64::from(x - origin[0]), f64::from(y - origin[1])];
            if let Some(c) = sample(paint, local) {
                let alpha = (f32::from(c.a) * transparency.alpha_at(local) * opacity * covered)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                let o = (py * size[0] + px) * 4;
                rgba[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, alpha]);
            }
        }
    }
    Some(rgba)
}

pub fn rasterize_fill(paint: &Paint, contours: &[Vec<[f32; 2]>], origin: [f32; 2], bounds: [f32; 4], size: [usize; 2]) -> Option<Vec<u8>> {
    rasterize_fill_with_transparency(paint, &Transparency::None, 1.0, contours, origin, bounds, size)
}

pub fn install() -> bool {
    register_scene(build_scene, rasterize_fill)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtool_core::document::{Story, Units};
    use dtool_core::object::{Object, ObjectKind};
    use dtool_core::scene::is_convex;
    use dtool_core::StoryId;
    use serde_json::json;

    fn red() -> Paint {
        Paint::Solid { colour: Colour::new(255, 0, 0, 255) }
    }

    fn sample_doc() -> Document {
        let mut doc = Document::new([400.0, 300.0], Units::Px, 72.0, 2);
        doc.create_object(Object { pos: [10.0, 10.0], size: [50.0, 40.0], fill: red(), ..Object::default() });
        let ellipse = serde_json::from_value(json!({ "kind": "ellipse", "size": [60.0, 30.0] })).unwrap();
        let mut stroke = dtool_core::paint::StrokeStyle::default();
        stroke.width = 2.0;
        stroke.paint = red();
        doc.create_object(Object { pos: [100.0, 10.0], size: [60.0, 30.0], fill: red(), stroke, shape: Some(ellipse), ..Object::default() });
        let story = StoryId(1);
        doc.stories.push(Story { id: story, text: "hello\nworld".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
        doc.create_object(Object {
            pos: [10.0, 100.0],
            size: [200.0, 80.0],
            kind: ObjectKind::FrameText { story },
            frame: Some(dtool_core::FrameProps::from_rect([200.0, 80.0])),
            ..Object::default()
        });
        doc.create_object(Object { pos: [250.0, 10.0], size: [40.0, 40.0], raster_data: Some(vec![255; 2 * 2 * 4]), raster_size: Some((2, 2)), ..Object::default() });
        doc
    }

    #[test]
    fn scene_maps_display_list_to_prims() {
        let doc = sample_doc();
        let scene = build_scene(&doc, &CanvasFlags::default());
        let c = scene.counts();
        assert_eq!(c.pages, 2);
        assert_eq!(c.fills, 2);
        assert_eq!(c.strokes, 1);
        assert_eq!(c.images, 1);
        assert_eq!(c.texts, 1);
        assert_eq!(scene.quads.len(), 4);
        let Some(Prim::Text { lines, .. }) = scene.prims.iter().find(|p| matches!(p, Prim::Text { .. })) else { panic!("no text") };
        assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["hello", "world"]);
        let ellipse = scene.prims.iter().find_map(|p| match p {
            Prim::Fill { contours, .. } if contours[0].len() > 8 => Some(contours[0].clone()),
            _ => None,
        });
        assert!(ellipse.is_some_and(|c| is_convex(&c)));
    }

    #[test]
    fn outline_mode_strokes_without_fills() {
        let doc = sample_doc();
        let flags = CanvasFlags { view_mode: ViewMode::Outline, ..CanvasFlags::default() };
        let c = build_scene(&doc, &flags).counts();
        assert_eq!(c.fills, 0);
        assert_eq!(c.images, 0);
        assert_eq!(c.strokes, 4);
    }

    #[test]
    fn gradient_raster_masks_outside() {
        let stop = |offset: f32, v: u8| dtool_core::paint::GradientStop { offset, colour: Colour::new(v, v, v, 255) };
        let paint = Paint::Gradient {
            kind: dtool_core::paint::GradientKind::Linear,
            stops: vec![stop(0.0, 0), stop(1.0, 255)],
            geom: dtool_core::paint::GradientGeom { start: [0.0, 0.0], end: [10.0, 0.0], ratio: 1.0 },
        };
        let tri = vec![vec![[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]]];
        let rgba = rasterize_fill(&paint, &tri, [0.0, 0.0], [0.0, 0.0, 10.0, 10.0], [10, 10]).unwrap();
        assert_eq!(rgba[(9 * 10 + 9) * 4 + 3], 0);
        assert_eq!(rgba[3], 255);
        assert!(rgba[(0 * 10 + 7) * 4] > rgba[0]);
    }

    #[test]
    fn linear_transparency_gradient_sets_pixel_alpha() {
        let fill = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
        let transparency = Transparency::Gradient {
            kind: dtool_core::paint::GradientKind::Linear,
            stops: vec![
                dtool_core::paint::GradientStop { offset: 0.0, colour: Colour::new(255, 255, 255, 0) },
                dtool_core::paint::GradientStop { offset: 1.0, colour: Colour::new(255, 255, 255, 255) },
            ],
            geom: dtool_core::paint::GradientGeom { start: [0.0, 0.0], end: [10.0, 0.0], ratio: 1.0 },
        };
        let rect = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]];
        let rgba = rasterize_fill_with_transparency(&fill, &transparency, 1.0, &rect, [0.0, 0.0], [0.0, 0.0, 10.0, 10.0], [10, 10]).unwrap();
        let alpha = |x: usize, y: usize| rgba[(y * 10 + x) * 4 + 3];
        assert_eq!(rgba[0], 255);
        assert_eq!(rgba[1], 0);
        assert_eq!(rgba[2], 0);
        assert!(alpha(0, 5) < 40, "left alpha {}", alpha(0, 5));
        assert!(alpha(9, 5) > 215, "right alpha {}", alpha(9, 5));
        let mid = alpha(5, 5);
        assert!(mid > 100 && mid < 170, "mid alpha {}", mid);
        assert!(alpha(0, 5) < mid && mid < alpha(9, 5));
        let faded = rasterize_fill_with_transparency(&fill, &transparency, 0.5, &rect, [0.0, 0.0], [0.0, 0.0, 10.0, 10.0], [10, 10]).unwrap();
        let faded_right = faded[(5 * 10 + 9) * 4 + 3];
        assert!(faded_right < alpha(9, 5));
        assert!((faded_right as i32 - (alpha(9, 5) as i32 / 2)).abs() <= 2);
    }

    #[test]
    fn dimension_and_crop_map_to_label_strokes_and_clip() {
        use dtool_core::object::{Dimension, VectorCrop};
        let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
        let dimension = Dimension { start: [0.0, 0.0], end: [30.0, 0.0], offset: 0.0, length: 30.0, units: "px".to_string() };
        doc.create_object(Object { pos: [10.0, 10.0], size: [30.0, 0.0], kind: ObjectKind::Dimension(dimension), ..Object::default() });
        doc.create_object(Object { pos: [50.0, 60.0], size: [40.0, 40.0], fill: red(), vector_crop: Some(VectorCrop { rect: [5.0, 5.0, 10.0, 10.0] }), ..Object::default() });
        let mut star = serde_json::from_value::<dtool_core::shape::ShapeRecipe>(json!({ "kind": "star", "size": [40.0, 40.0] })).unwrap();
        star.star = serde_json::from_value(json!({ "number_of_points": 5, "outer_radius": 100.0, "inner_radius": 50.0, "curve_left": 0.0, "curve_right": 0.0 })).ok();
        let has_star = star.star.is_some();
        doc.create_object(Object { pos: [100.0, 100.0], size: [40.0, 40.0], fill: red(), shape: Some(star), ..Object::default() });
        let scene = build_scene(&doc, &CanvasFlags::default());
        let c = scene.counts();
        assert_eq!(c.labels, 1);
        assert_eq!(c.strokes, 3);
        assert_eq!(c.clips, 2);
        let Some(Prim::Label { text, .. }) = scene.prims.iter().find(|p| matches!(p, Prim::Label { .. })) else { panic!("no label") };
        assert_eq!(text, "30 px");
        let clip = scene.prims.iter().find_map(|p| match p { Prim::Clip { rect: Some(r), .. } => Some(*r), _ => None });
        assert_eq!(clip, Some([55.0, 65.0, 10.0, 10.0]));
        if has_star {
            let points = scene.prims.iter().rev().find_map(|p| match p { Prim::Fill { contours, .. } => Some(contours[0].len()), _ => None });
            assert_eq!(points, Some(10));
        }
    }
}

#[cfg(test)]
mod hook_tests {
    #[test]
    fn install_registers_scene_hook() {
        assert!(super::install());
        assert!(dtool_core::scene::scene_ready());
    }
}
