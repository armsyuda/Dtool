use dtool_core::{CanvasFlags, ContextId, Document, Session, ViewState};
use dtool_core::view::{ViewTarget, ZoomLimits};
use dtool_strings::text;
use egui::{ColorImage, Pos2, Rect, TextureHandle, TextureOptions, vec2};

type ThumbnailRevision = (Option<ContextId>, u64, u64, CanvasFlags);
type NavigatorImage = ([f32; 4], Vec<dtool_core::scene::Quad>, ColorImage);

struct NavigatorThumbnail {
    bounds: [f32; 4],
    paper: Vec<dtool_core::scene::Quad>,
    texture: TextureHandle,
}
const THUMBNAIL_PIXELS: f32 = 256.0;

#[derive(Default)]
pub struct NavigatorPanel {
    pub view_box_dragging: bool,
    pub drag_start_view: [f32; 2],
    pub drag_start_scroll: [f32; 2],
    thumbnail: Option<(ThumbnailRevision, Option<NavigatorThumbnail>)>,
    pub(crate) navigation_changed: bool,
    job: Option<crate::panels::ThumbnailJob<ThumbnailRevision, Option<NavigatorImage>>>,
}

impl NavigatorPanel {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        session: Option<&Session>,
        view_size: [f32; 2],
        on_pan: &dyn Fn([f32; 2]),
        on_zoom_set: &dyn Fn(f32),
        on_toggle_guides: &dyn Fn(bool),
        on_toggle_bleed: &dyn Fn(bool),
    ) {
        let actual = session.map_or(1.0, Session::actual_zoom);
        let zoom = session.map_or(actual, |session| session.view.zoom);
        ui.add_enabled_ui(session.is_some(), |ui| zoom_controls(ui, zoom, actual, on_zoom_set));
        ui.separator();

        let width = ui.available_width().max(1.0);
        let height = (ui.available_height() - 46.0).clamp(64.0,
            (width * 1.1).max(dtool_design::space("nav.thumb.height")));
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click_and_drag());
        ui.painter().rect_filled(rect, dtool_design::radius("nav.thumb.radius"), dtool_design::color("navigator.thumb.bg"));
        if response.drag_stopped() {
            self.view_box_dragging = false;
        }
        let Some(session) = session else {
            self.thumbnail = None;
            self.view_box_dragging = false;
            return;
        };
        let state = &session.view;
        self.update_thumbnail(ui.ctx(), session);

        if let Some((_, Some(thumbnail))) = &self.thumbnail {
            let [x, y, width, height] = thumbnail.bounds;
            let scale = (rect.width() / width).min(rect.height() / height);
            let image_rect = Rect::from_center_size(rect.center(), vec2(width * scale, height * scale));
            let origin = image_rect.min - vec2(x * scale, y * scale);
            let painter = ui.painter_at(rect);
            for quad in &thumbnail.paper {
                let corners = quad.map(|[x, y]| origin + vec2(x * scale, y * scale));
                painter.add(egui::Shape::convex_polygon(corners.to_vec(), egui::Color32::WHITE, egui::Stroke::NONE));
            }
            painter.image(thumbnail.texture.id(), image_rect, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);

            let quad = view_box_quad(state, view_size, [origin.x, origin.y], scale);
            let points = quad.map(|point| egui::pos2(point[0], point[1])).to_vec();
            let edge = dtool_design::stroke_width("thin").max(1.0);
            painter.add(egui::Shape::convex_polygon(points, dtool_design::color("navigator.viewbox.fill"),
                egui::Stroke::new(edge, dtool_design::color("navigator.viewbox.stroke"))));

            if let Some(point) = response.interact_pointer_pos() {
                if response.drag_started() {
                    self.view_box_dragging = true;
                    self.drag_start_view = (point - response.total_drag_delta().unwrap_or_default()).into();
                    self.drag_start_scroll = state.scroll;
                }
                if response.dragged() && self.view_box_dragging {
                    let delta = [point.x - self.drag_start_view[0], point.y - self.drag_start_view[1]];
                    let pan = view_box_drag_pan(state, delta, scale);
                    let remaining = std::array::from_fn(|axis| self.drag_start_scroll[axis] + pan[axis] - state.scroll[axis]);
                    if remaining != [0.0, 0.0] {
                        on_pan(remaining);
                    }
                }
            }
            if response.clicked()
                && let Some(point) = response.interact_pointer_pos()
            {
                let doc = (image_rect.clamp(point) - origin) / scale;
                let shown = dtool_core::doc_to_view(state, [doc.x, doc.y]);
                on_pan([view_size[0] * 0.5 - shown[0], view_size[1] * 0.5 - shown[1]]);
            }
        }
        if response.hovered() {
            let wheel = ui.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta.y));
            if wheel != 0.0 {
                let zoom = ZoomLimits::for_actual(actual).clamp(zoom * (wheel * 0.004).exp());
                on_zoom_set(zoom);
            }
        }

        ui.add_space(dtool_design::space("sm"));
        let mut guides = session.guides.show;
        if dtool_design::toggle(ui, &mut guides, text("panel.navigator.show_guides")).changed() {
            on_toggle_guides(guides);
        }
        let mut bleed = state.canvas_flags.show_bleed;
        if dtool_design::toggle(ui, &mut bleed, text("panel.navigator.show_bleed")).changed() {
            on_toggle_bleed(bleed);
        }
    }

    fn update_thumbnail(&mut self, ctx: &egui::Context, session: &Session) {
        use std::sync::mpsc::TryRecvError;

        let flags = &session.view.canvas_flags;
        let revision = (session.context, session.artwork_generation(), session.document.version, flags.clone());
        if self.thumbnail.as_ref().is_some_and(|(old, ..)| old.0 != revision.0 || old.3 != revision.3) {
            self.thumbnail = None;
            self.view_box_dragging = false;
        }
        if let Some(job) = &self.job {
            match job.result.try_recv() {
                Ok(image) if job.revision == revision => {
                    let image = image.map(|(bounds, paper, image)| NavigatorThumbnail {
                        bounds, paper,
                        texture: ctx.load_texture("navigator overview", image, TextureOptions::LINEAR),
                    });
                    self.thumbnail = Some((revision.clone(), image));
                }
                Ok(_) | Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => return,
            }
            self.job = None;
        }
        if self.thumbnail.as_ref().is_none_or(|(stamp, ..)| stamp != &revision) {
            let doc = session.document.clone();
            let flags = flags.clone();
            self.job = crate::panels::ThumbnailJob::start(ctx, "dtool-navigator-thumbnail", revision,
                move || navigator_image(&doc, &flags)).ok();
        }
    }
}

fn zoom_controls(ui: &mut egui::Ui, zoom: f32, actual: f32, on_zoom_set: &dyn Fn(f32)) {
    let min = dtool_core::ZOOM_MIN * 100.0;
    let max = dtool_core::ZOOM_MAX * 100.0;
    let mut percent = (zoom / actual * 10_000.0).round() / 100.0;
    let mut changed = false;
    let label = text("panel.navigator.zoom");
    let limits = ZoomLimits::for_actual(actual);
    ui.horizontal(|ui| {
        let width = 20.0;
        let input_width = (ui.available_width() - 2.0 * width - 3.0 * ui.spacing().item_spacing.x).clamp(30.0, 62.0);
        let rail_width = ui.available_width() - 2.0 * width - input_width - 3.0 * ui.spacing().item_spacing.x;
        if ui.add_sized([width, ui.spacing().interact_size.y], egui::Button::new("−"))
            .on_hover_text(text("context.zoom.out")).clicked()
        {
            on_zoom_set(limits.clamp(zoom / dtool_core::command::ZOOM_STEP));
        }
        if rail_width >= 12.0 {
            changed |= dtool_design::logarithmic_slider_rail(ui, &mut percent, min..=max, rail_width, label,
                ui.is_enabled(), f64::from(min)).changed();
        }
        if ui.add_sized([width, ui.spacing().interact_size.y], egui::Button::new("+"))
            .on_hover_text(text("context.zoom.in")).clicked()
        {
            on_zoom_set(limits.clamp(zoom * dtool_core::command::ZOOM_STEP));
        }
        let field = egui::DragValue::new(&mut percent).range(min..=max).suffix(" %").max_decimals(2)
            .clamp_existing_to_range(false).update_while_editing(false);
        changed |= ui.add_sized([input_width, ui.spacing().interact_size.y], field).on_hover_text(label).changed();
    });
    if changed {
        on_zoom_set(limits.clamp(percent / 100.0 * actual));
    }
}

fn navigator_image(doc: &Document, flags: &CanvasFlags) -> Option<NavigatorImage> {
    let pages = dtool_core::scene::page_rects(doc).into_iter().enumerate()
        .filter(|(index, _)| !dtool_core::artboard::edit::is_backing_page(doc, *index))
        .map(|(_, rect)| dtool_core::geometry::rect_quad(rect, &dtool_core::scene::IDENTITY));
    let boards = ViewTarget::artboards(doc).map(dtool_core::geometry::object_world_quad);
    let paper: Vec<_> = pages.chain(boards).filter(|quad| quad.iter().flatten().all(|v| v.is_finite())).collect();
    let [x, y, right, bottom] = paper.iter().flatten().copied()
        .map(|[x, y]| [x, y, x, y])
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])?;
    let [width, height] = [right - x, bottom - y];
    if ![width, height].iter().all(|side| side.is_finite() && *side > 0.0) {
        return None;
    }
    let bounds = [x, y, width, height];
    let scale = THUMBNAIL_PIXELS / width.max(height);
    let [w, h] = [width, height].map(|axis| (axis * scale).round().max(1.0) as u32);
    let pixels = dtool_render::scene::with_fill_antialiasing(|| {
        dtool_core::layer_style::raster::render_region(doc, flags, &[], bounds, w, h)
    });
    Some((bounds, paper, ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels)))
}

fn adjust_thumbnail_bounds(doc: &Document, flags: &CanvasFlags, mut bounds: [f32; 4]) -> Option<[f32; 4]> {
    if flags.force_pixel_alignment {
        bounds = bounds.map(|value| value.round());
    }
    if flags.clip_to_canvas {
        let size = doc.pages.get(0).map(|page| page.size).unwrap_or([0.0, 0.0]);
        let (x_min, y_min, x_max, y_max) = (0.0, 0.0, size[0], size[1]);
        let (item_x, item_y, item_w, item_h) = (bounds[0], bounds[1], bounds[2], bounds[3]);
        if item_x >= x_max || item_y >= y_max || item_x + item_w <= x_min || item_y + item_h <= y_min {
            return None;
        }
        let clipped_x = item_x.max(x_min);
        let clipped_y = item_y.max(y_min);
        let clipped_right = (item_x + item_w).min(x_max);
        let clipped_bottom = (item_y + item_h).min(y_max);
        bounds = [clipped_x, clipped_y, clipped_right - clipped_x, clipped_bottom - clipped_y];
    }
    Some(bounds)
}

fn push_thumbnail_bounds(items: &mut Vec<[f32; 4]>, doc: &Document, flags: &CanvasFlags, bounds: [f32; 4]) {
    if let Some(bounds) = adjust_thumbnail_bounds(doc, flags, bounds) {
        items.push(bounds);
    }
}

pub fn thumbnail_item_bounds(doc: &Document, flags: &CanvasFlags) -> Vec<[f32; 4]> {
    let mut items = Vec::new();
    for obj in &doc.objects {
        if !dtool_core::scene::effective_pickable(doc, obj.id) {
            continue;
        }
        if let dtool_core::object::ObjectKind::Dimension(_) = &obj.kind {
            if let Some(geometry) = dtool_core::outline::dimension_geometry(obj) {
                let mut segments = vec![geometry.line];
                segments.extend(geometry.extensions.iter().copied());
                segments.extend(geometry.ticks.iter().copied());
                for [a, b] in segments {
                    let points = [[a[0] as f32, a[1] as f32], [b[0] as f32, b[1] as f32]];
                    push_thumbnail_bounds(&mut items, doc, flags, [
                        points[0][0].min(points[1][0]),
                        points[0][1].min(points[1][1]),
                        (points[0][0] - points[1][0]).abs(),
                        (points[0][1] - points[1][1]).abs(),
                    ]);
                }
                let at = [geometry.label_at[0] as f32, geometry.label_at[1] as f32];
                push_thumbnail_bounds(&mut items, doc, flags, [at[0], at[1], 0.0, 0.0]);
            }
            continue;
        }
        push_thumbnail_bounds(&mut items, doc, flags, [obj.pos[0], obj.pos[1], obj.size[0].abs(), obj.size[1].abs()]);
    }
    use dtool_tools::dtool_text::notes_layout::{layout_notes, note_line_height};
    for note in layout_notes(doc, note_line_height(doc)) {
        if !dtool_core::scene::effective_pickable(doc, note.frame) {
            continue;
        }
        let rect = note.rect;
        push_thumbnail_bounds(&mut items, doc, flags, [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]]);
    }
    for mark in dtool_core::longdoc::marks::layout_marks(doc) {
        if !dtool_core::scene::effective_pickable(doc, mark.frame) {
            continue;
        }
        let rect = mark.rect;
        push_thumbnail_bounds(&mut items, doc, flags, [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]]);
    }
    items
}

pub fn view_box_quad(state: &ViewState, view_size: [f32; 2], origin: [f32; 2], scale: f32) -> [[f32; 2]; 4] {
    dtool_core::view::visible_doc_quad(state, view_size).map(|p| [origin[0] + p[0] * scale, origin[1] + p[1] * scale])
}

pub fn view_box_drag_pan(state: &ViewState, thumb_delta: [f32; 2], scale: f32) -> [f32; 2] {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let mut moved = state.clone();
    dtool_core::view::pan_by_doc(&mut moved, [thumb_delta[0] / scale, thumb_delta[1] / scale]);
    [moved.scroll[0] - state.scroll[0], moved.scroll[1] - state.scroll[1]]
}
