use crate::dialogs::blend_options::BlendPreview;
use crate::panels::layers::LayerOpacityPreview;
use dtool_core::command::Session;
use dtool_core::commands::layer_style::{staged, StagedEffect};
use dtool_core::layer_style::raster::{
    EffectCache, effect_bounds, effects_key, live_effects, object_effect_bounds, prim_object,
    render_scene_region,
};
use dtool_core::object::ObjectId;
use dtool_core::paint::Colour;
use dtool_core::text_lines::TextLine;
use dtool_core::view::ZoomLimits;
use dtool_core::view_state::ViewState;
use dtool_core::scene::{contours_bounds, object_quad, rasterize_fill, representative_colour, simple_solid, Prim, Scene, IDENTITY};
use dtool_tools::dtool_text::decorations::{object_decorations, DecorationKind, DecorationRect};
use dtool_tools::dtool_text;
use egui::{Color32, Painter, Pos2, Rect, Stroke, Ui};
use std::collections::HashMap;

pub const MAX_TEXTURE: f32 = 1024.0;
pub const MIN_TEXT_PX: f32 = 2.0;
const MAX_PIXEL_PREVIEW_TEXTURE: f32 = 1536.0;
const MAX_VECTOR_TEXTURE: f32 = 4096.0;
const EFFECT_TEXTURE_START: usize = usize::MAX / 2 + 1;
const PAN_MARGIN: usize = 96;
const ZOOM_IDLE_SECONDS: f64 = 0.065;
const NAVIGATION_PREVIEW_PIXELS: f32 = 170_000.0;
const INTERACTIVE_RENDER_BUDGET: std::time::Duration = std::time::Duration::from_millis(12);

type TextureKey = (ObjectId, u64, usize, [usize; 2]);
type GlyphContours = std::sync::Arc<Vec<Vec<[f32; 2]>>>;
type GlyphInk = ([f32; 4], GlyphContours);

pub(crate) fn artboard_caption(
    ui: &Ui, doc: &dtool_core::Document, board: &dtool_core::Object, width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut name = board.name.clone();
    let spread = doc.master_pages.spread(board.id);
    if let Some(spread) = spread
        && let Some(index) = spread.pages.iter().position(|&id| id == board.id)
    {
        name = if index == 0 {
            dtool_core::commands::masters::master_name(board).to_owned()
        } else {
            dtool_strings::format("panel.pages.number", &[("number", &(index + 1).to_string())])
        };
    }
    let mut job = egui::text::LayoutJob::simple_singleline(name, egui::FontId::proportional(11.0), ui.visuals().text_color());
    if spread.is_some() {
        job.wrap = egui::text::TextWrapping::truncate_at_width((width - 4.0).max(1.0));
    }
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

struct GlyphTexture {
    contours: GlyphContours,
    lines: Vec<TextLine>,
    size: [usize; 2],
    texture: egui::TextureHandle,
    used: bool,
}

struct CompositeTexture {
    rotation_deg: Option<f32>,
    bounds: [f32; 4],
    size: [usize; 2],
    filter: egui::TextureOptions,
    staged: Vec<StagedEffect>,
    blend_preview: Option<Vec<BlendPreview>>,
    tone_preview: Option<crate::dialogs::tone_blend::Preview>,
    brush_revision: Option<u64>,
    pan_scale: Option<(f32, Option<f32>)>,
    navigation_preview: bool,
    device_scale: f32,
    detail: i32,
    render_time: std::time::Duration,
    texture: egui::TextureHandle,
    transform_preview: Option<CompositeTransformPreview>,
}

#[derive(Clone, Copy, PartialEq)]
struct CompositeRequest {
    rotation_deg: Option<f32>,
    revision: u64,
    detail: i32,
    bounds: [f32; 4],
    size: [usize; 2],
}

struct CompositeJob {
    request: CompositeRequest,
    result: std::sync::mpsc::Receiver<(egui::ColorImage, std::time::Duration)>,
}

impl CompositeJob {
    fn start(ctx: &egui::Context, request: CompositeRequest, document: dtool_core::Document,
        flags: dtool_core::CanvasFlags, scene: Scene,
    ) -> std::io::Result<Self> {
        let (send, result) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let viewport = ctx.viewport_id();
        std::thread::Builder::new().name("dtool-canvas-composite".into()).spawn(move || {
            let started = std::time::Instant::now();
            let render = || dtool_render::scene::with_fill_antialiasing(|| render_scene_region(
                &document, &flags, &[], &scene, request.bounds, request.size.map(|size| size as u32), 1.0,
            ));
            let pixels = if document.stories.is_empty() { render() } else {
                dtool_text::frame::with_doc_layout_reuse(&document, &dtool_text::FontLibrary::shared(), render)
            };
            let image = egui::ColorImage::from_rgba_unmultiplied(request.size, &pixels);
            let _ = send.send((image, started.elapsed()));
            ctx.request_repaint_of(viewport);
        })?;
        Ok(Self { request, result })
    }
}

struct CompositeTransformPreview {
    args: dtool_core::command::TransformSelectionBoxArgs,
    patches: Vec<CompositeMovePatch>,
    backdrop: Vec<[usize; 4]>,
}

struct CompositeMovePatch {
    pixels: [usize; 4],
    texture: egui::TextureHandle,
}

fn canvas_detail(view: &ViewState) -> i32 {
    // Round the target resolution up so nearby zoom levels can reuse the scene.
    if matches!(view.canvas_flags.view_mode, dtool_core::ViewMode::Pixel | dtool_core::ViewMode::Pixel2x) {
        0
    } else {
        view.zoom.max(1.0).log2().ceil().clamp(0.0, 16.0) as i32
    }
}

fn rotated_fill_scene(document: &dtool_core::Document, scene: &Scene, angle: f32) -> Option<Scene> {
    use dtool_core::paint::{Paint, transform_paint_matrix};

    // Geometry-dependent compositing needs the original document coordinate system.
    if document.objects.iter().any(|object| {
        object.tone_blend.is_some() || object.effects.iter().any(|effect| effect.enabled)
    }) {
        return None;
    }
    let (sin, cos) = f64::from(angle).to_radians().sin_cos();
    let matrix = [cos, sin, -sin, cos, 0.0, 0.0];
    let rotate = |[x, y]: [f32; 2]| {
        let [x, y] = [f64::from(x), f64::from(y)];
        [(cos * x - sin * y) as f32, (sin * x + cos * y) as f32]
    };
    let mut clips = Vec::new();
    let rotate_ink = |contours: &mut Vec<Vec<[f32; 2]>>, paint: &mut Paint, clips: &[[f32; 4]]| {
        if !dtool_core::scene::paint_supported(paint) {
            return None;
        }
        for &clip @ [x, y, w, h] in clips {
            let Some([left, top, right, bottom]) = contours_bounds(contours) else { break; };
            if left < x || top < y || right > x + w || bottom > y + h {
                *contours = dtool_core::clip_mask::clip_fill_rect(contours, clip);
            }
        }
        for point in contours.iter_mut().flatten() { *point = rotate(*point); }
        transform_paint_matrix(paint, matrix);
        Some(())
    };
    let mut prims = Vec::with_capacity(scene.prims.len());
    for mut prim in scene.prims.iter().cloned() {
        match &mut prim {
            Prim::Fill { contours, origin, paint, .. } => {
                rotate_ink(contours, paint, &clips)?;
                if contours.is_empty() { continue; }
                *origin = rotate(*origin);
            }
            Prim::Appearance { inks, origin, .. } => {
                for ink in inks.iter_mut() { rotate_ink(&mut ink.contours, &mut ink.paint, &clips)?; }
                inks.retain(|ink| !ink.contours.is_empty());
                if inks.is_empty() { continue; }
                *origin = rotate(*origin);
            }
            Prim::Stroke { object, points, closed, width, paint, opacity } => {
                if !matches!(paint, Paint::Solid { .. } | Paint::Gradient { .. }) {
                    return None;
                }
                let source = document.get_object(*object)?;
                let mut contours = dtool_core::scene::solid_stroke_contours(source, points, *closed, *width)?;
                rotate_ink(&mut contours, paint, &clips)?;
                if contours.is_empty() { continue; }
                prim = Prim::Fill {
                    object: *object, contours, origin: rotate(source.pos), paint: paint.clone(), opacity: *opacity,
                };
            }
            Prim::Image { object, quad, sample_in_quad, .. } => {
                let source = document.get_object(*object)?;
                if source.path_brush.is_some() {
                    return None;
                }
                match source.kind {
                    dtool_core::ObjectKind::Image => {
                        // Ordinary images use the axis-aligned footprint of the compositor.
                        let [left, top] = source.pos;
                        let [right, bottom] = [left + source.size[0], top + source.size[1]];
                        let [x0, y0, x1, y1] = [left.min(right), top.min(bottom), left.max(right), top.max(bottom)];
                        if !(x0 < x1 && y0 < y1) {
                            return None;
                        }
                        *quad = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
                        *sample_in_quad = true;
                    }
                    dtool_core::ObjectKind::PictureFrame => {
                        dtool_core::commands::picture_frame::frame_geometry(document, source)?;
                    }
                    _ => return None,
                }
                *quad = quad.map(rotate);
            }
            Prim::Text { object, .. } => {
                if dtool_core::scene::has_live_text_warp(document, *object) {
                    // Its visible ink is already in the scene as fills.
                    continue;
                }
                if dtool_core::text_lines::text_outlines(document, *object).is_none() {
                    return None;
                }
            }
            // The rasterizer still needs the clip in document coordinates for images and text.
            Prim::Clip { rect: Some(rect), .. } => clips.push(*rect),
            Prim::Clip { rect: None, .. } => { clips.pop()?; }
            Prim::Page { .. } | Prim::Mark { .. } | Prim::Handle { .. } => continue,
            _ => return None,
        }
        prims.push(prim);
    }
    Some(Scene { version: scene.version, prims, quads: Vec::new(), display_rotation: angle })
}

fn canvas_scene(document: &dtool_core::Document, view: &ViewState, detail: i32) -> Scene {
    let tolerance = dtool_core::outline::OUTLINE_TOLERANCE / 2.0_f64.powi(detail);
    let mut scene = dtool_render::scene::build_scene_with_tolerance(document, &view.canvas_flags, tolerance);
    dtool_core::clip_mask::apply(document, &view.canvas_flags, &mut scene);
    scene
}

fn composite_transform_patches(
    ctx: &egui::Context, session: &Session, view: &ViewState, scene: &Scene,
    bounds: [f32; 4], size: [usize; 2], antialias: bool,
    args: &dtool_core::command::TransformSelectionBoxArgs,
) -> Option<Vec<CompositeMovePatch>> {
    use dtool_core::layer_style::raster::{effect_bounds, has_group_effect, prim_object};
    use dtool_core::object::ObjectKind;

    let doc = &session.document;
    let selected = &session.selection.ids;
    if selected.is_empty() || selected.iter().any(|id| !doc.get_object(*id).is_some_and(|object| {
        !object.locked && matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_))
    })) || doc.objects().iter().any(|object| object.tone_blend.is_some() || has_group_effect(object)) {
        return None;
    }
    let targets = dtool_core::command::transform_target_ids(doc, selected);
    let mut draft = doc.clone();
    dtool_core::command::execute_transform_selection_box(&mut draft, selected, args).ok()?;
    let moved = canvas_scene(&draft, view, canvas_detail(view));
    // Include the old ink so the patch also restores what the object had
    // covered. Evaluate both scenes since a stationary clip may reveal more ink.
    let [x, y, w, h] = bounds;
    let [dx, dy] = [w / size[0] as f32, h / size[1] as f32];
    let mut regions = Vec::new();
    for (document, scene) in [(doc, scene), (&draft, &moved)] {
        let mut region = Rect::NOTHING;
        for &id in &targets {
            let prims = scene.prims.iter().filter(|prim| prim_object(prim) == Some(id));
            if let Some([x, y, w, h]) = effect_bounds(document, &[], id, prims) {
                region = region.union(Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)));
            }
        }
        if !region.is_finite() || !region.is_positive() {
            continue;
        }
        // Crop on the cached sample grid with two samples of edge coverage.
        let x0 = ((((region.min.x - x) / dx).floor() - 2.0) as usize).min(size[0]);
        let y0 = ((((region.min.y - y) / dy).floor() - 2.0) as usize).min(size[1]);
        let x1 = ((((region.max.x - x) / dx).ceil() + 2.0) as usize).min(size[0]);
        let y1 = ((((region.max.y - y) / dy).ceil() + 2.0) as usize).min(size[1]);
        if x0 < x1 && y0 < y1 {
            regions.push([x0, y0, x1, y1]);
        }
    }
    if let [a, b] = regions.as_slice()
        && pixel_rect_intersection(*a, *b).is_some()
    {
        regions = vec![[a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]];
    }
    // Keep disjoint regions separate; the span between them is unchanged.
    if regions.iter().map(|[x0, y0, x1, y1]| (x1 - x0) * (y1 - y0)).sum::<usize>() > 512 * 512 {
        return None;
    }
    Some(regions.into_iter().map(|pixels @ [x0, y0, x1, y1]| {
        let patch_size = [x1 - x0, y1 - y0];
        let patch_bounds = [x + x0 as f32 * dx, y + y0 as f32 * dy, patch_size[0] as f32 * dx, patch_size[1] as f32 * dy];
        let render = || render_scene_region(&draft, &view.canvas_flags, &[], &moved, patch_bounds, patch_size.map(|side| side as u32), 1.0);
        let rgba = if antialias { dtool_render::scene::with_fill_antialiasing(render) } else { render() };
        let texture = ctx.load_texture("canvas transform patch", egui::ColorImage::from_rgba_unmultiplied(patch_size, &rgba), egui::TextureOptions::NEAREST);
        CompositeMovePatch { pixels, texture }
    }).collect())
}

fn pixel_rect_intersection(a: [usize; 4], b: [usize; 4]) -> Option<[usize; 4]> {
    let [x0, y0, x1, y1] = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (x0 < x1 && y0 < y1).then_some([x0, y0, x1, y1])
}

fn composite_move_backdrop(size: [usize; 2], patches: &[CompositeMovePatch]) -> Vec<[usize; 4]> {
    let mut backdrop = vec![[0, 0, size[0], size[1]]];
    for patch in patches {
        let mut remainder = Vec::new();
        for region @ [left, top, right, bottom] in backdrop {
            if let Some([x0, y0, x1, y1]) = pixel_rect_intersection(region, patch.pixels) {
                remainder.extend([
                    [left, top, right, y0], [left, y1, right, bottom],
                    [left, y0, x0, y1], [x1, y0, right, y1],
                ].into_iter().filter(|[x0, y0, x1, y1]| x0 < x1 && y0 < y1));
            } else {
                remainder.push(region);
            }
        }
        backdrop = remainder;
    }
    backdrop
}

#[derive(Clone, Copy, PartialEq)]
struct FillRegion {
    edges: [f32; 4],
    size: [usize; 2],
    filter: egui::TextureOptions,
}

impl FillRegion {
    fn for_display(edges: [f32; 4], rect: Rect, view: &ViewState, pixels_per_point: f32, limit: f32) -> Option<Self> {
        let [left, top, right, bottom] = edges;
        let extent = [right - left, bottom - top];
        if !extent.iter().all(|length| length.is_finite() && *length > 0.0) {
            return None;
        }
        let density = view.zoom * pixels_per_point;
        // Make each coverage sample land on the physical pixel it represents.
        // Keep linear filtering for rotated views and regions larger than the GPU can hold.
        if view.rotation_deg == 0.0 && extent.iter().all(|length| length * density + 2.0 <= limit) {
            let origin = screen(rect, view, [0.0; 2]);
            let offset = [origin.x, origin.y].map(|value| (value * pixels_per_point).rem_euclid(1.0));
            let first = [(left * density + offset[0]).floor(), (top * density + offset[1]).floor()];
            let last = [(right * density + offset[0]).ceil(), (bottom * density + offset[1]).ceil()];
            let size = [0, 1].map(|axis| (last[axis] - first[axis]).max(1.0) as usize);
            let edges = [first[0] - offset[0], first[1] - offset[1], last[0] - offset[0], last[1] - offset[1]]
                .map(|value| value / density);
            if edges[0] < edges[2] && edges[1] < edges[3] {
                return Some(Self { edges, size, filter: egui::TextureOptions::NEAREST });
            }
        }
        let scale = density.min(limit / extent[0].max(extent[1]));
        let size = extent.map(|length| (length * scale).ceil().max(1.0) as usize);
        Some(Self { edges, size, filter: egui::TextureOptions::LINEAR })
    }
}

struct FillTexture {
    region: FillRegion,
    rotation_deg: Option<f32>,
    texture: egui::TextureHandle,
    used: bool,
}

pub fn wheel_view(view: &mut ViewState, scroll: [f32; 2], zoom_delta: f32, pointer: Option<[f32; 2]>) -> bool {
    wheel_view_with_limits(view, scroll, zoom_delta, pointer, ZoomLimits::default())
}

fn wheel_view_with_limits(
    view: &mut ViewState, scroll: [f32; 2], zoom_delta: f32, pointer: Option<[f32; 2]>, limits: ZoomLimits,
) -> bool {
    use crate::canvas::CanvasViewCommand;
    match CanvasViewCommand::from_wheel(view, scroll, zoom_delta, pointer, limits) {
        Some(CanvasViewCommand::ZoomSet { zoom, around: Some(point) }) => limits.zoom_about(view, zoom, point),
        Some(CanvasViewCommand::Pan { delta }) => dtool_core::command::view_pan_apply(view, delta),
        _ => return false,
    }
    true
}

#[derive(PartialEq)]
struct CompositionKey {
    revision: (dtool_core::document::DocInstance, u64, u64),
    story: dtool_core::StoryId,
    object: Option<ObjectId>,
    range: [usize; 2],
    text: String,
    symbol_sync: bool,
}

struct CompositionDraft {
    key: CompositionKey,
    session: Session,
    canvas: AppCanvas,
}

#[derive(Clone)]
struct GeometryPreview {
    revision: (dtool_core::document::DocInstance, u64, u64),
    selection: Vec<ObjectId>,
    command: dtool_core::Command,
    pass: u64,
}

impl GeometryPreview {
    fn revision(session: &Session) -> (dtool_core::document::DocInstance, u64, u64) {
        (session.document.instance, session.generation, session.document.version)
    }

    fn same_edit(&self, other: &Self) -> bool {
        self.revision == other.revision && self.selection == other.selection
            && self.command.name == other.command.name && self.command.params == other.command.params
    }
}

const GEOMETRY_PREVIEW_ID: &str = "dtool.canvas.numeric_geometry_preview";

pub(crate) fn stage_geometry_preview(ui: &Ui, session: &Session, command: dtool_core::Command) {
    let preview = GeometryPreview {
        revision: GeometryPreview::revision(session),
        selection: session.selection.ids.clone(),
        command,
        pass: ui.ctx().cumulative_pass_nr(),
    };
    let changed = ui.data_mut(|data| {
        let id = egui::Id::unique(GEOMETRY_PREVIEW_ID);
        let changed = data.get_temp::<GeometryPreview>(id).is_none_or(|old| !old.same_edit(&preview));
        data.insert_temp(id, preview);
        changed
    });
    if changed {
        ui.ctx().request_repaint();
    }
}

pub(crate) fn clear_geometry_preview(ui: &Ui) {
    let id = egui::Id::unique(GEOMETRY_PREVIEW_ID);
    let cleared = ui.data_mut(|data| {
        let present = data.get_temp::<GeometryPreview>(id).is_some();
        data.remove::<GeometryPreview>(id);
        present
    });
    if cleared { ui.ctx().request_repaint(); }
}

struct GeometryDraft {
    preview: GeometryPreview,
    session: Session,
    canvas: AppCanvas,
}

#[derive(Default)]
pub struct AppCanvas {
    scene: Scene,
    key: Option<(Option<dtool_core::workspace::ContextId>, u64, u64, dtool_core::CanvasFlags, i32)>,
    font_generation: u64,
    textures: HashMap<TextureKey, egui::TextureHandle>,
    fill_textures: HashMap<usize, FillTexture>,
    glyph_ink: HashMap<ObjectId, Option<GlyphInk>>,
    glyph_textures: HashMap<ObjectId, GlyphTexture>,
    decorations: HashMap<(ObjectId, u64), Vec<DecorationRect>>,
    effects: EffectCache,
    effect_prims: HashMap<ObjectId, Vec<usize>>,
    effect_regions: HashMap<ObjectId, (u64, Option<[f32; 4]>)>,
    uses_blending: bool,
    composite: Option<CompositeTexture>,
    rotated_fill_scene: Option<(f32, Option<Scene>)>,
    composite_revision: u64,
    settling_composite: bool,
    composite_job: Option<CompositeJob>,
    panning: bool,
    zoom_preview_until: f64,
    mesh_preview: Option<crate::canvas_input::warp_mesh::MeshPreview>,
    node_preview: Option<(Vec<(ObjectId, Vec<dtool_core::curve::Contour>)>, Box<AppCanvas>)>,
    composition_draft: Option<Box<CompositionDraft>>,
    geometry_draft: Option<Box<GeometryDraft>>,
    transform_draft: Option<(ObjectId, dtool_core::command::TransformSelectionBoxArgs, Box<AppCanvas>)>,
    pub(crate) blend_preview: Option<Vec<BlendPreview>>,
    pub(crate) tone_preview: Option<crate::dialogs::tone_blend::Preview>,
    pub(crate) layer_opacity_preview: Option<LayerOpacityPreview>,
    painted_layer_opacity: Option<LayerOpacityPreview>,
    pub(crate) stroke_width_preview: Option<crate::stroke_panel::StrokeWidthPreview>,
    painted_stroke_width: Option<crate::stroke_panel::StrokeWidthPreview>,
    pub(crate) toolbar_paint_preview: Vec<crate::fill_stroke::PaintUpdate>,
    painted_toolbar_paint: Vec<crate::fill_stroke::PaintUpdate>,
    painted_gradient: Option<crate::canvas_input::GradientPreview>,
    pub show_invisibles: bool,
    pub document_viewport: Option<[f32; 2]>,
    pub input: crate::canvas_input::CanvasInput,
    pixel_selection: crate::canvas_input::pixel_marquee::Outline,
    quick_mask: crate::canvas_input::pixel_marquee::QuickMaskOverlay,
}

pub fn invisible_glyph(c: char) -> Option<char> {
    match c {
        ' ' | '\u{2002}'..='\u{200A}' | '\u{3000}' => Some('\u{00B7}'),
        '\u{00A0}' | '\u{202F}' => Some('\u{00B0}'),
        '\t' => Some('\u{2192}'),
        '\u{2028}' => Some('\u{00AC}'),
        '\u{2029}' => Some('\u{00B6}'),
        '\u{00AD}' => Some('-'),
        '\u{200B}'..='\u{200D}' => Some('|'),
        _ => None,
    }
}

pub fn invisible_marks(text: &str, para_end: bool) -> Vec<(usize, char)> {
    let mut marks: Vec<(usize, char)> = text.chars().enumerate().filter_map(|(i, c)| invisible_glyph(c).map(|m| (i, m))).collect();
    if para_end {
        marks.push((text.chars().count(), '\u{00B6}'));
    }
    marks
}

fn colour32(colour: Colour, opacity: f32) -> Color32 {
    let a = (f32::from(colour.a) * opacity.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, a)
}

pub(crate) fn clip_frame_segment(a: [f32; 2], b: [f32; 2], origin: [f32; 2], extent: [f32; 2]) -> Option<[f32; 2]> {
    let (mut start, mut end) = (0.0_f32, 1.0_f32);
    for axis in 0..2 {
        let delta = b[axis] - a[axis];
        let left = origin[axis] - a[axis];
        let right = left + extent[axis];
        if delta == 0.0 {
            if left > 0.0 || right < 0.0 { return None; }
            continue;
        }
        let (first, last) = (left / delta, right / delta);
        start = start.max(first.min(last));
        end = end.min(first.max(last));
    }
    (start < end).then_some([start, end])
}

fn screen(rect: Rect, view: &ViewState, point: [f32; 2]) -> Pos2 {
    let v = dtool_core::doc_to_view(view, point);
    egui::pos2(rect.min.x + v[0], rect.min.y + v[1])
}

fn viewport_document_bounds(rect: Rect, view: &ViewState, density: f32) -> Rect {
    // Include the antialiasing fringe and the extra device-pixel bands used for table lines.
    let padded = rect.expand(2.0 / density);
    let corners = [padded.left_top(), padded.right_top(), padded.right_bottom(), padded.left_bottom()];
    Rect::from_points(&corners.map(|point| {
        let [x, y] = dtool_core::view::view_to_doc(view, (point - rect.min).into());
        egui::pos2(x, y)
    }))
}

fn paint_thin_table_rect(painter: &Painter, points: &[Pos2], fill: Color32, density: f32) -> bool {
    let area = Rect::from_points(points);
    let axis = usize::from(area.height() < area.width());
    let width = (area.max[axis] - area.min[axis]) * density;
    if !(0.0 < width && width <= 2.0)
        || !points.iter().all(|point| (point.x == area.min.x || point.x == area.max.x)
            && (point.y == area.min.y || point.y == area.max.y))
    {
        return false;
    }
    // Keep thin strokes equally visible at any subpixel position. A fractional
    // width beyond one device pixel is distributed over its two neighbours.
    let centre = (area.center()[axis] * density).floor();
    let mut mesh = egui::Mesh::default();
    let mut add_band = |offset: f32, coverage: f32| {
        let mut band = area;
        band.min[axis] = (centre + offset) / density;
        band.max[axis] = (centre + offset + 1.0) / density;
        mesh.add_colored_rect(band, fill.gamma_multiply(coverage));
    };
    add_band(0.0, width.min(1.0));
    if width > 1.0 {
        let fringe = (width - 1.0) * 0.5;
        add_band(-1.0, fringe);
        add_band(1.0, fringe);
    }
    painter.add(egui::Shape::mesh(mesh));
    true
}

pub fn image_mesh(document: &dtool_core::Document, object: &dtool_core::object::Object, quad: &[[f32; 2]; 4], rect: Rect, view: &ViewState, tint: Color32, texture: egui::TextureId) -> egui::Mesh {
    let mut mesh = egui::Mesh::with_texture(texture);
    let points: Vec<([f32; 2], [f32; 2])> = match dtool_core::commands::picture_frame::frame_geometry(document, object) {
        Some(geometry) => geometry.image_polygon(quad),
        None => quad.iter().copied().zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]).collect(),
    };
    if points.len() < 3 {
        return mesh;
    }
    for (point, uv) in &points {
        mesh.vertices.push(egui::epaint::Vertex { pos: screen(rect, view, *point), uv: egui::pos2(uv[0], uv[1]), color: tint });
    }
    for i in 1..points.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    mesh
}

fn paint_texture_quad(painter: &Painter, texture: egui::TextureId, corners: [Pos2; 4], tint: Color32) {
    let mut mesh = egui::Mesh::with_texture(texture);
    for (pos, [u, v]) in corners.into_iter().zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]) {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv: egui::pos2(u, v), color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

pub fn decoration_shape(rect: Rect, view: &ViewState, decoration: &DecorationRect, opacity: f32) -> Option<(Rect, Color32)> {
    let colour = representative_colour(&decoration.paint)?;
    let [x, y, w, h] = decoration.rect;
    if ![x, y, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h < 0.0 {
        return None;
    }
    let mut area = Rect::from_two_pos(screen(rect, view, [x, y]), screen(rect, view, [x + w, y + h]));
    if decoration.kind != DecorationKind::Background && area.height() < 1.0 {
        area = Rect::from_center_size(area.center(), egui::vec2(area.width(), 1.0));
    }
    area.is_positive().then_some((area, colour32(colour, opacity)))
}

fn effect_needs_backdrop(effect: &dtool_core::layer_style::LayerEffect) -> bool {
    use dtool_core::layer_style::EffectKind;

    effect.enabled && (!effect.blend.is_normal()
        || matches!(effect.kind, EffectKind::BevelEmboss | EffectKind::ThreeD))
}

impl AppCanvas {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate(&mut self) {
        self.key = None;
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    fn refresh(&mut self, session: &Session) {
        self.refresh_with_gradient(session, None);
    }

    fn refresh_with_gradient(&mut self, session: &Session, gradient: Option<&crate::canvas_input::GradientPreview>) {
        let flags = &session.view.canvas_flags;
        let detail = canvas_detail(&session.view);
        let key = (session.context, session.generation, session.document.version, flags.clone(), detail);
        let font_generation = dtool_core::text_lines::font_generation();
        let font_changed = !session.document.stories.is_empty() && self.font_generation != font_generation;
        self.font_generation = font_generation;
        let same_scene_key = !font_changed && self.key.as_ref() == Some(&key);
        let previous = self.painted_gradient.as_ref();
        if same_scene_key && previous == gradient {
            return;
        }
        // Zoom can change the curve tessellation without changing the artwork.
        // Retain the last composite so navigation can reuse it across that boundary.
        let only_detail_changed = !font_changed && previous.is_none() && gradient.is_none()
            && self.key.as_ref().is_some_and(|(context, generation, version, old_flags, _)| {
                *context == session.context && *generation == session.generation
                    && *version == session.document.version && old_flags == flags
            });
        let changing = [previous.map(|draft| draft.id), gradient.map(|draft| draft.id)];
        let rebuild_scene = !same_scene_key || previous.into_iter().chain(gradient).any(|draft| draft.changes_scene_paint());
        if rebuild_scene { self.rotated_fill_scene = None; }
        let previous_prims = rebuild_scene.then(|| self.rebuild_scene(session, detail));
        if same_scene_key {
            // Fill textures belong to scene slots. Preserve only unchanged ink
            // outside the draft targets, including when a paint changes the slot order.
            self.fill_textures.retain(|index, _| {
                self.scene.prims.get(*index).is_some_and(|prim| {
                    !changing.contains(&prim_object(prim))
                        && previous_prims.as_ref().is_none_or(|prims| prims.get(*index) == Some(prim))
                })
            });
        } else {
            self.fill_textures.clear();
        }
        if !only_detail_changed {
            self.composite = None;
            self.composite_revision = self.composite_revision.wrapping_add(1);
            self.settling_composite = false;
        }
        self.key = Some(key);
        self.painted_gradient = gradient.cloned();
        // Undo can change the scene generation without changing the document version.
        self.textures.clear();
        self.glyph_ink.clear();
        self.decorations.clear();
        self.effects = Default::default();
        self.effect_regions.clear();
    }

    fn rebuild_scene(&mut self, session: &Session, detail: i32) -> Vec<Prim> {
        let scene = canvas_scene(&session.document, &session.view, detail);
        let previous_prims = std::mem::replace(&mut self.scene, scene).prims;
        self.effect_prims.clear();
        for (index, prim) in self.scene.prims.iter().enumerate() {
            if let Some(id) = prim_object(prim) {
                self.effect_prims.entry(id).or_default().push(index);
            }
        }
        let flags = &session.view.canvas_flags;
        let doc = &session.document;
        let masked_ink = (dtool_core::clip_mask::has_roles(doc)
            || doc.objects.iter().any(|object| dtool_core::scene::oriented_vector_crop(object).is_some()))
            && self.scene.prims.iter().any(|prim| {
                match prim {
                    Prim::Image { object, .. } | Prim::Text { object, .. } => dtool_core::clip_mask::clip_region(doc, *object).is_some(),
                    _ => false,
                }
            });
        self.uses_blending = flags.mask_view.is_none()
            && !matches!(flags.view_mode, dtool_core::ViewMode::Outline | dtool_core::ViewMode::Wireframe)
            && (masked_ink || dtool_core::blend_composite::scene_needs_canvas_composite(doc, &self.scene)
                || doc.objects.iter().any(|object| {
                    dtool_core::layer_style::raster::has_group_effect(object)
                        || object.effects.iter().any(effect_needs_backdrop)
                }));
        previous_prims
    }

    pub fn effect_cache(&self) -> &EffectCache {
        &self.effects
    }

    pub fn staged_effects(session: &Session, object: ObjectId) -> Vec<StagedEffect> {
        dtool_core::commands::layer_style::staged_effects(session, object)
    }

    pub fn has_effects(session: &Session, object: ObjectId) -> bool {
        !live_effects(&session.document, object, &Self::staged_effects(session, object)).is_empty()
    }

    pub fn effect_pixels(&mut self, session: &Session, object: ObjectId, size: [u32; 2]) -> Option<([f32; 4], Vec<u8>)> {
        let live = Self::staged_effects(session, object);
        let bounds = object_effect_bounds(&session.document, &session.view.canvas_flags, &live, object)?;
        let size = [size[0].max(1), size[1].max(1)];
        let pixels = self.effects.object_region(&session.document, &session.view.canvas_flags, &live, object, bounds, size).to_vec();
        Some((bounds, pixels))
    }

    fn paint_effects(&mut self, ui: &Ui, painter: &Painter, rect: Rect, view: &ViewState, session: &Session, object: ObjectId, shift: egui::Vec2, prims: &[Prim]) -> bool {
        let live = Self::staged_effects(session, object);
        let doc = &session.document;
        let hash = effects_key(doc, object, &live);
        let Some(indices) = self.effect_prims.get(&object) else { return false };
        let bounds = if let Some((_, bounds)) = self.effect_regions.get(&object).filter(|(key, _)| *key == hash) {
            *bounds
        } else {
            let bounds = if doc.get_object(object).is_some_and(dtool_core::layer_style::raster::group_effect_target) {
                let members = doc.subtree_ids(object);
                effect_bounds(doc, &live, object, prims.iter().filter(|prim| prim_object(prim).is_some_and(|id| members.contains(&id))))
            } else {
                effect_bounds(doc, &live, object, indices.iter().map(|&index| &prims[index]))
            };
            self.effect_regions.insert(object, (hash, bounds));
            bounds
        };
        let Some(bounds) = bounds else { return false };
        let area = Rect::from_two_pos(screen(rect, view, [bounds[0], bounds[1]]), screen(rect, view, [bounds[0] + bounds[2], bounds[1] + bounds[3]]));
        let displayed = area.translate(shift);
        if !displayed.intersect(rect).is_positive() {
            return true;
        }
        let px = [(area.width().min(MAX_TEXTURE).ceil() as usize).max(1), (area.height().min(MAX_TEXTURE).ceil() as usize).max(1)];
        let slot = EFFECT_TEXTURE_START | (hash as usize >> 1);
        let key = (object, session.document.version, slot, px);
        // The painter has already drawn lower objects. Only upload this object's ink and effects.
        let pixels = (!self.textures.contains_key(&key)).then(|| {
            let scene = Scene {
                version: self.scene.version,
                prims: indices.iter().map(|&index| prims[index].clone()).collect(),
                quads: self.scene.quads.clone(),
                display_rotation: self.scene.display_rotation,
            };
            dtool_render::scene::with_fill_antialiasing(|| {
                render_scene_region(doc, &view.canvas_flags, &live, &scene, bounds, px.map(|side| side as u32), 1.0)
            })
        });
        if let Some(id) = self.texture(ui.ctx(), key, || pixels) {
            painter.image(id, displayed, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            true
        } else {
            false
        }
    }

    pub fn text_decorations(&mut self, session: &Session, object: ObjectId) -> Vec<DecorationRect> {
        let key = (object, session.document.version);
        self.decorations.entry(key).or_insert_with(|| object_decorations(&session.document, object)).clone()
    }

    fn paint_missing(painter: &Painter, rect: Rect, view: &ViewState, session: &Session, object: ObjectId, lines: &[dtool_core::text_lines::TextLine], fonts: bool, chars: bool) {
        let story = session.document.get_object(object).and_then(|source| source.story_id()).and_then(|id| session.document.stories.iter().find(|story| story.id == id));
        let (family, style) = story.and_then(|story| story.ranges.iter().find_map(|range| {
            let family = range.get("family").and_then(serde_json::Value::as_str)?;
            Some((family.to_string(), range.get("style").and_then(serde_json::Value::as_str).unwrap_or("").to_string()))
        })).unwrap_or_default();
        let library = dtool_tools::dtool_text::FontLibrary::shared();
        let colour = dtool_design::color("error");
        for line in lines {
            let split = dtool_tools::dtool_text::fonts::split_by_fallback(&library, &family, &style, &line.text);
            let (top, bottom) = (line.origin[1] - line.size * 0.8, line.origin[1] + line.size * 0.2);
            if fonts && split.missing_font && !family.is_empty() {
                let area = Rect::from_two_pos(screen(rect, view, [line.origin[0], top]), screen(rect, view, [line.origin[0] + line.width, bottom]));
                painter.rect_filled(area, 0.0, colour.gamma_multiply(0.25));
            }
            let count = line.text.chars().count();
            if chars && count > 0 {
                let advance = line.width / count as f32;
                for index in &split.missing_chars {
                    let x = line.origin[0] + advance * *index as f32;
                    let area = Rect::from_two_pos(screen(rect, view, [x, top]), screen(rect, view, [x + advance, bottom]));
                    painter.rect_filled(area, 0.0, colour.gamma_multiply(0.45));
                }
            }
        }
    }

    fn paint_decorations(painter: &Painter, rect: Rect, view: &ViewState, matrix: &[f32; 6], decorations: &[DecorationRect], background: bool, opacity: f32) {
        for decoration in decorations.iter().filter(|d| (d.kind == DecorationKind::Background) == background) {
            if *matrix == dtool_core::geometry::IDENTITY && view.rotation_deg == 0.0 {
                if let Some((area, colour)) = decoration_shape(rect, view, decoration, opacity) {
                    painter.rect_filled(area, 0.0, colour);
                }
            } else if let Some(colour) = representative_colour(&decoration.paint) {
                let mut bounds = decoration.rect;
                if !bounds.iter().all(|value| value.is_finite()) || bounds[2] <= 0.0 || bounds[3] < 0.0 {
                    continue;
                }
                if *matrix == dtool_core::geometry::IDENTITY && !background {
                    let thickness = bounds[3].max(view.zoom.max(1e-4).recip());
                    bounds[1] -= (thickness - bounds[3]) * 0.5;
                    bounds[3] = thickness;
                }
                let points = dtool_core::geometry::rect_quad(bounds, matrix)
                    .map(|point| screen(rect, view, point));
                painter.add(egui::Shape::convex_polygon(points.to_vec(), colour32(colour, opacity), Stroke::NONE));
            }
        }
    }

    fn texture(&mut self, ctx: &egui::Context, key: TextureKey, make: impl FnOnce() -> Option<Vec<u8>>) -> Option<egui::TextureId> {
        if let Some(handle) = self.textures.get(&key) {
            return Some(handle.id());
        }
        let rgba = make()?;
        let image = egui::ColorImage::from_rgba_unmultiplied(key.3, &rgba);
        let handle = ctx.load_texture(format!("app_canvas_{}_{}_{}", key.0 .0, key.1, key.2), image, egui::TextureOptions::LINEAR);
        let id = handle.id();
        self.textures.insert(key, handle);
        Some(id)
    }

    fn glyph_texture(
        &mut self, ctx: &egui::Context, document: &dtool_core::Document, key: TextureKey,
        lines: &[TextLine], bounds: [f32; 4], contours: GlyphContours,
    ) -> Option<egui::TextureId> {
        let (object, _, _, size) = key;
        if let Some(texture) = self.textures.get(&key) {
            return Some(texture.id());
        }
        let rasterize = || dtool_core::scene::text_glyph_image_from_outlines(
            document, object, lines, bounds, size, (*contours).clone(),
        );
        // Solid line fills depend only on the outlines and line geometry. Custom
        // runs and other paints can also depend on document resources.
        if lines.iter().any(|line| !matches!(line.paint, dtool_core::paint::Paint::Solid { .. }))
            || dtool_core::text_lines::text_ink(document, object).is_some()
        {
            return self.texture(ctx, key, rasterize);
        }
        if let Some(cached) = self.glyph_textures.get_mut(&object)
            && cached.size == size && cached.lines == lines && cached.contours == contours
        {
            cached.used = true;
            return Some(cached.texture.id());
        }
        let rgba = rasterize()?;
        let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
        let texture = ctx.load_texture(format!("canvas glyphs {}", object.0), image, egui::TextureOptions::LINEAR);
        let id = texture.id();
        self.glyph_textures.insert(object, GlyphTexture { contours, lines: lines.to_vec(), size, texture, used: true });
        Some(id)
    }

    pub fn frame(&mut self, ui: &mut Ui, response: &egui::Response, session: &mut Session, input: bool) {
        let rect = response.rect;
        self.panning = ui.input(|state| {
            let space = state.key_down(egui::Key::Space);
            let middle = state.pointer.middle_down();
            let wheel = response.hovered() && state.zoom_delta() == 1.0
                && state.smooth_scroll_delta != egui::Vec2::ZERO;
            let pointer_drag = response.dragged() || (response.hovered() && (middle || state.pointer.primary_down()));
            wheel || (!input && pointer_drag
                && crate::canvas_input::nav_mode(&session.active_tool, space, middle) == crate::canvas_input::NavMode::Pan)
        });
        if session.has_document {
            if self.panning {
                self.zoom_preview_until = 0.0;
            }
            let limits = session.zoom_limits();
            if self.wheel(ui, response, &mut session.view, limits) {
                self.preview_zoom(ui);
            }
        }
        let node_input = input && session.has_document
            && crate::canvas_input::tool_kind(&session.active_tool) == crate::canvas_input::ToolKind::Node
            && !self.input.warp_mesh.visible(session);
        if node_input {
            if self.mesh_preview.take().is_some() {
                self.invalidate();
            }
            self.refresh(session);
            self.input.update(ui, response, session, &self.scene, false);
        }
        let move_input = input && session.has_document && matches!(session.active_tool.as_str(), "tool.move" | "tool.select.object");
        if move_input {
            self.refresh(session);
            self.input.update(ui, response, session, &self.scene, false);
        }
        let frame_tool = input && crate::canvas_input::tool_kind(&session.active_tool) == crate::canvas_input::ToolKind::FrameText;
        let mesh_preview = input.then(|| self.input.warp_mesh.preview(session).cloned()).flatten();
        if self.mesh_preview != mesh_preview {
            self.invalidate();
            self.mesh_preview = mesh_preview.clone();
        }
        // Paint a draft on the regular canvas route. Restore the stored mesh
        // before handling input, saving, or serving the next MCP request.
        let original_mesh = mesh_preview.as_ref().and_then(|preview| preview.install(session));
        let vector_draft = move_input.then(|| self.direct_vector_transform(session)).flatten();
        let numeric_preview = self.paint_numeric_geometry_preview(ui, rect, session);
        if numeric_preview || self.paint_composition_preview(ui, rect, session, input, !frame_tool) {
            self.transform_draft = None;
            self.node_preview = None;
        } else if input && let Some(id) = self.input.live_text_resize(session).or(vector_draft)
            && let Some(args) = self.input.preview.as_ref()
        {
            let mut draft = session.document.clone();
            if dtool_core::command::execute_transform_selection_box(&mut draft, &[id], args).is_ok() {
                let args = args.clone();
                // Keep the input scene on the original transform basis for the drag.
                self.refresh(session);
                let (target, previous, canvas) = self.transform_draft.get_or_insert_with(|| {
                    (id, args.clone(), Box::new(Self::new()))
                });
                if *target != id || *previous != args {
                    *target = id;
                    *previous = args;
                    canvas.invalidate();
                }
                let stored = std::mem::replace(&mut session.document, draft);
                if vector_draft.is_some() && canvas.key.is_none() {
                    canvas.refresh(session);
                    if stored.get_object(id).is_some_and(|object| !dtool_core::clip_mask::is_mask(&stored, object)) {
                        let affected = stored.subtree_ids(id);
                        // Reuse effects on stationary layers that do not use
                        // the transformed subtree as their text path.
                        canvas.textures.extend(self.textures.iter().filter_map(|(&(object, _, slot, size), texture)| {
                            (slot >= EFFECT_TEXTURE_START && !affected.contains(&object)
                                && !stored.get_object(object).and_then(|layer| layer.path_text_data())
                                    .is_some_and(|path| affected.contains(&path.path)))
                                .then(|| ((object, session.document.version, slot, size), texture.clone()))
                        }));
                    }
                }
                canvas.show_invisibles = self.show_invisibles;
                canvas.paint_layers(ui, rect, session, false);
                session.document = stored;
            } else {
                self.paint_layers(ui, rect, session, !frame_tool);
            }
        } else {
            self.transform_draft = None;
            if !node_input || !self.paint_node_preview(ui, rect, session) {
                self.node_preview = None;
                self.paint_layers_with_style_preview(ui, rect, session, !frame_tool);
            }
        }
        if let (Some(preview), Some(original)) = (mesh_preview, original_mesh) {
            preview.restore(session, original);
        }
        if !session.has_document {
            return;
        }
        if input {
            if node_input || move_input {
                if !crate::rulers::guide_drag_active(ui.ctx()) {
                    self.input.paint_preview(ui, rect, session, &self.scene);
                }
            } else {
                self.input.handle(ui, response, session, &self.scene);
                if self.input.gradient_preview(session) != self.painted_gradient.as_ref() {
                    ui.ctx().request_repaint();
                }
            }
            if !numeric_preview {
                self.paint_selection(&ui.painter_at(rect), rect, &session.view, session, true);
                crate::canvas::frame_text_input::paint_text_selection(ui, session, response, &session.view);
                crate::canvas::frame_text_input::paint_caret(ui, session, response, &session.view);
            }
        } else {
            if self.input.busy() {
                self.input.cancel();
            }
            if !numeric_preview {
                self.paint_selection(&ui.painter_at(rect), rect, &session.view, session, false);
            }
        }
    }

    fn paint_numeric_geometry_preview(&mut self, ui: &Ui, rect: Rect, session: &Session) -> bool {
        let id = egui::Id::unique(GEOMETRY_PREVIEW_ID);
        if !ui.input(|input| input.pointer.primary_down() && !input.key_pressed(egui::Key::Escape)) {
            ui.data_mut(|data| data.remove::<GeometryPreview>(id));
            self.geometry_draft = None;
            return false;
        }
        let preview = ui.data(|data| data.get_temp::<GeometryPreview>(id));
        let Some(preview) = preview.filter(|preview| {
            preview.revision == GeometryPreview::revision(session)
                && preview.selection == session.selection.ids
                && ui.ctx().cumulative_pass_nr().saturating_sub(preview.pass) <= 1
        }) else {
            self.geometry_draft = None;
            return false;
        };

        if self.geometry_draft.as_ref().is_none_or(|draft| !draft.preview.same_edit(&preview)) {
            let mut document = session.document.clone();
            document.instance = Default::default();
            let mut draft = Session::new(document);
            draft.selection = session.selection.clone();
            draft.symbol_sync = session.symbol_sync;
            draft.active_tool.clone_from(&session.active_tool);
            draft.view = session.view.clone();
            if draft.run(&preview.command).is_err() {
                self.geometry_draft = None;
                return false;
            }
            self.geometry_draft = Some(Box::new(GeometryDraft { preview, session: draft, canvas: Self::new() }));
        }

        let Some(draft) = self.geometry_draft.as_mut() else { return false };
        draft.session.view = session.view.clone();
        draft.canvas.show_invisibles = self.show_invisibles;
        draft.canvas.paint_layers(ui, rect, &draft.session, true);
        draft.canvas.paint_selection(&ui.painter_at(rect), rect, &draft.session.view, &draft.session, false);
        true
    }

    fn paint_composition_preview(
        &mut self, ui: &Ui, rect: Rect, session: &Session, input: bool, overlay: bool,
    ) -> bool {
        use crate::canvas::frame_text_input::{TextRangeMark, composition_text, paint_text_range};
        use crate::canvas_input::{ToolKind, tool_kind};

        let active = input && session.has_document
            && matches!(tool_kind(&session.active_tool), ToolKind::ArtText | ToolKind::FrameText | ToolKind::Table);
        if !active {
            self.composition_draft = None;
            return false;
        }
        let (Some(edit), Some(text)) = (session.text_edit.as_ref(), composition_text(session)) else {
            self.composition_draft = None;
            return false;
        };
        let start = edit.anchor.min(edit.caret);
        let key = CompositionKey {
            revision: (session.document.instance, session.generation, session.document.version),
            story: edit.story,
            object: edit.object,
            range: [start, edit.anchor.max(edit.caret)],
            text,
            symbol_sync: session.symbol_sync,
        };

        if self.composition_draft.as_ref().is_none_or(|current| current.key != key) {
            let mut document = session.document.clone();
            // Keep temporary glyph and layout caches separate from the saved document.
            document.instance = Default::default();
            let mut draft = Session::new(document);
            draft.text_edit = Some(dtool_core::command::TextEditState {
                story: edit.story,
                object: edit.object,
                caret: edit.caret,
                anchor: edit.anchor,
                before: None,
            });
            draft.symbol_sync = session.symbol_sync;
            if draft.run_named("text.edit.type", serde_json::json!({"text": &key.text})).is_err() {
                self.composition_draft = None;
                return false;
            }
            if let Some(edit) = draft.text_edit.as_mut() { edit.anchor = start; }
            draft.text_composing = Some(key.text.clone());
            if let Some(current) = self.composition_draft.as_mut() {
                current.key = key;
                current.session = draft;
                current.canvas.invalidate();
            } else {
                self.composition_draft = Some(Box::new(CompositionDraft { key, session: draft, canvas: Self::new() }));
            }
        }

        // Input and hit testing still use the committed document.
        self.refresh(session);
        let Some(draft) = self.composition_draft.as_mut() else { return false; };
        draft.session.view = session.view.clone();
        draft.session.active_tool.clone_from(&session.active_tool);
        draft.canvas.show_invisibles = self.show_invisibles;
        draft.canvas.paint_layers(ui, rect, &draft.session, overlay);
        let mark = TextRangeMark::Composition(egui::Stroke::new(1.0, dtool_design::color("accent")));
        paint_text_range(ui, &draft.session, rect, &draft.session.view, mark);
        true
    }

    fn direct_vector_transform(&self, session: &Session) -> Option<ObjectId> {
        use dtool_core::object::ObjectKind;
        use dtool_tools::move_tool::GestureKind;

        let &[id] = session.selection.ids.as_slice() else { return None };
        let args = self.input.preview.as_ref()?;
        let needs_preview = args.scale != [1.0, 1.0] || args.rotation_deg != 0.0 || args.shear_deg != 0.0;
        if !needs_preview || !matches!(self.input.move_tool.gesture, Some(GestureKind::Scale(_) | GestureKind::Rotate | GestureKind::Shear(_)))
            || self.uses_blending || self.blend_preview.is_some() || self.tone_preview.is_some()
            || self.layer_opacity_preview.is_some() || self.stroke_width_preview.is_some()
            || session.view.rotation_deg != 0.0
            || session.view.canvas_flags.mask_view.is_some()
            || dtool_render::scene::pixel_step(&session.document, &session.view.canvas_flags).is_some()
            || !dtool_core::commands::layer_style::snapshot_staged(session).is_empty()
        {
            return None;
        }
        session.document.get_object(id).filter(|object| {
            !object.locked && matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_))
        }).map(|object| object.id)
    }

    fn paint_node_preview(&mut self, ui: &Ui, rect: Rect, session: &mut Session) -> bool {
        use dtool_core::commands::curve_edit::edited_curve;

        let Some(overrides) = self.input.nodes.live_curve_overrides() else { return false };
        let mut originals = Vec::new();
        for (id, contours) in &overrides {
            if let Some(source) = session.document.find_object_mut(id.0)
                && !source.locked && source.curve().is_some()
                && let Ok(Some(edited)) = edited_curve(source, contours, true)
            {
                originals.push((*id, std::mem::replace(source, edited)));
            }
        }
        if originals.is_empty() { return false; }
        let (last, canvas) = self.node_preview.get_or_insert_with(|| (Vec::new(), Box::new(Self::new())));
        if *last != overrides {
            *last = overrides;
            canvas.invalidate();
        }
        // Use normal scene order, masks and paint while keeping the committed document for input and undo.
        canvas.show_invisibles = self.show_invisibles;
        canvas.paint_layers(ui, rect, session, true);
        for (id, original) in originals {
            if let Some(source) = session.document.find_object_mut(id.0) {
                *source = original;
            }
        }
        true
    }

    fn paint_layers_with_style_preview(&mut self, ui: &Ui, rect: Rect, session: &mut Session, overlay: bool) {
        let preview = self.layer_opacity_preview.clone();
        let stroke_preview = self.stroke_width_preview.clone();
        let gradient = self.input.gradient_preview(session).cloned();
        let mut toolbar_paint = self.toolbar_paint_preview.clone();
        if self.painted_layer_opacity != preview || self.painted_stroke_width != stroke_preview
            || self.painted_toolbar_paint != toolbar_paint
        {
            self.painted_toolbar_paint = toolbar_paint.clone();
            self.invalidate();
            self.painted_layer_opacity = preview.clone();
            self.painted_stroke_width = stroke_preview.clone();
        }
        let originals = if let Some(preview) = preview {
            preview.ids.into_iter().filter_map(|id| {
                let object = session.document.find_object_mut(id.0)?;
                Some((id, std::mem::replace(&mut object.opacity, preview.value)))
            }).collect()
        } else {
            Vec::new()
        };
        let stroke_widths = if let Some(preview) = stroke_preview {
            preview.ids.into_iter().filter_map(|id| {
                let object = session.document.find_object_mut(id.0)?;
                let stroke = match preview.index {
                    0 => &mut object.stroke,
                    index => object.additional_strokes.get_mut(index - 1)?,
                };
                Some((id, preview.index, std::mem::replace(&mut stroke.width, preview.width)))
            }).collect()
        } else {
            Vec::new()
        };
        let restore_gradient = gradient.clone().and_then(|mut preview| {
            let object = session.document.find_object_mut(preview.id.0)?;
            preview.swap_with_object(object).then_some(preview)
        });
        // The drag paints through the regular scene; only its final command is recorded in history.
        for paint in &mut toolbar_paint {
            paint.swap(&mut session.document);
        }
        let active_gradient = restore_gradient.as_ref().and(gradient.as_ref());
        self.paint_layers_with_gradient(ui, rect, session, overlay, active_gradient);
        for paint in toolbar_paint.iter_mut().rev() {
            paint.swap(&mut session.document);
        }
        if let Some(mut preview) = restore_gradient
            && let Some(object) = session.document.find_object_mut(preview.id.0)
        {
            preview.swap_with_object(object);
        }
        for (id, index, width) in stroke_widths {
            if let Some(object) = session.document.find_object_mut(id.0) {
                let stroke = match index {
                    0 => &mut object.stroke,
                    index => &mut object.additional_strokes[index - 1],
                };
                stroke.width = width;
            }
        }
        for (id, opacity) in originals {
            if let Some(object) = session.document.find_object_mut(id.0) {
                object.opacity = opacity;
            }
        }
    }

    pub fn finish_text_input(&mut self, session: &mut Session) -> bool {
        crate::canvas::frame_text_input::finish_text_input(session)
    }

    pub fn text_entry_active(&self, session: &Session) -> bool {
        crate::canvas::frame_text_input::text_entry_active(session)
    }

    pub(crate) fn preview_zoom(&mut self, ui: &Ui) {
        self.zoom_preview_until = ui.input(|state| state.time) + ZOOM_IDLE_SECONDS;
        ui.ctx().request_repaint();
    }

    fn wheel(&self, ui: &Ui, response: &egui::Response, view: &mut ViewState, limits: ZoomLimits) -> bool {
        if !response.hovered() {
            return false;
        }
        let before = view.zoom;
        let rect = response.rect;
        let (scroll, zoom, pos) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
        let pointer = pos.map(|p| [p.x - rect.min.x, p.y - rect.min.y]);
        if wheel_view_with_limits(view, [scroll.x, scroll.y], zoom, pointer, limits) {
            ui.ctx().request_repaint();
        }
        view.zoom != before
    }

    pub fn paint(&mut self, ui: &Ui, rect: Rect, session: &Session) {
        self.document_viewport = Some([rect.width(), rect.height()]);
        self.paint_layers(ui, rect, session, true);
        self.paint_selection(&ui.painter_at(rect), rect, &session.view, session, false);
    }

    fn paint_composite(
        &mut self, ui: &Ui, painter: &Painter, rect: Rect, view: &ViewState, session: &Session,
        staged: Vec<StagedEffect>,
    ) -> bool {
        let show_previews = view.canvas_flags.mask_view.is_none()
            && !matches!(view.canvas_flags.view_mode, dtool_core::ViewMode::Outline | dtool_core::ViewMode::Wireframe);
        let preview = self.blend_preview.as_ref().filter(|_| show_previews);
        let tone_preview = self.tone_preview.filter(|_| show_previews);
        let brush_draft = (show_previews && !session.quick_mask.active)
            .then(|| self.input.pixel_brush_draft(session)).flatten();
        let brush_revision = brush_draft.as_ref().map(|draft| draft.revision);
        let device_scale = view.zoom * ui.ctx().pixels_per_point();
        // Use hard document pixels only when the display can resolve them.
        let pixel_step = show_previews.then(|| {
            dtool_render::scene::pixel_step(&session.document, &view.canvas_flags)
        }).flatten().filter(|step| step * device_scale >= 1.0);
        let staged_composite_effect = show_previews && staged.iter().any(|item| {
            effect_needs_backdrop(&item.effect)
                || ((dtool_core::layer_style::raster::group_effect_enabled(&item.effect)
                    || dtool_core::layer_style::raster::group_blur_params(&item.effect).is_some())
                    && session.document.get_object(item.object).is_some_and(dtool_core::layer_style::raster::group_effect_target))
        });
        if (!self.uses_blending && !staged_composite_effect && preview.is_none() && tone_preview.is_none()
            && brush_draft.is_none() && pixel_step.is_none()) || !rect.is_positive() {
            return false;
        }
        let align_fill = show_previews && view.rotation_deg != 0.0
            && view.canvas_flags.view_mode == dtool_core::ViewMode::Normal
            && staged.is_empty() && preview.is_none() && tone_preview.is_none() && brush_draft.is_none()
            && self.painted_gradient.is_none() && self.painted_layer_opacity.is_none()
            && self.painted_stroke_width.is_none()
            && self.painted_toolbar_paint.is_empty();
        if align_fill && self.rotated_fill_scene.as_ref().is_none_or(|(angle, _)| *angle != view.rotation_deg) {
            let document = &session.document;
            let rotate = || rotated_fill_scene(document, &self.scene, view.rotation_deg);
            let aligned = if document.stories.is_empty() {
                rotate()
            } else {
                dtool_text::frame::with_doc_layout_reuse(document, &dtool_text::FontLibrary::shared(), rotate)
            };
            self.rotated_fill_scene = Some((view.rotation_deg, aligned));
        }
        let aligned = self.rotated_fill_scene.as_ref().filter(|(angle, _)| align_fill && *angle == view.rotation_deg)
            .and_then(|(_, scene)| scene.as_ref());
        let rotation_deg = aligned.map(|_| view.rotation_deg);
        let unrotated = aligned.map(|_| ViewState { rotation_deg: 0.0, ..view.clone() });
        let sample_view = unrotated.as_ref().unwrap_or(view);
        let visible = dtool_core::view::visible_doc_quad(sample_view, [rect.width(), rect.height()]);
        let Some(edges) = contours_bounds(&[visible.to_vec()]) else {
            return false;
        };
        let texture_limit = ui.input(|input| input.max_texture_side) as f32;
        let (mut edges, mut size, mut filter) = if let Some(step) = pixel_step {
            let texture_limit = texture_limit.min(MAX_PIXEL_PREVIEW_TEXTURE);
            let longest = (edges[2] - edges[0]).max(edges[3] - edges[1]);
            // Align the preview to document pixels so panning does not move the pixel grid.
            let step = step * (longest / (step * (texture_limit - 2.0).max(1.0))).ceil().max(1.0);
            let aligned = [(edges[0] / step).floor(), (edges[1] / step).floor(),
                (edges[2] / step).ceil(), (edges[3] / step).ceil()].map(|value| value * step);
            let size = [aligned[2] - aligned[0], aligned[3] - aligned[1]]
                .map(|length| (length / step).round().max(1.0) as usize);
            (aligned, size, egui::TextureOptions::NEAREST)
        } else {
            let Some(region) = FillRegion::for_display(edges, rect, sample_view, ui.ctx().pixels_per_point(), texture_limit.min(MAX_VECTOR_TEXTURE)) else {
                return false;
            };
            (region.edges, region.size, region.filter)
        };
        let remaining_zoom_preview = self.zoom_preview_until - ui.input(|state| state.time);
        let use_navigation_preview = (self.panning || remaining_zoom_preview > 0.0)
            && pixel_step.is_none() && view.canvas_flags.view_mode == dtool_core::ViewMode::Normal
            && staged.is_empty() && preview.is_none() && tone_preview.is_none() && brush_revision.is_none()
            && !self.input.busy()
            && self.composite.as_ref().is_some_and(|cached| {
                cached.navigation_preview || cached.render_time > INTERACTIVE_RENDER_BUDGET
            });
        let pan_scale = (self.panning && !use_navigation_preview
            && view.rotation_deg == 0.0 && filter == egui::TextureOptions::NEAREST)
            .then_some((device_scale, pixel_step));
        let mut reused_bounds = None;
        let mut navigation_preview = false;
        if use_navigation_preview {
            self.settling_composite = true;
            if remaining_zoom_preview > 0.0 {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(remaining_zoom_preview));
            }
            let buffered = self.composite.as_ref().filter(|cached| {
                let [x, y, width, height] = cached.bounds;
                cached.rotation_deg == rotation_deg && cached.pan_scale.is_none() && cached.staged.is_empty()
                    && cached.blend_preview.is_none() && cached.tone_preview.is_none()
                    && cached.brush_revision.is_none() && cached.transform_preview.is_none()
                    && edges[0] >= x && edges[1] >= y
                    && edges[2] <= x + width && edges[3] <= y + height
                    && device_scale <= cached.device_scale * 1.20
            });
            if let Some(cached) = buffered {
                let [x, y, width, height] = cached.bounds;
                edges = [x, y, x + width, y + height];
                size = cached.size;
                filter = cached.filter;
                navigation_preview = cached.navigation_preview;
                reused_bounds = Some(cached.bounds);
            } else {
                // Leave room for the next pan or outward zoom. Restore the
                // display sample grid when the gesture ends.
                let margin_ratio = if self.panning { 0.25 } else { 0.12 };
                for axis in 0..2 {
                    let margin = (edges[axis + 2] - edges[axis]) * margin_ratio;
                    edges[axis] -= margin;
                    edges[axis + 2] += margin;
                }
                let factor = (NAVIGATION_PREVIEW_PIXELS / (size[0] * size[1]) as f32).sqrt().min(1.0);
                size = size.map(|side| (side as f32 * factor).ceil().max(1.0) as usize);
                filter = egui::TextureOptions::LINEAR;
                navigation_preview = true;
            }
        }
        if let Some(scale) = pan_scale {
            let buffered = self.composite.as_ref().filter(|cached| {
                let [x, y, width, height] = cached.bounds;
                cached.rotation_deg == rotation_deg && cached.pan_scale == Some(scale) && edges[0] >= x && edges[1] >= y
                    && edges[2] <= x + width && edges[3] <= y + height
            });
            if let Some(cached) = buffered {
                let [x, y, width, height] = cached.bounds;
                edges = [x, y, x + width, y + height];
                size = cached.size;
                reused_bounds = Some(cached.bounds);
            } else {
                let limit = texture_limit.min(if pixel_step.is_some() { MAX_PIXEL_PREVIEW_TEXTURE } else { MAX_VECTOR_TEXTURE }) as usize;
                for axis in 0..2 {
                    let margin = PAN_MARGIN.min(limit.saturating_sub(size[axis]) / 2);
                    let sample = (edges[axis + 2] - edges[axis]) / size[axis] as f32;
                    edges[axis] -= sample * margin as f32;
                    edges[axis + 2] += sample * margin as f32;
                    size[axis] += margin * 2;
                }
            }
        }
        let [left, top, right, bottom] = edges;
        let bounds = reused_bounds.unwrap_or([left, top, right - left, bottom - top]);
        if bounds[2] <= 0.0 || bounds[3] <= 0.0 {
            return false;
        }
        let detail = canvas_detail(view);
        let reusing_navigation_texture = use_navigation_preview && reused_bounds.is_some();
        let completed = self.composite_job.as_ref().and_then(|job| match job.result.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            result => Some((job.request, result.ok())),
        });
        if completed.is_some() {
            self.composite_job = None;
        }
        if self.composite.as_ref().is_none_or(|cached| {
            (!reusing_navigation_texture && cached.detail != detail)
                || cached.rotation_deg != rotation_deg || cached.bounds != bounds || cached.size != size || cached.filter != filter || cached.pan_scale != pan_scale
                || cached.navigation_preview != navigation_preview || cached.staged != staged
                || cached.blend_preview.as_ref() != preview || cached.tone_preview != tone_preview
                || cached.brush_revision != brush_revision
        }) {
            let request = CompositeRequest { rotation_deg, revision: self.composite_revision, detail, bounds, size };
            let fallback = self.composite.as_ref().filter(|cached| {
                self.settling_composite && !use_navigation_preview && !self.panning && pixel_step.is_none()
                    && view.canvas_flags.view_mode == dtool_core::ViewMode::Normal
                    && staged.is_empty() && preview.is_none() && tone_preview.is_none() && brush_revision.is_none()
                    && !self.input.busy() && self.input.preview.is_none() && self.painted_gradient.is_none()
                    && self.painted_layer_opacity.is_none() && self.painted_stroke_width.is_none()
                    && self.painted_toolbar_paint.is_empty()
                    && cached.rotation_deg == rotation_deg && cached.pan_scale.is_none() && cached.staged.is_empty()
                    && cached.blend_preview.is_none() && cached.tone_preview.is_none()
                    && cached.brush_revision.is_none() && cached.transform_preview.is_none()
                    && left >= cached.bounds[0] && top >= cached.bounds[1]
                    && right <= cached.bounds[0] + cached.bounds[2] && bottom <= cached.bounds[1] + cached.bounds[3]
            });
            let mut finished_image = None;
            if let Some(cached) = fallback {
                let worker_failed = matches!(&completed, Some((stamp, None)) if *stamp == request);
                finished_image = completed.and_then(|(stamp, image)| (stamp == request).then_some(image).flatten());
                if finished_image.is_none() && !worker_failed {
                    if self.composite_job.is_none() {
                        self.composite_job = CompositeJob::start(
                            ui.ctx(), request, session.document.clone(), view.canvas_flags.clone(), aligned.unwrap_or(&self.scene).clone(),
                        ).ok();
                    }
                    if self.composite_job.is_some() {
                        // Also poll if the worker exits before requesting a repaint.
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                        let [x, y, w, h] = cached.bounds;
                        Self::paint_composite_texture(painter, rect, sample_view, &cached.texture, [x, y, x + w, y + h]);
                        return true;
                    }
                }
            }
            let (image, render_time) = finished_image.unwrap_or_else(|| {
                let draft_document = brush_draft.as_ref().map_or(&session.document, |draft| draft.document.as_ref());
                let preview_document = (preview.is_some() || tone_preview.is_some()).then(|| {
                    let mut document = draft_document.clone();
                    for draft in preview.into_iter().flatten() {
                        if let Some(object) = document.find_object_mut(draft.id.0) {
                            object.blend_mode = draft.mode;
                            object.blend_ranges = draft.ranges.clone();
                        }
                    }
                    if let Some(draft) = tone_preview && let Some(object) = document.find_object_mut(draft.id.0) {
                        object.tone_blend = Some(draft.tone);
                    }
                    document
                });
                let document = preview_document.as_ref().unwrap_or(draft_document);
                let draft_scene = brush_draft.as_ref().map(|_| canvas_scene(document, &view, canvas_detail(&view)));
                let started = std::time::Instant::now();
                let render = || render_scene_region(
                    document, &view.canvas_flags, &staged, draft_scene.as_ref().or(aligned).unwrap_or(&self.scene), bounds,
                    size.map(|length| length as u32), 1.0,
                );
                let pixels = if pixel_step.is_none() { dtool_render::scene::with_fill_antialiasing(render) } else { render() };
                (egui::ColorImage::from_rgba_unmultiplied(size, &pixels), started.elapsed())
            });
            let texture = ui.ctx().load_texture("canvas composite", image, filter);
            self.composite = Some(CompositeTexture {
                rotation_deg, bounds, size, filter, staged, blend_preview: preview.cloned(), tone_preview, brush_revision,
                pan_scale, navigation_preview, device_scale, detail, render_time, texture, transform_preview: None,
            });
        }
        if !use_navigation_preview && !self.panning {
            self.settling_composite = false;
        }
        let Some(composite) = &mut self.composite else { return false };
        use dtool_tools::move_tool::GestureKind;
        let transform = self.input.preview.as_ref().filter(|args| {
            matches!(self.input.move_tool.gesture, Some(GestureKind::Move | GestureKind::Rotate | GestureKind::Scale(_) | GestureKind::Shear(_)))
                && !self.input.move_tool.duplicate_on_drag
                && (args.translate != [0.0, 0.0] || args.rotation_deg != 0.0 || args.scale != [1.0, 1.0] || args.shear_deg != 0.0)
                && view.rotation_deg == 0.0 && filter == egui::TextureOptions::NEAREST
                && composite.staged.is_empty() && preview.is_none() && tone_preview.is_none()
                && brush_revision.is_none()
        });
        if let Some(args) = transform {
            if composite.transform_preview.as_ref().is_none_or(|preview| preview.args != *args) {
                let patches = composite_transform_patches(ui.ctx(), session, view, &self.scene, bounds, size, pixel_step.is_none(), args)
                    .unwrap_or_default();
                let backdrop = composite_move_backdrop(size, &patches);
                composite.transform_preview = Some(CompositeTransformPreview { args: args.clone(), patches, backdrop });
            }
        } else {
            composite.transform_preview = None;
        }
        if let Some(preview) = composite.transform_preview.as_ref().filter(|preview| !preview.patches.is_empty()) {
            let screen_at = |x, y| screen(rect, view, [
                left + bounds[2] * (x as f32 / size[0] as f32),
                top + bounds[3] * (y as f32 / size[1] as f32),
            ]);
            let uv_at = |x, y| egui::pos2(x as f32 / size[0] as f32, y as f32 / size[1] as f32);
            let mut mesh = egui::Mesh::with_texture(composite.texture.id());
            for &[x0, y0, x1, y1] in &preview.backdrop {
                mesh.add_rect_with_uv(Rect::from_min_max(screen_at(x0, y0), screen_at(x1, y1)),
                    Rect::from_min_max(uv_at(x0, y0), uv_at(x1, y1)), Color32::WHITE);
            }
            painter.add(egui::Shape::mesh(mesh));
            for patch in &preview.patches {
                let [x0, y0, x1, y1] = patch.pixels;
                painter.image(patch.texture.id(), Rect::from_min_max(screen_at(x0, y0), screen_at(x1, y1)),
                    Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            return true;
        }

        Self::paint_composite_texture(painter, rect, sample_view, &composite.texture, edges);
        true
    }

    fn paint_composite_texture(
        painter: &Painter, rect: Rect, view: &ViewState, texture: &egui::TextureHandle,
        [left, top, right, bottom]: [f32; 4],
    ) {
        let mut mesh = egui::Mesh::with_texture(texture.id());
        for (point, uv) in [[left, top], [right, top], [right, bottom], [left, bottom]].into_iter()
            .zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
        {
            mesh.vertices.push(egui::epaint::Vertex { pos: screen(rect, view, point), uv: egui::pos2(uv[0], uv[1]), color: Color32::WHITE });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }

    fn paint_layers(&mut self, ui: &Ui, rect: Rect, session: &Session, overlay: bool) {
        self.paint_layers_with_gradient(ui, rect, session, overlay, None);
    }

    fn paint_layers_with_gradient(
        &mut self, ui: &Ui, rect: Rect, session: &Session, overlay: bool,
        gradient: Option<&crate::canvas_input::GradientPreview>,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, dtool_design::theme::surface(ui.ctx(), "pasteboard"));
        if !session.has_document {
            self.fill_textures.clear();
            self.glyph_textures.clear();
            return;
        }
        if session.document.stories.is_empty() {
            self.paint_document_layers(ui, rect, session, overlay, painter, gradient);
        } else {
            let fonts = dtool_text::FontLibrary::shared();
            dtool_text::frame::with_doc_layout_reuse(&session.document, &fonts, || {
                self.paint_document_layers(ui, rect, session, overlay, painter, gradient);
            });
        }
        // Keep one texture per text object painted in the current viewport.
        self.glyph_textures.retain(|_, texture| std::mem::take(&mut texture.used));
        self.fill_textures.retain(|_, texture| std::mem::take(&mut texture.used));
    }

    fn paint_document_layers(&mut self, ui: &Ui, rect: Rect, session: &Session, overlay: bool, painter: Painter, gradient: Option<&crate::canvas_input::GradientPreview>) {
        dtool_tools::frame_text::install_flow_ranges();
        dtool_tools::frame_text::install_canvas_text();
        self.refresh_with_gradient(session, gradient);
        let view = session.view.clone();
        for prim in &self.scene.prims {
            if let Prim::Page { index, rect: page } = prim
                && !dtool_core::artboard::edit::is_backing_page(&session.document, *index)
            {
                Self::paint_page(ui, &painter, rect, &view, *page);
            }
        }
        if !view.canvas_flags.preview_mode {
            let stroke = Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.45));
            for [x, y, width, height] in dtool_core::data_merge::layout::vacant_page_rects(&session.document) {
                let corners = [[x, y], [x + width, y], [x + width, y + height], [x, y + height]];
                let points = corners.map(|point| screen(rect, &view, point));
                painter.add(egui::Shape::closed_line(points.to_vec(), stroke));
            }
        }
        let artboard_border = Self::page_border(ui, &view);
        let linked_layout_pages = session.document.layout_artboard_origins.is_some().then(|| {
            session.document.pages.iter().filter_map(|page| page.artboard_id)
                .collect::<std::collections::HashSet<_>>()
        });
        for artboard in dtool_core::view::ViewTarget::artboards(&session.document) {
            let quad = dtool_core::geometry::object_world_quad(artboard);
            let points = quad.map(|point| screen(rect, &view, point));
            painter.add(egui::Shape::convex_polygon(points.to_vec(), Color32::WHITE, artboard_border));
            let is_layout_page = linked_layout_pages.as_ref().is_some_and(|ids| ids.contains(&artboard.id));
            if !view.canvas_flags.preview_mode && !is_layout_page {
                let galley = artboard_caption(ui, &session.document, artboard, points[0].distance(points[1]));
                painter.galley(points[0] - egui::vec2(0.0, 3.0 + galley.size().y), galley, ui.visuals().text_color());
            }
        }
        let staged = dtool_core::commands::layer_style::snapshot_staged(session);
        let staged_ids: std::collections::HashSet<_> = staged.iter().map(|item| item.object).collect();
        let composited = self.paint_composite(ui, &painter, rect, &view, session, staged);
        // Several primitives can refer to the same object. Resolve the owner
        // without scanning the entire document for each primitive.
        let mut objects = HashMap::new();
        if !composited {
            objects.reserve(session.document.objects().len());
            for object in session.document.objects() {
                objects.entry(object.id).or_insert(object);
            }
        }
        let moved = (!composited).then(|| self.translated_ink(session, &view)).flatten();
        let density = ui.ctx().pixels_per_point();
        let viewport = viewport_document_bounds(rect, &view, density);
        let moved_viewport = moved.as_ref().map(|(_, view)| viewport_document_bounds(rect, view, density));
        let prims = std::mem::take(&mut self.scene.prims);
        let mut clip_stack: Vec<Rect> = Vec::new();
        let mut effected: HashMap<ObjectId, bool> = HashMap::new();
        let mut graded = std::collections::HashSet::new();
        for (index, prim) in prims.iter().enumerate() {
            if let Prim::Clip { rect: area, .. } = prim {
                match area {
                    Some(r) => {
                        let next = Rect::from_two_pos(screen(rect, &view, [r[0], r[1]]), screen(rect, &view, [r[0] + r[2], r[1] + r[3]])).intersect(rect);
                        let next = clip_stack.last().copied().map(|cur| cur.intersect(next)).unwrap_or(next);
                        clip_stack.push(next);
                    }
                    None => {
                        clip_stack.pop();
                    }
                }
                continue;
            }
            let clipped = clip_stack.last().copied().map(|c| painter.with_clip_rect(c)).unwrap_or_else(|| painter.clone());
            if composited {
                match prim {
                    Prim::Text { object, lines, opacity } => self.paint_text_annotations(&clipped, rect, &view, session, *object, lines, *opacity),
                    Prim::Handle { .. } => self.paint_prim(ui, &clipped, rect, &view, viewport, session, index, prim, None),
                    _ => {}
                }
                continue;
            }
            let id = prim_object(prim);
            let source = id.and_then(|id| objects.get(&id).copied());
            let (object_view, object_viewport) = moved.as_ref().zip(moved_viewport).filter(|((ids, _), _)| {
                id.is_some_and(|id| ids.contains(&id))
            }).map_or((&view, viewport), |((_, moved_view), bounds)| (moved_view, bounds));
            if view.canvas_flags.mask_view.is_none()
                && let Some(object) = id.filter(|id| {
                    staged_ids.contains(id)
                        || source.is_some_and(|object| object.effects.iter().any(|effect| effect.enabled))
                }).filter(|_| {
                matches!(prim, Prim::Fill { .. } | Prim::Stroke { .. } | Prim::Text { .. })
                    || dtool_core::layer_style::raster::is_path_brush_image(&session.document, prim)
            }) {
                let painted = *effected.entry(object).or_insert_with(|| {
                    let shift = egui::vec2(object_view.scroll[0] - view.scroll[0], object_view.scroll[1] - view.scroll[1]);
                    Self::has_effects(session, object) && self.paint_effects(ui, &clipped, rect, &view, session, object, shift, &prims)
                });
                if painted {
                    if let Prim::Text { lines, opacity, .. } = prim {
                        self.paint_text_annotations(&clipped, rect, object_view, session, object, lines, *opacity);
                    }
                    continue;
                }
            }
            if let Some(object) = source
                && !matches!(prim, Prim::Handle { .. })
                && view.canvas_flags.mask_view.is_none()
                && !matches!(view.canvas_flags.view_mode, dtool_core::ViewMode::Outline | dtool_core::ViewMode::Wireframe)
                && (!matches!(object.transparency, dtool_core::paint::Transparency::None)
                    || matches!(object.stroke.paint, dtool_core::paint::Paint::Gradient { .. }))
            {
                if graded.insert(object.id) {
                    self.paint_graded_object(ui, &clipped, rect, object_view, session, object, index);
                }
                if let Prim::Text { lines, opacity, .. } = prim {
                    self.paint_text_annotations(&clipped, rect, object_view, session, object.id, lines, *opacity);
                }
                continue;
            }
            self.paint_prim(ui, &clipped, rect, object_view, object_viewport, session, index, prim, source);
        }
        self.scene.prims = prims;
        if !view.canvas_flags.preview_mode && view.canvas_flags.mask_view.is_none() {
            self.paint_empty_picture_frames(ui, &painter, rect, session);
        }
        if !view.canvas_flags.preview_mode && overlay {
            crate::canvas::frame_text_input::paint_overlay(ui, session, rect, &view);
        }
    }

    fn translated_ink(&self, session: &Session, view: &ViewState) -> Option<(Vec<ObjectId>, ViewState)> {
        use dtool_core::object::{ClipRole, ObjectKind};
        use dtool_tools::move_tool::GestureKind;

        let tool = &self.input.move_tool;
        let args = self.input.preview.as_ref()?;
        if tool.gesture != Some(GestureKind::Move) || tool.duplicate_on_drag
            || args.translate == [0.0, 0.0] || view.canvas_flags.mask_view.is_some()
        {
            return None;
        }
        let doc = &session.document;
        let targets = dtool_core::command::transform_target_ids(doc, &session.selection.ids);
        // These objects can be repositioned using the existing primitives and
        // textures. Other kinds may have layout or annotations outside the scene.
        if targets.iter().any(|id| !doc.get_object(*id).is_some_and(|object| {
            matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Group | ObjectKind::Image)
                && !object.locked
        })) {
            return None;
        }
        // A clip owned outside the selection must be evaluated at the new
        // position. Keep the outline preview for that case.
        if dtool_core::clip_mask::has_roles(doc) {
            let stationary_masks: std::collections::HashSet<_> = doc.objects().iter()
                .filter(|object| object.clip_role == ClipRole::Mask && !targets.contains(&object.id))
                .filter_map(|object| object.parent_id)
                .collect();
            for &id in &targets {
                let mut current = doc.get_object(id);
                for _ in 0..64 {
                    let Some(object) = current else { break };
                    if stationary_masks.contains(&object.id)
                        || (object.clip_role != ClipRole::None
                            && object.parent_id.is_some_and(|parent| !targets.contains(&parent)))
                    {
                        return None;
                    }
                    current = object.parent_id.and_then(|parent| doc.get_object(parent));
                }
            }
        }
        let mut moved_view = view.clone();
        moved_view.scroll = dtool_core::view::doc_to_view(view, args.translate);
        Some((targets, moved_view))
    }

    fn paint_empty_picture_frames(&self, ui: &Ui, painter: &Painter, rect: Rect, session: &Session) {
        use dtool_core::commands::picture_frame::{FrameShape, frame_image_size, frame_outline};
        use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};

        let document = &session.document;
        let view = &session.view;
        let stroke = Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.72));
        for frame in &document.picture_frames {
            let Some(object) = document.get_object(frame.object) else { continue };
            if !dtool_core::scene::is_drawn_in_view(document, view, object.id)
                || frame_image_size(document, object, frame).is_some()
            {
                continue;
            }
            let [width, height] = object.size.map(f32::abs);
            if !(width > 0.0 && height > 0.0) {
                continue;
            }
            let left = object.pos[0] + object.size[0].min(0.0);
            let top = object.pos[1] + object.size[1].min(0.0);
            let matrix = object_matrix(object);
            let board = dtool_core::artboard::owner(document, object.id);
            let clip = board.and_then(|board| {
                let inverse = invert_matrix(&object_matrix(board))?;
                Some((board.pos, board.size.map(f32::abs), inverse))
            });
            if board.is_some() && clip.is_none() {
                continue;
            }
            let line = |a, b| {
                let mut points = [a, b].map(|point| apply_matrix(&matrix, point));
                if let Some((origin, extent, inverse)) = clip {
                    let [a, b] = points.map(|point| apply_matrix(&inverse, point));
                    let Some([start, end]) = clip_frame_segment(a, b, origin, extent) else { return };
                    let [a, b] = points;
                    points = [start, end].map(|t| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                }
                painter.line_segment(points.map(|point| screen(rect, view, point)), stroke);
            };
            let outline = frame_outline(frame.shape, [left, top, width, height]);
            for (&a, &b) in outline.iter().zip(outline.iter().cycle().skip(1)) {
                line(a, b);
            }
            let inset = if frame.shape == FrameShape::Ellipse {
                (1.0 - std::f32::consts::FRAC_1_SQRT_2) * 0.5
            } else {
                0.0
            };
            let x0 = left + width * inset;
            let x1 = left + width * (1.0 - inset);
            let y0 = top + height * inset;
            let y1 = top + height * (1.0 - inset);
            line([x0, y0], [x1, y1]);
            line([x0, y1], [x1, y0]);
        }
    }

    fn paint_graded_object(
        &mut self, ui: &Ui, painter: &Painter, rect: Rect, view: &ViewState,
        session: &Session, object: &dtool_core::Object, index: usize,
    ) {
        let [left, top, width, height] = dtool_core::geometry::object_world_bounds(object);
        let padding = dtool_core::commands::appearance::visible_strokes(object)
            .map(|stroke| {
                let join = match stroke.join {
                    dtool_core::paint::Join::Miter { limit } => limit.max(1.0),
                    _ => 1.0,
                };
                let pressure = stroke.pressure.iter().map(|point| point[1]).fold(1.0, f32::max);
                stroke.width * join * pressure
            })
            .fold(1.0, f32::max);
        let bounds = [left - padding, top - padding, width + 2.0 * padding, height + 2.0 * padding];
        let visible = dtool_core::view::visible_doc_quad(view, [rect.width(), rect.height()]);
        let Some([vx0, vy0, vx1, vy1]) = contours_bounds(&[visible.to_vec()]) else { return };
        let [x0, y0, w, h] = bounds;
        let (x1, y1) = (x0 + w, y0 + h);
        if w <= 0.0 || h <= 0.0 || x0 >= vx1 || y0 >= vy1 || x1 <= vx0 || y1 <= vy0 {
            return;
        }
        let density = view.zoom * ui.ctx().pixels_per_point();
        let size = [w, h].map(|length| (length * density).ceil().clamp(1.0, MAX_TEXTURE) as usize);
        let Some(texture) = self.texture(ui.ctx(), (object.id, session.document.version, index, size), || {
            Some(dtool_render::scene::with_fill_antialiasing(|| dtool_core::layer_style::raster::render_object_alone(
                &session.document, &view.canvas_flags, object.id, bounds, size[0] as u32, size[1] as u32,
            )))
        }) else { return };
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|point| screen(rect, view, point));
        paint_texture_quad(painter, texture, corners, Color32::WHITE);
    }

    fn page_border(ui: &Ui, view: &ViewState) -> Stroke {
        if !view.canvas_flags.preview_mode || view.canvas_flags.show_page_edges_in_preview {
            Stroke::new(1.0, ui.visuals().weak_text_color())
        } else {
            Stroke::NONE
        }
    }

    fn paint_page(ui: &Ui, painter: &Painter, rect: Rect, view: &ViewState, page: [f32; 4]) {
        let [left, top, width, height] = page;
        let [right, bottom] = [left + width, top + height];
        let border = Self::page_border(ui, view);
        if view.rotation_deg == 0.0 {
            let area = Rect::from_two_pos(screen(rect, view, [left, top]), screen(rect, view, [right, bottom]));
            painter.rect_filled(area, 0.0, Color32::WHITE);
            painter.rect_stroke(area, 0.0, border, egui::StrokeKind::Outside);
        } else {
            let corners = [[left, top], [right, top], [right, bottom], [left, bottom]];
            let corners = corners.map(|point| screen(rect, view, point)).to_vec();
            painter.add(egui::Shape::convex_polygon(corners, Color32::WHITE, border));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_prim(
        &mut self, ui: &Ui, painter: &Painter, rect: Rect, view: &ViewState, viewport: Rect,
        session: &Session, index: usize, prim: &Prim, source: Option<&dtool_core::Object>,
    ) {
        let zoom = view.zoom.max(1e-4);
        let version = session.document.version;
        match prim {
            Prim::Page { .. } => {}
            Prim::Fill { contours, origin, paint, opacity, .. } => {
                let Some([left, top, right, bottom]) = contours_bounds(contours) else { return };
                if !viewport.intersects(Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom))) {
                    return;
                }
                let transparency = view.canvas_flags.mask_view.is_none()
                    .then_some(source)
                    .flatten()
                    .map(|source| source.transparency.clone())
                    .unwrap_or_default();
                let graded = !matches!(transparency, dtool_core::paint::Transparency::None);
                if let Some(colour) = simple_solid(paint, contours).filter(|_| !graded) {
                    let points: Vec<Pos2> = contours[0].iter().map(|p| screen(rect, view, *p)).collect();
                    let fill = colour32(colour, *opacity);
                    if points.len() == 4
                        && source.is_some_and(|object| matches!(object.kind, dtool_core::ObjectKind::Table(_)))
                        && paint_thin_table_rect(painter, &points, fill, ui.ctx().pixels_per_point())
                    {
                        return;
                    }
                    painter.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
                    return;
                }
                let clip = painter.clip_rect().intersect(rect);
                if !clip.is_positive() { return; }
                let rotation_deg = (view.rotation_deg != 0.0
                    && view.canvas_flags.view_mode == dtool_core::ViewMode::Normal
                    && dtool_core::scene::paint_supported(paint)).then_some(view.rotation_deg);
                let rotation = rotation_deg.map(|angle| egui::emath::Rot2::from_angle(angle.to_radians()));
                // Build coverage in view-aligned document axes, then sample the
                // original paint using the inverse rotation.
                let unrotated = rotation.map(|_| ViewState { rotation_deg: 0.0, ..view.clone() });
                let sample_view = unrotated.as_ref().unwrap_or(view);
                let rotate_point = |rotation: egui::emath::Rot2, [x, y]: [f32; 2]| {
                    let point = rotation * egui::vec2(x, y);
                    [point.x, point.y]
                };
                let [left, top, right, bottom] = if let Some(rotation) = rotation {
                    let corners = [[left, top], [right, top], [right, bottom], [left, bottom]]
                        .map(|point| rotate_point(rotation, point));
                    let Some(bounds) = contours_bounds(&[corners.to_vec()]) else { return };
                    bounds
                } else {
                    [left, top, right, bottom]
                };
                let [vw, vh] = [clip.width(), clip.height()];
                let mut clipped_view = sample_view.clone();
                clipped_view.scroll[0] -= clip.min.x - rect.min.x;
                clipped_view.scroll[1] -= clip.min.y - rect.min.y;
                let visible = dtool_core::view::visible_doc_quad(&clipped_view, [vw, vh]);
                let Some([vx0, vy0, vx1, vy1]) = contours_bounds(&[visible.to_vec()]) else { return };
                let edges = [left.max(vx0), top.max(vy0), right.min(vx1), bottom.min(vy1)];
                let limit = (ui.input(|input| input.max_texture_side) as f32).min(MAX_VECTOR_TEXTURE);
                let Some(region) = FillRegion::for_display(edges, rect, sample_view, ui.ctx().pixels_per_point(), limit) else { return };
                let [x0, y0, x1, y1] = region.edges;
                let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|point| screen(rect, sample_view, point));
                let fallback = representative_colour(paint);
                let id = if let Some(cached) = self.fill_textures.get_mut(&index)
                    .filter(|cached| cached.region == region && cached.rotation_deg == rotation_deg)
                {
                    cached.used = true;
                    Some(cached.texture.id())
                } else {
                    let pixels = dtool_render::scene::with_fill_antialiasing(|| {
                        if let Some(rotation) = rotation {
                            let rotated: Vec<Vec<_>> = contours.iter()
                                .map(|contour| contour.iter().map(|point| rotate_point(rotation, *point)).collect())
                                .collect();
                            let inverse = rotation.inverse();
                            return dtool_render::scene::rasterize_fill_mapped(
                                paint, &transparency, 1.0, &rotated, region.edges, region.size, |point| {
                                    let [x, y] = rotate_point(inverse, point);
                                    [f64::from(x - origin[0]), f64::from(y - origin[1])]
                                },
                            );
                        }
                        match paint {
                            dtool_core::paint::Paint::Bitmap(bitmap) => dtool_core::scene::rasterize_bitmap(&session.document, bitmap, contours, *origin, region.edges, region.size),
                            _ if graded => dtool_render::scene::rasterize_fill_with_transparency(paint, &transparency, 1.0, contours, *origin, region.edges, region.size),
                            _ => rasterize_fill(paint, contours, *origin, region.edges, region.size),
                        }
                    });
                    pixels.map(|rgba| {
                        let image = egui::ColorImage::from_rgba_unmultiplied(region.size, &rgba);
                        let texture = ui.ctx().load_texture("canvas fill", image, region.filter);
                        let id = texture.id();
                        self.fill_textures.insert(index, FillTexture { region, rotation_deg, texture, used: true });
                        id
                    })
                };
                match (id, fallback) {
                    (Some(id), _) => {
                        paint_texture_quad(painter, id, corners, Color32::WHITE.gamma_multiply(*opacity));
                    }
                    (None, Some(colour)) => {
                        painter.add(egui::Shape::convex_polygon(corners.to_vec(), colour32(colour, *opacity), Stroke::NONE));
                    }
                    _ => {}
                }
            }
            Prim::Stroke { points, closed, width, paint, opacity, .. } => {
                let colour = representative_colour(paint).unwrap_or(Colour::new(0, 0, 0, 255));
                let px = if *width <= 0.0 { 1.0 } else { (width * zoom).max(0.5) };
                let pts: Vec<Pos2> = points.iter().map(|p| screen(rect, view, *p)).collect();
                let colour = if matches!(view.canvas_flags.view_mode, dtool_core::ViewMode::Outline | dtool_core::ViewMode::Wireframe) {
                    crate::canvas_input::SELECTION_BLUE.gamma_multiply(0.70)
                } else {
                    colour32(colour, *opacity)
                };
                let stroke = Stroke::new(px, colour);
                if *closed {
                    painter.add(egui::Shape::closed_line(pts, stroke));
                } else {
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
            Prim::Image { object, quad, opacity, .. } => {
                let Some(source) = source else { return };
                if !viewport.intersects(Rect::from_points(&quad.map(|[x, y]| egui::pos2(x, y)))) {
                    return;
                }
                let doc = &session.document;
                let texture = if source.path_brush.is_some() {
                    let [top_left, _, bottom_right, _] = *quad;
                    let extent = [bottom_right[0] - top_left[0], bottom_right[1] - top_left[1]];
                    let size = dtool_core::path_brush::pixel_size(extent, zoom * ui.ctx().pixels_per_point());
                    self.texture(ui.ctx(), (*object, version, index, size), || {
                        Some(dtool_core::path_brush::Ink::new(doc, source)?.render_clipped(doc, *object, size))
                    })
                } else if let Some(size) = dtool_core::raster_source::dimensions(doc, source) {
                    self.texture(ui.ctx(), (*object, version, index, size), || dtool_core::raster_source::rgba(doc, source).map(|r| r.1))
                } else {
                    None
                };
                let Some(id) = texture else { return };
                let mesh = image_mesh(&session.document, source, quad, rect, view, Color32::WHITE.gamma_multiply(*opacity), id);
                if !mesh.is_empty() {
                    painter.add(egui::Shape::mesh(mesh));
                }
            }
            Prim::Text { object, lines, opacity } => {
                if dtool_core::scene::has_live_text_warp(&session.document, *object) {
                    return;
                }
                let decorations = self.text_decorations(session, *object);
                let matrix = source.map_or(dtool_core::geometry::IDENTITY, dtool_core::geometry::text_matrix);
                Self::paint_decorations(painter, rect, view, &matrix, &decorations, true, *opacity);
                let ink = self.glyph_ink.entry(*object).or_insert_with(|| {
                    let contours = dtool_core::text_lines::text_outlines(&session.document, *object)?;
                    Some((contours_bounds(&contours)?, std::sync::Arc::new(contours)))
                }).clone();
                let glyphs = ink.and_then(|(bounds, contours)| {
                    let [x0, y0, x1, y1] = bounds;
                    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|point| screen(rect, view, point));
                    if !Rect::from_points(&corners).intersect(rect).is_positive() {
                        return Some(());
                    }
                    let px = [x1 - x0, y1 - y0].map(|length| (length * zoom).ceil().clamp(1.0, MAX_TEXTURE) as usize);
                    let id = self.glyph_texture(ui.ctx(), &session.document, (*object, version, index, px), lines, bounds, contours)?;
                    paint_texture_quad(painter, id, corners, Color32::WHITE.gamma_multiply(*opacity));
                    Some(())
                });
                if glyphs.is_none() {
                    let angle = view.rotation_deg.to_radians();
                    let rotation = egui::emath::Rot2::from_angle(angle);
                    for line in lines {
                        let size = line.size * zoom;
                        if size < MIN_TEXT_PX {
                            continue;
                        }
                        let colour = colour32(representative_colour(&line.paint).unwrap_or(Colour::new(0, 0, 0, 255)), *opacity);
                        let at = screen(rect, view, line.origin);
                        let galley = painter.layout_no_wrap(line.text.clone(), egui::FontId::proportional(size), colour);
                        let top = at + rotation * egui::vec2(0.0, size * 0.2 - galley.size().y);
                        painter.add(egui::epaint::TextShape::new(top, galley, colour).with_angle(angle));
                    }
                }
                Self::paint_decorations(painter, rect, view, &matrix, &decorations, false, *opacity);
                self.paint_text_annotations(painter, rect, view, session, *object, lines, *opacity);
            }
            Prim::Handle { object, at } => {
                if view.canvas_flags.preview_mode || !session.selection.ids.contains(object) { return; }
                let side = dtool_design::size("icon.size.sm") * 0.5;
                let stroke = Stroke::new(dtool_design::stroke_width("md"), dtool_design::color("accent"));
                painter.rect_stroke(Rect::from_center_size(screen(rect, view, *at), egui::vec2(side, side)), 0.0, stroke, egui::StrokeKind::Inside);
            }
            Prim::Label { at, angle_deg, text, size, paint, opacity, .. } => {
                let px = size * zoom;
                if px < MIN_TEXT_PX {
                    return;
                }
                let colour = colour32(representative_colour(paint).unwrap_or(Colour::new(0, 0, 0, 255)), *opacity);
                let galley = painter.layout_no_wrap(text.clone(), egui::FontId::proportional(px), colour);
                let angle = angle_deg.to_radians();
                let (sin, cos) = angle.sin_cos();
                let half = galley.size().x * 0.5;
                let up = galley.size().y;
                let offset = egui::vec2(-half * cos + up * sin, -half * sin - up * cos);
                painter.add(egui::epaint::TextShape::new(screen(rect, view, *at) + offset, galley, colour).with_angle(angle));
            }
            Prim::Mark { .. } | Prim::Clip { .. } | Prim::Appearance { .. } => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_text_annotations(&self, painter: &Painter, rect: Rect, view: &ViewState, session: &Session, object: ObjectId, lines: &[dtool_core::text_lines::TextLine], opacity: f32) {
        if view.canvas_flags.preview_mode {
            return;
        }
        let zoom = view.zoom.max(1e-4);
        if self.show_invisibles || view.canvas_flags.show_invisibles {
            let mark_colour = dtool_design::color("invisibles.mark");
            let doc = &session.document;
            let source = doc.get_object(object);
            let library = dtool_text::FontLibrary::shared();
            let matrix = source.map_or(IDENTITY, dtool_core::geometry::text_matrix);
            let map = |point| screen(rect, view, dtool_core::geometry::apply_matrix(&matrix, point));
            let [a, b, c, d, _, _] = matrix;
            let angle = view.rotation_deg.to_radians() + b.atan2(a);
            let rotation = egui::emath::Rot2::from_angle(angle);
            let scale = zoom * c.hypot(d);
            let paint_mark = |glyph: String, point, doc_size| {
                let size = doc_size * scale;
                if size < MIN_TEXT_PX {
                    return 0.0;
                }
                let galley = painter.layout_no_wrap(glyph, egui::FontId::proportional(size), mark_colour);
                let advance = galley.size().x / (zoom * a.hypot(b)).max(1e-4);
                let top = map(point) + rotation * egui::vec2(0.0, size * 0.2 - galley.size().y);
                painter.add(egui::epaint::TextShape::new(top, galley, mark_colour).with_angle(angle));
                advance
            };
            let mut paragraph_ends = HashMap::new();
            if let Some(path_text) = source.filter(|source| source.is_path_text()) {
                for (glyph, ends) in dtool_text::canvas_text::path_invisible_mark_positions(&library, doc, path_text, invisible_glyph) {
                    let [top, bottom] = ends.map(|point| screen(rect, view, point));
                    let normal = bottom - top;
                    let size = normal.length();
                    if size < MIN_TEXT_PX { continue; }
                    let angle = normal.angle() - std::f32::consts::FRAC_PI_2;
                    let galley = painter.layout_no_wrap(glyph.to_string(), egui::FontId::proportional(size), mark_colour);
                    let origin = bottom - normal * (galley.size().y / size);
                    painter.add(egui::epaint::TextShape::new(origin, galley, mark_colour).with_angle(angle));
                }
            } else if let Some(marks) = source.and_then(|source| {
                dtool_text::canvas_text::invisible_mark_positions(&library, doc, source, invisible_glyph)
            }) {
                for (offset, glyph, at, size) in marks {
                    let advance = paint_mark(glyph.to_string(), at, size);
                    if glyph == '\u{00B6}' {
                        paragraph_ends.insert(offset, [at[0] + advance, at[1]]);
                    }
                }
            } else {
                for line in lines {
                    let size = line.size * zoom;
                    if size < MIN_TEXT_PX { continue; }
                    let colour = colour32(representative_colour(&line.paint).unwrap_or(Colour::new(0, 0, 0, 255)), opacity);
                    let galley = painter.layout_no_wrap(line.text.clone(), egui::FontId::proportional(size), colour);
                    let count = line.text.chars().count();
                    for (index, mark) in invisible_marks(&line.text, line.para_end) {
                        let offset = if index >= count { line.width } else { galley.pos_from_cursor(egui::text::CCursor::new(index)).min.x / zoom };
                        paint_mark(mark.to_string(), [line.origin[0] + offset, line.origin[1]], line.size);
                    }
                }
            }
            for mark in dtool_core::text_lines::text_break_marks(doc, object) {
                let glyph = match mark.kind {
                    dtool_core::document::BreakKind::Column => "\u{2193}",
                    dtool_core::document::BreakKind::Frame => "\u{21E3}",
                    dtool_core::document::BreakKind::Page => "\u{21A7}",
                    dtool_core::document::BreakKind::OddPage => "\u{21A7}1",
                    dtool_core::document::BreakKind::EvenPage => "\u{21A7}2",
                    dtool_core::document::BreakKind::Line => continue,
                };
                let at = paragraph_ends.get(&mark.pos).copied().unwrap_or(mark.at);
                paint_mark(glyph.to_owned(), at, mark.size);
            }
        }
        let (fonts, chars) = session.view.highlight();
        if fonts || chars {
            Self::paint_missing(painter, rect, view, session, object, lines, fonts, chars);
        }
    }

    fn paint_selection(&mut self, painter: &Painter, rect: Rect, view: &ViewState, session: &Session, interactive: bool) {
        use crate::canvas_input::{SELECTION_BLUE, ToolKind, tool_kind};
        let kind = tool_kind(&session.active_tool);
        let draft = session.quick_mask.active.then(|| self.input.pixel_brush_draft(session)).flatten();
        self.quick_mask.paint(painter, rect, session, draft.as_ref());
        self.pixel_selection.paint(painter, rect, session);
        let editing_nodes = kind == ToolKind::Node
            && !self.input.nodes.primitive
            && self.input.nodes.target.is_some_and(|id| session.selection.ids.contains(&id));
        if view.canvas_flags.preview_mode
            || matches!(kind, ToolKind::VectorCrop | ToolKind::StrokeWidth | ToolKind::Slice | ToolKind::PixelBrush(_) | ToolKind::PixelFlood { .. } | ToolKind::PixelMarquee(_))
            || (session.quick_mask.active && matches!(kind, ToolKind::Gradient(_) | ToolKind::Transparency))
            || self.input.dragging_artboard()
            || self.input.warp_mesh.visible(session)
            || editing_nodes
        {
            return;
        }
        if let Some(edit) = &session.text_edit {
            if session.selection.ids.len() == 1 && edit.object == session.selection.ids.first().copied() {
                return;
            }
        }
        let accent = SELECTION_BLUE;
        let paint_origin = |origin| {
            let at = screen(rect, view, origin);
            painter.circle_filled(at, 6.0, Color32::from_black_alpha(220));
            crate::context_bar::paint_transform_origin(painter, at, accent, 4.0);
        };
        let origin = self.input.displayed_transform_origin(session);
        if kind == ToolKind::Move && !crate::canvas_input::move_selection_box_visible(session) {
            if let Some(origin) = origin { paint_origin(origin); }
            return;
        }
        let stroke = Stroke::new(1.25, accent.gamma_multiply(0.70));
        let handle_fill = Color32::WHITE.gamma_multiply(0.35);
        // Keep the drawing separate from the larger hit target in the Move tool.
        let side = 7.0;
        let selection = &session.selection.ids;
        let move_tool = &self.input.move_tool;
        let map = self.input.preview.as_ref()
            .filter(|_| interactive && move_tool.dragging && !move_tool.duplicate_on_drag && selection.len() == 1)
            .map(dtool_core::command::selection_box_matrix)
            .unwrap_or(IDENTITY);
        let to_screen = |point| screen(rect, view, dtool_core::geometry::apply_matrix(&map, point));
        for id in selection {
            let Some(object) = session.document.get_object(*id) else { continue };
            let quad = dtool_core::scene_hit::selection_quad(&self.scene, &session.document, *id)
                .unwrap_or_else(|| object_quad(object, &IDENTITY)).map(to_screen);
            if session.selection.explicit_key() == Some(*id) {
                let center = quad[0].lerp(quad[2], 0.5);
                let outer = std::array::from_fn::<_, 4, _>(|i| {
                    let corner = quad[i];
                    let normal = |other: Pos2| {
                        let n = (other - corner).rot90().normalized();
                        if n.dot(center - corner) > 0.0 { -n } else { n }
                    };
                    let a = normal(quad[(i + 3) % 4]);
                    let b = normal(quad[(i + 1) % 4]);
                    corner + (a + b) * (3.25 / (1.0 + a.dot(b)).max(0.5))
                });
                painter.add(egui::Shape::closed_line(outer.to_vec(), Stroke::new(1.5, accent)));
            }
            painter.add(egui::Shape::closed_line(quad.to_vec(), stroke));
        }
        let Some(frame) = dtool_core::scene_hit::selection_frame(&self.scene, &session.document, &session.selection.ids) else { return };
        let [x, y, w, h] = frame.bounds;
        if session.selection.ids.len() > 1 {
            let area = Rect::from_two_pos(screen(rect, view, [x, y]), screen(rect, view, [x + w, y + h]));
            painter.rect_stroke(area, 0.0, stroke, egui::StrokeKind::Middle);
        }
        let top = to_screen(frame.grip([0.5, 0.0]));
        let rotate_offset = crate::canvas_input::ROTATE_PX / view.zoom.max(1e-4);
        let knob_point = frame.to_world([x + w * 0.5, y - rotate_offset]);
        let stem = screen(rect, view, knob_point).distance(screen(rect, view, frame.grip([0.5, 0.0])));
        let knob = top + (to_screen(knob_point) - top).normalized() * stem;
        if session.active_tool != "tool.artboard" {
            painter.line_segment([top, knob], stroke);
            painter.circle(knob, side * 0.5, handle_fill, stroke);
        }
        for frac in [[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 0.5], [1.0, 1.0], [0.5, 1.0], [0.0, 1.0], [0.0, 0.5]] {
            let at = to_screen(frame.grip(frac));
            let handle = Rect::from_center_size(at, egui::vec2(side, side));
            if kind == crate::canvas_input::ToolKind::Move || kind == crate::canvas_input::ToolKind::Node {
                painter.circle(at, side * 0.5, Color32::WHITE, stroke);
            } else {
                painter.rect_filled(handle, 0.0, handle_fill);
                painter.rect_stroke(handle, 0.0, stroke, egui::StrokeKind::Inside);
            }
        }
        if session.active_tool == "tool.artboard" { return; }
        if kind == ToolKind::Move && matches!(selection.as_slice(), [id]
            if session.document.get_object(*id).is_some_and(|object| {
                matches!(object.kind, dtool_core::ObjectKind::FrameText { .. })
            })) {
            let corner = frame.grip([1.0, 1.0]);
            let gap = rotate_offset * 0.58;
            let grip = frame.to_world([x + w + gap, y + h + gap]);
            let distance = screen(rect, view, corner).distance(screen(rect, view, grip));
            let corner = to_screen(corner);
            let at = corner + (to_screen(grip) - corner).normalized() * distance;
            painter.line_segment([corner, at], stroke);
            let points = crate::canvas_tools::diamond(at, side * 0.65);
            painter.add(egui::Shape::convex_polygon(points, Color32::WHITE, stroke));
        }
        for grip in crate::canvas_tools::shear_grips(frame.bounds, view.zoom) {
            let points = crate::canvas_tools::diamond(to_screen(frame.to_world(grip)), side * 0.5);
            painter.add(egui::Shape::convex_polygon(points, handle_fill, stroke));
        }
        if let Some(origin) = origin { paint_origin(origin); }
    }
}
