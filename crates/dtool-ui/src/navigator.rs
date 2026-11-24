use dtool_core::{CanvasFlags, Document, ViewState};
use dtool_design;
use dtool_strings::text;

pub struct NavigatorPanel {
    pub view_box_dragging: bool,
    pub drag_start_view: [f32; 2],
    pub drag_start_scroll: [f32; 2],
}

impl Default for NavigatorPanel {
    fn default() -> Self {
        Self {
            view_box_dragging: false,
            drag_start_view: [0.0, 0.0],
            drag_start_scroll: [0.0, 0.0],
        }
    }
}

impl NavigatorPanel {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        state: &ViewState,
        thumbnail: &Document,
        page_size: [f32; 2],
        view_size: [f32; 2],
        actual: f32,
        show_guides: bool,
        show_bleed: bool,
        on_pan: &dyn Fn([f32; 2]),
        on_zoom_set: &dyn Fn(f32),
        on_toggle_guides: &dyn Fn(bool),
        on_toggle_bleed: &dyn Fn(bool),
    ) {
        let thumb_bg = dtool_design::color("navigator.thumb.bg");
        let view_box_stroke = dtool_design::color("navigator.viewbox.stroke");
        let view_box_fill = dtool_design::color("navigator.viewbox.fill");

        let header_text = text("panel.navigator.title");
        dtool_design::panel_header(ui, header_text);

        let thumb_size = [
            dtool_design::space("nav.thumb.width"),
            dtool_design::space("nav.thumb.height"),
        ];

        let (thumb_rect, response) = ui.allocate_exact_size(
            egui::vec2(thumb_size[0], thumb_size[1]),
            egui::Sense::click_and_drag(),
        );

        if response.drag_started() {
            self.view_box_dragging = true;
            self.drag_start_view = [response.interact_pointer_pos().unwrap_or_default()[0], response.interact_pointer_pos().unwrap_or_default()[1]];
            self.drag_start_scroll = state.scroll;
        }

        let drag_scale = (thumb_size[0] / page_size[0].max(1.0)).min(thumb_size[1] / page_size[1].max(1.0));
        if response.dragged() && self.view_box_dragging {
            let d = response.drag_delta();
            if d.x != 0.0 || d.y != 0.0 {
                on_pan(view_box_drag_pan(state, [d.x, d.y], drag_scale));
            }
        }

        if response.drag_stopped() {
            self.view_box_dragging = false;
        }

        if response.clicked() {
            if let Some(point) = response.interact_pointer_pos() {
                let ox = (thumb_size[0] - page_size[0] * drag_scale) * 0.5;
                let oy = (thumb_size[1] - page_size[1] * drag_scale) * 0.5;
                let doc_point = [((point.x-thumb_rect.left()-ox)/drag_scale).clamp(0.0,page_size[0]),
                    ((point.y-thumb_rect.top()-oy)/drag_scale).clamp(0.0,page_size[1])];
                let view_point = dtool_core::doc_to_view(state, doc_point);
                on_pan([view_size[0]*0.5-view_point[0],view_size[1]*0.5-view_point[1]]);
            }
        }

        let painter = ui.painter();

        painter.rect_filled(
            thumb_rect,
            dtool_design::radius("nav.thumb.radius"),
            thumb_bg,
        );

        let scale_x = thumb_size[0] / page_size[0].max(1.0);
        let scale_y = thumb_size[1] / page_size[1].max(1.0);
        let scale = scale_x.min(scale_y);

        let offset_x = (thumb_size[0] - page_size[0] * scale) * 0.5;
        let offset_y = (thumb_size[1] - page_size[1] * scale) * 0.5;

        let thumb_items = thumbnail_item_bounds(thumbnail, &state.canvas_flags);
        if !thumb_items.is_empty() {
            for bounds in thumb_items {
                let x0 = thumb_rect.min.x + offset_x + bounds[0] * scale;
                let y0 = thumb_rect.min.y + offset_y + bounds[1] * scale;
                let x1 = thumb_rect.min.x + offset_x + (bounds[0] + bounds[2]) * scale;
                let y1 = thumb_rect.min.y + offset_y + (bounds[1] + bounds[3]) * scale;

                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)),
                    0.0,
                    dtool_design::color("navigator.thumb.item"),
                );
            }
        } else {
            let x0 = thumb_rect.min.x + offset_x;
            let y0 = thumb_rect.min.y + offset_y;
            let x1 = thumb_rect.min.x + offset_x + page_size[0] * scale;
            let y1 = thumb_rect.min.y + offset_y + page_size[1] * scale;

            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)),
                0.0,
                dtool_design::color("navigator.thumb.page"),
            );
        }

        let origin = [thumb_rect.min.x + offset_x, thumb_rect.min.y + offset_y];
        let quad = view_box_quad(state, view_size, origin, scale);
        let points: Vec<egui::Pos2> = quad.iter().map(|p| egui::pos2(p[0], p[1])).collect();
        let edge = dtool_design::stroke_width("thin").max(1.0);
        painter.with_clip_rect(thumb_rect).add(egui::Shape::convex_polygon(points, view_box_fill, egui::Stroke::new(edge, view_box_stroke)));

        ui.add_space(dtool_design::space("sm"));

        let zoom_min = dtool_tools::ZOOM_MIN;
        let zoom_max = dtool_tools::ZOOM_MAX;

        let mut slider_value = state.zoom / actual;
        let slider_response = dtool_design::slider(
            ui,
            &mut slider_value,
            zoom_min..=zoom_max,
            text("panel.navigator.zoom"),
        );

        if slider_response.changed() {
            on_zoom_set(dtool_tools::clamp_zoom(slider_value * actual));
        }

        if response.hovered() && ui.input(|i| i.pointer.has_pointer()) {
            let zoom_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if zoom_delta != 0.0 {
                let new_zoom = dtool_tools::clamp_zoom(state.zoom * (1.0 + zoom_delta * 0.01));
                on_zoom_set(new_zoom);
            }
        }

        ui.add_space(dtool_design::space("sm"));

        let mut show_guides = show_guides;
        if dtool_design::toggle(ui, &mut show_guides, text("panel.navigator.show_guides")).changed() {
            on_toggle_guides(show_guides);
        }

        let mut show_bleed = show_bleed;
        if dtool_design::toggle(ui, &mut show_bleed, text("panel.navigator.show_bleed")).changed() {
            on_toggle_bleed(show_bleed);
        }
    }
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
