use crate::view_state::{ViewState, ZOOM_MAX, ZOOM_MIN};
use dtool_protocol::CommandError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewTarget {
    Page(crate::document::PageId),
    Artboard(crate::object::ObjectId),
}

impl ViewTarget {
    pub fn artboards(doc: &crate::document::Document) -> impl Iterator<Item = &crate::object::Object> {
        doc.objects().iter().filter(|object| {
            (matches!(object.kind, crate::object::ObjectKind::Artboard) || crate::artboard::is_artboard(object))
                && crate::scene::is_drawn(doc, object.id)
        })
    }

    fn regions(doc: &crate::document::Document) -> impl Iterator<Item = (Self, [f32; 4])> + '_ {
        let artboards = Self::artboards(doc)
            .map(|object| (Self::Artboard(object.id), crate::geometry::object_world_bounds(object)));
        let pages = doc.pages.iter().zip(crate::scene::page_rects(doc)).enumerate()
            .filter(|(index, _)| !crate::artboard::edit::is_backing_page(doc, *index))
            .map(|(_, (page, rect))| (Self::Page(page.id), rect));
        artboards.chain(pages).filter(|(_, rect)| {
            rect.iter().all(|value| value.is_finite()) && rect[2] > 0.0 && rect[3] > 0.0
        })
    }

    pub fn bounds(self, doc: &crate::document::Document) -> Option<[f32; 4]> {
        match self {
            Self::Page(id) => doc.pages.iter().position(|page| page.id == id)
                .map(|index| crate::data_merge::layout::page_rect(doc, index))
                .filter(|bounds| bounds.iter().all(|value| value.is_finite()) && bounds[2] > 0.0 && bounds[3] > 0.0),
            Self::Artboard(_) => Self::regions(doc).find_map(|(target, bounds)| (target == self).then_some(bounds)),
        }
    }

    pub fn first(doc: &crate::document::Document) -> Option<[f32; 4]> {
        if doc.layout_artboard_origins.is_some()
            && let Some(bounds) = doc.pages.first().and_then(|page| Self::Page(page.id).bounds(doc))
        {
            return Some(bounds);
        }
        Self::regions(doc).find(|(target, _)| {
            !matches!(target, Self::Artboard(id) if doc.master_pages.roots.contains(id))
        }).map(|(_, bounds)| bounds)
    }

    pub fn at_view_center(doc: &crate::document::Document, view: &ViewState, size: [f32; 2]) -> Option<Self> {
        Self::at_view_center_with_hint(doc, view, size, None)
    }

    pub fn at_view_center_with_hint(
        doc: &crate::document::Document, view: &ViewState, size: [f32; 2], hint: Option<Self>,
    ) -> Option<Self> {
        let [px, py] = view_to_doc(view, size.map(|side| side * 0.5));
        let score = |(target, [x, y, w, h]): (Self, [f32; 4])| {
            let dx = (x - px).max(px - (x + w)).max(0.0);
            let dy = (y - py).max(py - (y + h)).max(0.0);
            let distance = dx * dx + dy * dy;
            let center = (px - x - w * 0.5).powi(2) + (py - y - h * 0.5).powi(2);
            (distance, matches!(target, Self::Page(_)), center, Some(target) != hint)
        };
        Self::regions(doc).min_by(|&a, &b| {
            let (distance_a, page_a, center_a, other_a) = score(a);
            let (distance_b, page_b, center_b, other_b) = score(b);
            distance_a.total_cmp(&distance_b)
                .then(page_a.cmp(&page_b))
                .then(center_a.total_cmp(&center_b))
                .then(other_a.cmp(&other_b))
        }).map(|(target, _)| target)
    }
}

pub fn center_region(view: &mut ViewState, [x, y, w, h]: [f32; 4], size: [f32; 2]) {
    let center = doc_to_view(view, [x + w * 0.5, y + h * 0.5]);
    for (axis, scroll) in view.scroll.iter_mut().enumerate() {
        *scroll += size[axis] * 0.5 - center[axis];
    }
}

pub use crate::command::NEW_DOCUMENT_BLEED_MM as DEFAULT_BLEED_MM;

pub fn bleed_margins(doc: &crate::Document) -> [f32; 4] {
    let b = doc.setup.bleed;
    if !b.enabled {
        return [0.0; 4];
    }
    let sides = [b.left, b.top, b.right, b.bottom].map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    if sides.iter().any(|v| *v > 0.0) {
        return sides;
    }
    let px = crate::units::to_px(DEFAULT_BLEED_MM, crate::document::Units::Mm, doc.dpi).unwrap_or(0.0);
    [crate::units::from_px(px, doc.units, doc.dpi).unwrap_or(px); 4]
}

pub fn bleed_outlines(doc: &crate::Document) -> impl Iterator<Item = (ViewTarget, [[f32; 2]; 4])> + '_ {
    let [l, t, r, b] = bleed_margins(doc);
    let has_bleed = [l, t, r, b].iter().any(|side| *side > 0.0);
    let pages = doc.pages.iter().zip(crate::scene::page_rects(doc)).enumerate()
        .filter(|(index, _)| !crate::artboard::edit::is_backing_page(doc, *index))
        .map(|(_, (page, rect))| (ViewTarget::Page(page.id), rect, None));
    let artboards = ViewTarget::artboards(doc).map(|board| {
        (ViewTarget::Artboard(board.id),
            [board.pos[0], board.pos[1], board.size[0].abs(), board.size[1].abs()],
            Some(crate::geometry::object_matrix(board)))
    });
    pages.chain(artboards).take_while(move |_| has_bleed).map(move |(target, [x, y, w, h], matrix)| {
        let [x0, y0, x1, y1] = [x - l, y - t, x + w + r, y + h + b];
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
            .map(|p| matrix.map_or(p, |m| crate::geometry::apply_matrix(&m, p)));
        (target, corners)
    })
}

pub const VIEW_ZOOM_FIT: &str = "view.zoom.fit";
pub const VIEW_ZOOM_TO_WIDTH: &str = "view.zoom.to_width";
pub const VIEW_ZOOM_TO_SELECTION: &str = "view.zoom.to_selection";
pub const VIEW_ZOOM_TO_DOCUMENT: &str = "view.zoom.to_document";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FitTarget {
    Current,
    Width,
    Selection,
    Document,
}

impl FitTarget {
    pub(crate) fn includes_bleed(self, view: &ViewState) -> bool {
        self == Self::Current && view.canvas_flags.show_bleed && !view.canvas_flags.preview_mode
    }

    pub(crate) fn for_command(name: &str) -> Option<Self> {
        match name {
            VIEW_ZOOM_FIT => Some(Self::Current),
            VIEW_ZOOM_TO_WIDTH => Some(Self::Width),
            VIEW_ZOOM_TO_SELECTION => Some(Self::Selection),
            VIEW_ZOOM_TO_DOCUMENT => Some(Self::Document),
            _ => None,
        }
    }

    pub(crate) fn command(self) -> &'static str {
        match self {
            Self::Current => VIEW_ZOOM_FIT,
            Self::Width => VIEW_ZOOM_TO_WIDTH,
            Self::Selection => VIEW_ZOOM_TO_SELECTION,
            Self::Document => VIEW_ZOOM_TO_DOCUMENT,
        }
    }

    pub(crate) fn bounds(self, session: &crate::Session, view_size: [f32; 2]) -> Option<[f32; 4]> {
        use crate::commands::align::union_rects;
        use crate::data_merge::layout::spread_ranges;

        let doc = &session.document;
        match self {
            Self::Selection => session.selection_bounds().or_else(|| {
                let mask = doc.pixel_selection.as_ref()?;
                let surface = crate::selection::PixelSurface::for_page(doc, mask.page_id)?;
                if !surface.matches(doc, mask) {
                    return None;
                }
                let [x, y, width, height] = mask.bounds()?;
                let [x, y] = surface.pixel_to_doc([x as f32, y as f32]);
                Some([x, y, width as f32 / surface.scale, height as f32 / surface.scale])
            }),
            Self::Document => {
                let bounds: Vec<_> = if doc.layout_artboard_origins.is_some() {
                    crate::scene::page_rects(doc)
                } else {
                    ViewTarget::regions(doc).map(|(_, bounds)| bounds).collect()
                };
                union_rects(&bounds)
            }
            Self::Current | Self::Width => {
                let current = ViewTarget::at_view_center_with_hint(
                    doc, &session.view, view_size, session.view_target_hint,
                );
                if self.includes_bleed(&session.view)
                    && let Some(corners) = bleed_outlines(doc).find_map(|(target, corners)| (Some(target) == current).then_some(corners))
                    && let Some([x0, y0, x1, y1]) = crate::geometry::points_bounds(&corners)
                {
                    return Some([x0, y0, x1 - x0, y1 - y0]);
                }
                let page_index = match current {
                    Some(ViewTarget::Page(id)) => doc.pages.iter().position(|page| page.id == id),
                    Some(ViewTarget::Artboard(id)) => doc.pages.iter().position(|page| page.artboard_id == Some(id)),
                    None => None,
                };
                if self == Self::Width && doc.layout_artboard_origins.is_some()
                    && let Some(index) = page_index
                    && let Some(spread) = spread_ranges(doc).into_iter().find(|range| range.contains(&index))
                {
                    return union_rects(&crate::scene::page_rects(doc)[spread]);
                }
                current.and_then(|target| target.bounds(doc)).or_else(|| ViewTarget::first(doc))
            }
        }
        .filter(|bounds| bounds.iter().all(|value| value.is_finite()))
    }
}

pub fn actual_zoom(units: crate::document::Units) -> f32 {
    match units {
        crate::document::Units::Px => 1.0,
        // Windows and egui lay out the canvas in 96-DPI logical screen units.
        physical => crate::units::to_px(1.0, physical, 96.0).unwrap_or(1.0),
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewZoomFitArgs {
    pub view_size: [f32; 2],
}

pub fn parse_view_size(params: &serde_json::Value) -> Result<[f32; 2], CommandError> {
    let args: ViewZoomFitArgs = serde_json::from_value(params.clone()).map_err(|_| crate::command::invalid_params_error())?;
    let ok = args.view_size.iter().all(|v| v.is_finite() && *v > 0.0);
    if ok { Ok(args.view_size) } else { Err(crate::command::invalid_params_error()) }
}

pub fn rotated_bounds(page_size: [f32; 2], rotation_deg: f32) -> [f32; 2] {
    let rad = rotation_deg.to_radians();
    let cos = rad.cos().abs();
    let sin = rad.sin().abs();
    [page_size[0] * cos + page_size[1] * sin, page_size[0] * sin + page_size[1] * cos]
}

pub fn fit_scroll(page_size: [f32; 2], view_size: [f32; 2], zoom: f32, rotation_deg: f32) -> [f32; 2] {
    let probe = ViewState { zoom, scroll: [0.0, 0.0], rotation_deg, ..ViewState::default() };
    let centre = doc_to_view(&probe, [page_size[0] * 0.5, page_size[1] * 0.5]);
    [view_size[0] * 0.5 - centre[0], view_size[1] * 0.5 - centre[1]]
}

pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_nan() {
        return 1.0;
    }
    zoom.clamp(ZOOM_MIN, ZOOM_MAX)
}

fn safe_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 }
}

pub fn rotate_vec(rotation_deg: f32, v: [f32; 2]) -> [f32; 2] {
    let (sin, cos) = rotation_deg.to_radians().sin_cos();
    [v[0] * cos - v[1] * sin, v[0] * sin + v[1] * cos]
}

pub fn canvas_to_doc(state: &ViewState, canvas_min: [f32; 2], screen: [f32; 2]) -> [f32; 2] {
    view_to_doc(state, [screen[0] - canvas_min[0], screen[1] - canvas_min[1]])
}

pub fn doc_to_canvas(state: &ViewState, canvas_min: [f32; 2], doc: [f32; 2]) -> [f32; 2] {
    let v = doc_to_view(state, doc);
    [v[0] + canvas_min[0], v[1] + canvas_min[1]]
}

pub fn zoom_about(state: &mut ViewState, zoom: f32, pivot: [f32; 2]) {
    let old = safe_zoom(state.zoom);
    let zoom = clamp_zoom(zoom);
    let doc_x = (pivot[0] - state.scroll[0]) / old;
    let doc_y = (pivot[1] - state.scroll[1]) / old;
    state.scroll = [pivot[0] - doc_x * zoom, pivot[1] - doc_y * zoom];
    state.zoom = zoom;
}

pub fn pan_by_doc(state: &mut ViewState, doc_delta: [f32; 2]) {
    let zoom = safe_zoom(state.zoom);
    let r = rotate_vec(state.rotation_deg, doc_delta);
    state.scroll[0] -= r[0] * zoom;
    state.scroll[1] -= r[1] * zoom;
}

pub fn visible_doc_quad(state: &ViewState, view_size: [f32; 2]) -> [[f32; 2]; 4] {
    [
        view_to_doc(state, [0.0, 0.0]),
        view_to_doc(state, [view_size[0], 0.0]),
        view_to_doc(state, [view_size[0], view_size[1]]),
        view_to_doc(state, [0.0, view_size[1]]),
    ]
}

pub fn fit_zoom(page_size: [f32; 2], view_size: [f32; 2], rotation_deg: f32) -> f32 {
    let [rotated_w, rotated_h] = rotated_bounds(page_size, rotation_deg);

    if rotated_w <= 0.0 || rotated_h <= 0.0 {
        return ZOOM_MIN;
    }

    let zoom_x = view_size[0] / rotated_w;
    let zoom_y = view_size[1] / rotated_h;

    clamp_zoom(zoom_x.min(zoom_y))
}

pub fn view_to_doc(state: &ViewState, point: [f32; 2]) -> [f32; 2] {
    let rad = state.rotation_deg.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();

    let dx = point[0] - state.scroll[0];
    let dy = point[1] - state.scroll[1];

    let inv_zoom = 1.0 / safe_zoom(state.zoom);

    let rx = dx * inv_zoom;
    let ry = dy * inv_zoom;

    let doc_x = rx * cos + ry * sin;
    let doc_y = -rx * sin + ry * cos;

    [doc_x, doc_y]
}

pub fn doc_to_view(state: &ViewState, point: [f32; 2]) -> [f32; 2] {
    let rad = state.rotation_deg.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();

    let rx = point[0] * cos - point[1] * sin;
    let ry = point[0] * sin + point[1] * cos;

    let view_x = rx * state.zoom + state.scroll[0];
    let view_y = ry * state.zoom + state.scroll[1];

    [view_x, view_y]
}
