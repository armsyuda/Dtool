use dtool_core::command::Session;
use dtool_core::guides::{Guide, GuideId, GuideKind, Guides};
use dtool_core::snap_candidates::{CandidateKind, SnapDocument, snap_candidates, smart_guides, SmartGuideKind};
use dtool_core::ViewState;
use dtool_core::view::ViewTarget;
use dtool_core::rulers::{RulerState, MENU_SHOW_RULERS, MENU_SHOW_TEXT_RULER, RULER_ORIGIN_RESET, RULER_ORIGIN_SET, RULER_SHOW, RULER_UNIT_SET};
use dtool_core::CommandError;
use dtool_strings::text;
use egui::{Pos2, Rect, Sense, Ui, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulerSide {
    Top,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RulerDragTarget {
    NewGuide { side: RulerSide },
    ExistingGuide { id: GuideId },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RulerDrag {
    pub target: RulerDragTarget,
    pub doc_point: [f32; 2],
    raw_point: [f32; 2],
    pub alt: bool,
    pub over_ruler: bool,
}

impl RulerDrag {
    pub fn kind(&self, original: Option<GuideKind>) -> GuideKind {
        match self.target {
            RulerDragTarget::NewGuide { side } => guide_kind_for(side, self.alt),
            RulerDragTarget::ExistingGuide { .. } => original.unwrap_or(GuideKind::Horizontal),
        }
    }

    pub fn position(&self, kind: GuideKind) -> f32 {
        match kind {
            GuideKind::Horizontal => self.doc_point[1],
            GuideKind::Vertical | GuideKind::Column => self.doc_point[0],
        }
    }
}

fn snap_ruler_point(session: &Session, view: &ViewState, point: [f32; 2], excluded: Option<GuideId>) -> [f32; 2] {
    let mut prefs = session.snap_prefs.query().clone();
    if !prefs.enabled || !view.zoom.is_finite() || view.zoom <= 0.0 {
        return point;
    }
    prefs.max_candidates = prefs.max_candidates.max(crate::canvas_tools::SNAP_MAX_CANDIDATES);
    let doc = &session.document;
    let source = SnapDocument { document: doc, guides: &session.guides, grid: &doc.grid };
    let tolerance = prefs.screen_tolerance as f32 / view.zoom;
    let mut best: [Option<(f32, u8, f32)>; 2] = [None, None];
    let mut offer = |axis: usize, value: f32, priority| {
        let distance = (value - point[axis]).abs();
        if distance.is_finite() && distance <= tolerance
            && best[axis].is_none_or(|(previous, rank, _)| distance < previous || (distance == previous && priority < rank))
        {
            best[axis] = Some((distance, priority, value));
        }
    };
    for candidate in snap_candidates(&source, view, &prefs, point) {
        if candidate.kind == CandidateKind::Guide && excluded.is_some_and(|id| candidate.owner == Some(id.0)) { continue; }
        for axis in 0..2 {
            if candidate.point[axis] != point[axis] {
                offer(axis, candidate.point[axis], candidate.kind.priority());
            }
        }
    }
    if prefs.page_snapping {
        use dtool_core::view::ViewTarget;
        let boards = ViewTarget::artboards(doc).map(dtool_core::geometry::object_world_bounds);
        for [x, y, w, h] in dtool_core::scene::page_rects(doc).into_iter().chain(boards) {
            for x in [x, x + w * 0.5, x + w] { offer(0, x, CandidateKind::Page.priority()); }
            for y in [y, y + h * 0.5, y + h] { offer(1, y, CandidateKind::Page.priority()); }
        }
    }
    std::array::from_fn(|axis| best[axis].map_or(point[axis], |(_, _, value)| value))
}

fn guide_snap_increment(modifiers: egui::Modifiers) -> Option<f64> {
    if modifiers.ctrl {
        Some(5.0)
    } else if modifiers.shift {
        Some(1.0)
    } else {
        None
    }
}

fn snap_to_ruler_units(
    session: &Session,
    point: [f32; 2],
    kind: GuideKind,
    increment: f64,
) -> [f32; 2] {
    let ruler = ruler_state(session);
    let scale = f64::from(ruler.display_scale(session.document.units, session.document.dpi));
    let axis = usize::from(kind == GuideKind::Horizontal);
    let origin = f64::from(ruler.origin[axis]);
    let steps = (f64::from(point[axis]) - origin) * scale / increment;
    let mut snapped = point;
    let coordinate = (origin + steps.round() * increment / scale) as f32;
    if coordinate.is_finite() {
        snapped[axis] = coordinate;
    }
    snapped
}

fn guide_name(guide: &Guide) -> String {
    guide.name.as_deref().map(str::trim).filter(|name| !name.is_empty())
        .map(str::to_owned).unwrap_or_else(|| Guides::default_name(guide.id))
}

fn guide_label(ui: &Ui, canvas: Rect, guide: &Guide, [a, b]: [Pos2; 2]) -> Option<(Rect, std::sync::Arc<egui::Galley>)> {
    let [start, end] = crate::app_canvas::clip_frame_segment(
        [a.x, a.y], [b.x, b.y], [canvas.left(), canvas.top()], [canvas.width(), canvas.height()],
    )?;
    let delta = b - a;
    let a = a + delta * start;
    let b = a + delta * (end - start);
    let max_width = (canvas.width() - 10.0).min(186.0);
    if max_width < 8.0 || canvas.height() < 18.0 {
        return None;
    }
    let colour = dtool_design::color("text");
    let mut job = egui::text::LayoutJob::simple_singleline(guide_name(guide), egui::TextStyle::Small.resolve(ui.style()), colour);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let size = galley.size() + egui::vec2(6.0, 2.0);
    let point = if delta.x.abs() >= delta.y.abs() {
        let left = if a.x <= b.x { a } else { b };
        left + egui::vec2(5.0, -size.y - 2.0)
    } else {
        let top = if a.y <= b.y { a } else { b };
        top + egui::vec2(3.0, 5.0)
    };
    Some((Rect::from_min_size(point.clamp(canvas.min, (canvas.max - size).max(canvas.min)), size), galley))
}

fn ruler_units_menu(ui: &mut Ui, session: &mut Session) {
    crate::menu_tree::context_command_menu(ui, |ui| {
        let current = ruler_state(session).unit_value(None);
        for (value, label) in [
            (None, "ruler.unit.document"), (Some("Px"), "ruler.unit.px"), (Some("Mm"), "ruler.unit.mm"),
            (Some("Cm"), "ruler.unit.cm"), (Some("M"), "ruler.unit.m"), (Some("In"), "ruler.unit.in"),
            (Some("Pt"), "ruler.unit.pt"),
        ] {
            let label = text(label);
            let response = crate::menu_tree::menu_choice(ui, label, current == serde_json::json!(value));
            if crate::menu_tree::command_menu_tooltip(ui, response, label, false, None).clicked() {
                let _ = run_ruler(session, RULER_UNIT_SET, serde_json::json!({ "units": value }));
                ui.close();
            }
        }
    });
}

#[derive(Debug, Clone, PartialEq)]
pub enum RulerDropResult {
    None,
    Added(serde_json::Value),
    Moved(serde_json::Value),
    Deleted(serde_json::Value),
    Refused(String),
}

pub fn guide_kind_for(side: RulerSide, _alt: bool) -> GuideKind {
    match side {
        RulerSide::Top => GuideKind::Horizontal,
        RulerSide::Left => GuideKind::Vertical,
    }
}

pub fn screen_to_doc(view: &ViewState, canvas: Rect, screen: Pos2) -> [f32; 2] {
    dtool_core::view::canvas_to_doc(view, [canvas.min.x, canvas.min.y], [screen.x, screen.y])
}

fn paint_folds(ui: &Ui, doc: &dtool_core::Document, view: &ViewState, canvas: Rect) {
    let painter = ui.painter().with_clip_rect(canvas);
    let stroke = egui::Stroke::new(1.0, dtool_design::color("smart_guide"));
    for (_, segments) in dtool_core::page_fold::segments(doc) {
        for segment in segments {
            paint_dashed_guide(&painter, view, canvas, segment, stroke);
        }
    }
}

fn paint_dashed_guide(painter: &egui::Painter, view: &ViewState, canvas: Rect, segment: [[f32; 2]; 2], stroke: egui::Stroke) {
    let [a, b] = segment.map(|point| doc_to_screen(view, canvas, point));
    let Some([start, end]) = crate::app_canvas::clip_frame_segment(
        [a.x, a.y], [b.x, b.y], [canvas.min.x, canvas.min.y], [canvas.width(), canvas.height()],
    ) else { return };
    let delta = b - a;
    painter.extend(egui::Shape::dashed_line(&[a + delta * start, a + delta * end], stroke, 5.0, 4.0));
}

pub(crate) fn paint_column_guides(ui: &Ui, session: &Session, view: &ViewState, canvas: Rect, fill: &str, outline: &str) {
    use dtool_core::guides::column_layout::{Style, layouts};
    if !session.guides.show_columns || view.canvas_flags.preview_mode {
        return;
    }
    let painter = ui.painter().with_clip_rect(canvas);
    let accent = dtool_design::color("smart_guide");
    let fill = egui::Color32::from_hex(fill).unwrap_or_else(|_| accent.gamma_multiply(0.2));
    let outline = egui::Color32::from_hex(outline).unwrap_or(accent);
    let corners = |x0, y0, x1, y1| [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|point| doc_to_screen(view, canvas, point));
    for (style, grid) in layouts(&session.document) {
        let [x, y, w, h] = grid.bounds;
        let outer = corners(x, y, x + w, y + h);
        if !canvas.intersects(Rect::from_points(&outer)) {
            continue;
        }
        let [columns, rows] = grid.axes;
        match style {
            Style::Filled => {
                for [y0, y1] in rows.spans() {
                    for [x0, x1] in columns.spans() {
                        let points = corners(x0, y0, x1, y1);
                        if canvas.intersects(Rect::from_points(&points)) {
                            painter.add(egui::Shape::convex_polygon(points.to_vec(), fill, egui::Stroke::NONE));
                        }
                    }
                }
            }
            Style::Outline => {
                let stroke = egui::Stroke::new(1.0, outline);
                painter.add(egui::Shape::closed_line(outer.to_vec(), stroke));
                for (axis, span) in [[y, y + h], [x, x + w]].into_iter().enumerate() {
                    for edge in grid.boundaries(axis) {
                        let line = span.map(|along| {
                            let point = if axis == 0 { [edge, along] } else { [along, edge] };
                            doc_to_screen(view, canvas, point)
                        });
                        painter.line_segment(line, stroke);
                    }
                }
            }
        }
    }
}

pub(crate) fn paint_baseline_grid(ui: &Ui, session: &Session, view: &ViewState, canvas: Rect, colour: &str) {
    let grid = &session.document.grid;
    let threshold = grid.baseline.map_or(0.0, |settings| settings.threshold_percent);
    if !grid.show_baseline || view.canvas_flags.preview_mode || view.zoom / session.actual_zoom() * 100.0 < threshold {
        return;
    }
    let visible = [canvas.left_top(), canvas.right_top(), canvas.left_bottom(), canvas.right_bottom()]
        .map(|corner| screen_to_doc(view, canvas, corner));
    let doc_rect = Rect::from_points(&visible.map(|[x, y]| Pos2::new(x, y)));
    let painter = ui.painter_at(canvas);
    let colour = egui::Color32::from_hex(colour).unwrap_or(dtool_design::color("smart_guide"));
    let stroke = egui::Stroke::new(1.0, colour);
    for page in dtool_core::grid::baseline::pages(&session.document) {
        let [x, y, w, h] = page.rect;
        if !doc_rect.intersects(Rect::from_min_max(Pos2::new(x, y), Pos2::new(x + w, y + h))) { continue; }
        let step = page.spacing * (3.0 / (page.spacing * view.zoom)).ceil().max(1.0);
        if !step.is_finite() || step <= 0.0 { continue; }
        let first = page.first + ((doc_rect.top() - page.first) / step).ceil().max(0.0) * step;
        let end = doc_rect.bottom().min(y + h);
        for index in 0..2048 {
            let y = first + index as f32 * step;
            if !y.is_finite() || y > end { break; }
            painter.line_segment([[x, y], [x + w, y]].map(|point| doc_to_screen(view, canvas, point)), stroke);
        }
    }
}

pub fn doc_to_screen(view: &ViewState, canvas: Rect, doc: [f32; 2]) -> Pos2 {
    let p = dtool_core::view::doc_to_canvas(view, [canvas.min.x, canvas.min.y], doc);
    Pos2::new(p[0], p[1])
}

pub fn ticks_visible(view: &ViewState) -> bool {
    dtool_core::command::view_rotation_wrap(view.rotation_deg) == 0.0
}

pub const GUIDE_FAR: f32 = 100_000.0;

pub fn guide_segment(view: &ViewState, canvas: Rect, kind: GuideKind, position: f32) -> [Pos2; 2] {
    let (a, b) = match kind {
        GuideKind::Horizontal => ([-GUIDE_FAR, position], [GUIDE_FAR, position]),
        GuideKind::Vertical | GuideKind::Column => ([position, -GUIDE_FAR], [position, GUIDE_FAR]),
    };
    [doc_to_screen(view, canvas, a), doc_to_screen(view, canvas, b)]
}

fn screen_guide(doc: &dtool_core::Document, view: &ViewState, canvas: Rect, guide: &Guide) -> Option<[Pos2; 2]> {
    if guide.surface().is_some() {
        return guide.source_segment(doc).map(|segment| segment.map(|point| doc_to_screen(view, canvas, point)));
    }
    Some(guide_segment(view, canvas, guide.kind, guide.position))
}

fn dropped_surface(session: &Session, drag: RulerDrag) -> Option<ViewTarget> {
    match drag.target {
        RulerDragTarget::ExistingGuide { id } => session.guides.get(id)?.surface(),
        RulerDragTarget::NewGuide { .. } if session.document.layout_artboard_origins.is_some() => {
            let [px, py] = drag.raw_point;
            for &id in &session.document.master_pages.roots {
                let master = ViewTarget::Artboard(id);
                if dtool_core::guides::surface_bounds(&session.document, master).is_some_and(|[x, y, w, h]|
                    (x..=x+w).contains(&px) && (y..=y+h).contains(&py))
                {
                    return Some(master);
                }
            }
            let index = dtool_core::artboard::edit::page_at_point(&session.document, drag.raw_point)?;
            Some(ViewTarget::Page(session.document.pages[index].id))
        }
        _ => None,
    }
}

fn local_drop_position(session: &Session, drag: RulerDrag, kind: GuideKind, target: Option<ViewTarget>) -> f32 {
    let position = drag.position(kind);
    let Some(bounds) = target.and_then(|target| dtool_core::guides::surface_bounds(&session.document, target)) else { return position };
    let axis = kind.axis();
    (position - bounds[axis]).clamp(0.0, bounds[axis + 2].max(0.0))
}

pub fn distance_to_segment(p: Pos2, segment: [Pos2; 2]) -> f32 {
    let [a, b] = segment;
    let ab = b - a;
    let len = ab.length_sq();
    if len <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

fn error_text(err: &str) -> String {
    text(err).to_string()
}

fn guide_drag_id() -> egui::Id {
    egui::Id::unique("dtool.guides.drag_owner")
}

fn canvas_gesture_id() -> egui::Id {
    egui::Id::unique("dtool.canvas.gesture_owner")
}

pub fn guide_drag_active(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.get_temp::<(u64, bool)>(guide_drag_id())).is_some_and(|(at, owned)| owned && at == pass)
}

pub fn set_canvas_gesture(ctx: &egui::Context, active: bool) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(canvas_gesture_id(), (pass, active)));
}

pub fn canvas_gesture_active(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.get_temp::<(u64, bool)>(canvas_gesture_id())).is_some_and(|(at, active)| active && at + 1 >= pass)
}

pub fn ruler_state(session: &Session) -> RulerState {
    session.document.rulers
}

fn run_ruler(session: &mut Session, name: &str, params: serde_json::Value) -> Result<serde_json::Value, CommandError> {
    session.run_named(name, params)
}

#[derive(Debug, Clone, Default)]
pub struct Rulers {
    pub show: bool,
    pub drag: Option<RulerDrag>,
    pub origin_preview: Option<[f32; 2]>,
    origin_drag_start: Option<Pos2>,
    name_edit: Option<GuideNameEdit>,
    press_position: Option<Pos2>,
    grab_offset: [f32; 2],
}

#[derive(Debug, Clone)]
struct GuideNameEdit {
    context: Option<dtool_core::workspace::ContextId>,
    id: GuideId,
    name: String,
    focus: bool,
    error: Option<String>,
    chrome: crate::dialog_chrome::DialogChrome,
}

impl Rulers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn thickness() -> f32 {
        dtool_design::space("xl") + dtool_design::space("xs")
    }

    pub fn begin_new_guide(&mut self, side: RulerSide, doc_point: [f32; 2], alt: bool) {
        self.drag = Some(RulerDrag {
            target: RulerDragTarget::NewGuide { side },
            doc_point,
            raw_point: doc_point,
            alt,
            over_ruler: true,
        });
    }

    pub fn begin_move_guide(&mut self, session: &Session, id: GuideId, doc_point: [f32; 2]) -> bool {
        if session.guides.can_move(id).is_err() {
            return false;
        }
        self.drag = Some(RulerDrag {
            target: RulerDragTarget::ExistingGuide { id },
            doc_point,
            raw_point: doc_point,
            alt: false,
            over_ruler: false,
        });
        true
    }

    pub fn update_drag(&mut self, doc_point: [f32; 2], alt: bool, over_ruler: bool) {
        if let Some(drag) = self.drag.as_mut() {
            drag.doc_point = doc_point;
            drag.raw_point = doc_point;
            drag.alt = alt;
            drag.over_ruler = over_ruler;
        }
    }

    pub fn cancel_drag(&mut self) {
        self.drag = None;
        self.press_position = None;
        self.grab_offset = [0.0; 2];
    }

    pub fn finish_drag(&mut self, session: &mut Session) -> RulerDropResult {
        self.press_position = None;
        self.grab_offset = [0.0; 2];
        let drag = match self.drag.take() {
            Some(drag) => drag,
            None => return RulerDropResult::None,
        };
        match drag.target {
            RulerDragTarget::NewGuide { .. } => {
                if drag.over_ruler {
                    return RulerDropResult::None;
                }
                let kind = drag.kind(None);
                let target = dropped_surface(session, drag);
                let position = local_drop_position(session, drag, kind, target);
                let mut params = serde_json::json!({ "kind": kind, "position": position });
                match target {
                    Some(ViewTarget::Page(id)) => params["page_id"] = serde_json::json!(id),
                    Some(ViewTarget::Artboard(id)) => params["master_id"] = serde_json::json!(id),
                    None => {}
                }
                match session.run_named("guides.add", params) {
                    Ok(v) => RulerDropResult::Added(v),
                    Err(e) => RulerDropResult::Refused(error_text(&e.message_key)),
                }
            }
            RulerDragTarget::ExistingGuide { id } => {
                let original = session.guides.get(id).map(|g| g.kind);
                let kind = drag.kind(original);
                let position = local_drop_position(session, drag, kind, dropped_surface(session, drag));
                let unmoved = session.guides.get(id).is_some_and(|guide| guide.position == position);
                if !drag.over_ruler && unmoved {
                    return RulerDropResult::None;
                }
                let result = if drag.over_ruler {
                    session.run_named("guides.delete", serde_json::json!({ "id": id.0 })).map(RulerDropResult::Deleted)
                } else {
                    session
                        .run_named("guides.update", serde_json::json!({ "id": id.0, "position": position }))
                        .map(RulerDropResult::Moved)
                };
                match result {
                    Ok(r) => r,
                    Err(e) => RulerDropResult::Refused(error_text(&e.message_key)),
                }
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui, session: &mut Session, view: &ViewState, area: Rect) -> RulerDropResult {
        let t = Self::thickness();
        let top = Rect::from_min_max(Pos2::new(area.min.x + t, area.min.y), Pos2::new(area.max.x, area.min.y + t));
        let left = Rect::from_min_max(Pos2::new(area.min.x, area.min.y + t), Pos2::new(area.min.x + t, area.max.y));
        let corner = Rect::from_min_size(area.min, Vec2::splat(t));
        let canvas = Rect::from_min_max(Pos2::new(area.min.x + t, area.min.y + t), area.max);
        self.show = ruler_state(session).show;

        if self.show {
            let painter = ui.painter();
            let chrome = dtool_design::color("tab_row");
            painter.rect_filled(top, 0.0, chrome);
            painter.rect_filled(left, 0.0, chrome);
            painter.rect_filled(corner, 0.0, chrome);
            let centre = corner.center();
            let line = egui::Stroke::new(1.0, dtool_design::color("text"));
            for delta in [egui::vec2(4.0, 0.0), egui::vec2(0.0, 4.0)] {
                painter.line_segment([centre - delta, centre + delta], line);
            }
            self.paint_ticks(ui, session, view, canvas, top, left);
        }

        if self.show {
            let origin_response = ui.interact(corner, ui.id().with("dtool.ruler.origin"), Sense::click_and_drag())
                .on_hover_cursor(egui::CursorIcon::Grab);
            origin_response.context_menu(|ui| ruler_units_menu(ui, session));
            let (press, release) = crate::input::primary_button_positions(ui);
            if let Some(start) = press.filter(|point| corner.contains(*point)
                && ui.ctx().layer_id_at(*point) == Some(ui.layer_id()) && !egui::Popup::is_any_open(ui.ctx()))
            {
                self.origin_drag_start = Some(start);
            }
            if origin_response.double_clicked() {
                let _ = run_ruler(session, RULER_ORIGIN_RESET, serde_json::json!({}));
            }
            if ui.input(|input| !input.focused || input.key_pressed(egui::Key::Escape)) {
                self.origin_drag_start = None;
                self.origin_preview = None;
            } else if let Some(start) = self.origin_drag_start {
                let point = release.or_else(|| ui.input(|input| input.pointer.latest_pos()));
                self.origin_preview = point.filter(|point| point.distance(start) >= 2.0)
                    .map(|point| snap_ruler_point(session, view, screen_to_doc(view, canvas, point), None));
                if release.is_some() {
                    self.origin_drag_start = None;
                    if let Some(origin) = self.origin_preview.take().filter(|_| release.is_some_and(|point| canvas.contains(point))) {
                        let _ = run_ruler(session, RULER_ORIGIN_SET, serde_json::json!({ "origin": origin }));
                    }
                } else if self.origin_preview.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    ui.ctx().request_repaint();
                }
            }
        }
        self.interact(ui, session, view, canvas, Some((top, left, corner)))
    }

    pub fn canvas_guides(&mut self, ui: &mut Ui, session: &mut Session, view: &ViewState, canvas: Rect) -> RulerDropResult {
        self.interact(ui, session, view, canvas, None)
    }

    fn interact(&mut self, ui: &mut Ui, session: &mut Session, view: &ViewState, canvas: Rect, strips: Option<(Rect, Rect, Rect)>) -> RulerDropResult {
        let had_drag = self.drag.is_some();
        let result = self.interact_inner(ui, session, view, canvas, strips);
        let owned = had_drag || self.drag.is_some() || self.name_edit.is_some();
        let pass = ui.ctx().cumulative_pass_nr();
        ui.ctx().data_mut(|d| d.insert_temp(guide_drag_id(), (pass, owned)));
        result
    }

    fn interact_inner(&mut self, ui: &mut Ui, session: &mut Session, view: &ViewState, canvas: Rect, strips: Option<(Rect, Rect, Rect)>) -> RulerDropResult {
        let alt = ui.input(|i| i.modifiers.alt);
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if self.name_edit.as_ref().is_some_and(|edit| edit.context != session.context) {
            self.name_edit = None;
        }
        if view.canvas_flags.preview_mode {
            if let Some((top, left, _)) = strips {
                self.units_on_strips(ui, session, top, left);
            }
            self.cancel_drag();
            self.name_edit = None;
            return RulerDropResult::None;
        }
        if self.name_edit.is_some() {
            self.show_name_editor(ui, session);
            return RulerDropResult::None;
        }
        if session.guides.show && ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary))
            && !egui::Popup::is_any_open(ui.ctx())
            && !canvas_gesture_active(ui.ctx())
            && let Some(id) = pointer.filter(|pos| canvas.contains(*pos) && ui.ctx().layer_id_at(*pos) == Some(ui.layer_id()))
                .and_then(|pos| self.guide_at(ui, session, view, canvas, pos))
            && let Some(guide) = session.guides.get(id)
        {
            self.cancel_drag();
            self.name_edit = Some(GuideNameEdit {
                context: session.context,
                id,
                name: guide_name(guide),
                focus: true,
                error: None,
                chrome: Default::default(),
            });
            self.show_name_editor(ui, session);
            return RulerDropResult::None;
        }

        if self.drag.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_drag();
            return RulerDropResult::None;
        }

        if let Some((top, left, _)) = strips {
            self.units_on_strips(ui, session, top, left);
        }
        let (pressed, released) = crate::input::primary_button_positions(ui);

        if self.drag.is_none() && !session.guides.locked && !canvas_gesture_active(ui.ctx()) {
            if let Some(pos) = pressed.filter(|pos| ui.ctx().layer_id_at(*pos) == Some(ui.layer_id())
                && !egui::Popup::is_any_open(ui.ctx()))
            {
                let doc = screen_to_doc(view, canvas, pos);
                match strips {
                    Some((top, _, _)) if top.contains(pos) => self.begin_new_guide(RulerSide::Top, doc, alt),
                    Some((_, left, _)) if left.contains(pos) => self.begin_new_guide(RulerSide::Left, doc, alt),
                    _ => {
                        if session.guides.show && canvas.contains(pos) {
                            if let Some(id) = self.guide_at(ui, session, view, canvas, pos) {
                                if self.begin_move_guide(session, id, doc) && let Some(guide) = session.guides.get(id) {
                                    let axis = guide.kind.axis();
                                    let position = guide.source_segment(&session.document).map_or(guide.position, |segment| segment[0][axis]);
                                    self.grab_offset[axis] = position - doc[axis];
                                }
                            }
                        }
                    }
                }
                if self.drag.is_some() {
                    self.press_position = Some(pos);
                }
            }
        }

        if let Some(drag) = self.drag {
            if let Some(pos) = released.or(pointer) {
                let over_ruler = strips.is_some_and(|(top, left, corner)| top.contains(pos) || left.contains(pos) || corner.contains(pos));
                let mut raw = screen_to_doc(view, canvas, pos);
                for (value, offset) in raw.iter_mut().zip(self.grab_offset) { *value += offset; }
                let excluded = match drag.target {
                    RulerDragTarget::ExistingGuide { id } => Some(id),
                    RulerDragTarget::NewGuide { .. } => None,
                };
                let modifiers = ui.input(|input| {
                    input.events.iter().rev().find_map(|event| match event {
                        egui::Event::PointerButton {
                            button: egui::PointerButton::Primary, pressed: false, modifiers, ..
                        } => Some(*modifiers),
                        _ => None,
                    }).unwrap_or(input.modifiers)
                });
                let doc_point = if over_ruler {
                    raw
                } else if let Some(increment) = guide_snap_increment(modifiers) {
                    let kind = drag.kind(excluded.and_then(|id| session.guides.get(id)).map(|guide| guide.kind));
                    snap_to_ruler_units(session, raw, kind, increment)
                } else {
                    snap_ruler_point(session, view, raw, excluded)
                };
                self.update_drag(doc_point, alt, over_ruler);
                if let Some(current) = self.drag.as_mut() {
                    current.raw_point = raw;
                }
                ui.ctx().request_repaint();
            }
            if let Some(end) = released {
                if self.press_position.is_some_and(|start| start.distance(end) < 2.0)
                    || (!canvas.contains(end) && !self.drag.is_some_and(|drag| drag.over_ruler))
                {
                    self.cancel_drag();
                    return RulerDropResult::None;
                }
                return self.finish_drag(session);
            }
        }

        RulerDropResult::None
    }

    fn units_on_strips(&self, ui: &Ui, session: &mut Session, top: Rect, left: Rect) {
        for (rect, key) in [(top, "dtool.ruler.top"), (left, "dtool.ruler.left")] {
            ui.interact(rect, ui.scope_id().with(key), Sense::click_and_drag())
                .context_menu(|ui| ruler_units_menu(ui, session));
        }
    }

    fn guide_at(&self, ui: &Ui, session: &Session, view: &ViewState, canvas: Rect, pos: Pos2) -> Option<GuideId> {
        let reach = dtool_design::space("sm");
        let mut closest = None;
        for guide in session.guides.list().iter().rev() {
            if guide.kind == GuideKind::Column && !session.guides.show_columns { continue; }
            let Some(segment) = screen_guide(&session.document, view, canvas, guide) else { continue };
            if guide_label(ui, canvas, guide, segment).is_some_and(|(rect, _)| rect.contains(pos)) {
                return Some(guide.id);
            }
            let distance = distance_to_segment(pos, segment);
            if distance <= reach && closest.is_none_or(|(_, previous)| distance < previous) {
                closest = Some((guide.id, distance));
            }
        }
        closest.map(|(id, _)| id)
    }

    fn show_name_editor(&mut self, ui: &Ui, session: &mut Session) {
        use crate::dialog_chrome::{ClosePolicy, DialogMode, DialogResult};
        let Some(edit) = &mut self.name_edit else { return };
        let (dismissal, action) = edit.chrome.show(
            ui.ctx(), "ruler.guide_name", text("dialog.guides.rename"), DialogMode::Modal,
            ClosePolicy::CancelOnly, false, |ui| {
                let width = (ui.ctx().content_rect().width() - 44.0).clamp(120.0, 340.0);
                ui.set_width(width);
                let label = text("dialog.guides.name");
                ui.label(label);
                let response = ui.add(egui::TextEdit::singleline(&mut edit.name).desired_width(width));
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                if std::mem::take(&mut edit.focus) {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                        let end = egui::text::CCursor::new(edit.name.chars().count());
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::default(), end)));
                        state.store(ui.ctx(), response.id);
                    }
                }
                if response.changed() {
                    edit.error = None;
                }
                if let Some(error) = &edit.error {
                    ui.colored_label(ui.visuals().error_fg_color, text(error));
                }
                ui.separator();
                // Enter must leave the field, rather than just confirm an IME candidate.
                let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let saved = dtool_design::primary_button(ui, text("button.ok"), egui::vec2(72.0, 22.0)).clicked();
                    let cancelled = dtool_design::button(ui, text("button.cancel")).clicked();
                    if cancelled { DialogResult::Cancel }
                    else if saved || enter { DialogResult::Confirm }
                    else { DialogResult::Open }
                }).inner
            },
        );
        if dismissal == DialogResult::Cancel || action == DialogResult::Cancel {
            self.name_edit = None;
        } else if action == DialogResult::Confirm {
            if session.guides.get(edit.id).is_some_and(|guide| guide_name(guide) == edit.name.trim()) {
                self.name_edit = None;
            } else {
                match crate::guides_dialog::rename_guide(session, edit.id.0, &edit.name) {
                    Ok(()) => self.name_edit = None,
                    Err(error) => edit.error = Some(error.message_key),
                }
            }
        }
    }

    pub fn paint_line(ui: &Ui, view: &ViewState, canvas: Rect, kind: GuideKind, position: f32, colour: egui::Color32) {
        let [a, b] = guide_segment(view, canvas, kind, position);
        dtool_design::canvas_line(&ui.painter().with_clip_rect(canvas), a, b, colour);
    }

    pub(crate) fn paint_guide(ui: &Ui, doc: &dtool_core::Document, view: &ViewState, canvas: Rect, guide: &Guide, colour: egui::Color32) {
        let painter = ui.painter().with_clip_rect(canvas);
        if let Some([a, b]) = screen_guide(doc, view, canvas, guide) {
            dtool_design::canvas_line(&painter, a, b, colour);
        }
        for segment in guide.inherited_segments(doc) {
            paint_dashed_guide(&painter, view, canvas, segment, egui::Stroke::new(1.0, colour));
        }
    }

    pub fn paint_visible_guides(&self, ui: &Ui, session: &Session, view: &ViewState, canvas: Rect) {
        if view.canvas_flags.preview_mode {
            return;
        }
        paint_grid(ui, &session.document.grid, view, canvas);
        if session.guides.show {
            self.paint_guides(ui, session, view, canvas);
            paint_folds(ui, &session.document, view, canvas);
        }
        if session.view.canvas_flags.show_bleed {
            paint_bleed(ui, session, view, canvas);
        }
        self.paint_drag_preview(ui, session, view, canvas);
        if let Some(origin) = self.origin_preview {
            let point = doc_to_screen(view, canvas, origin);
            let painter = ui.painter().with_clip_rect(canvas);
            let line = egui::Stroke::new(1.0, dtool_design::color("smart_guide"));
            for delta in [egui::vec2(7.0, 0.0), egui::vec2(0.0, 7.0)] {
                painter.line_segment([point - delta, point + delta], line);
            }
        }
    }

    pub fn canvas_rect(area: Rect, shown: bool) -> Rect {
        let t = if shown { Self::thickness() } else { 0.0 };
        Rect::from_min_max(Pos2::new(area.min.x + t, area.min.y + t), area.max)
    }

    fn paint_guides(&self, ui: &Ui, session: &Session, view: &ViewState, canvas: Rect) {
        let colour = dtool_design::color("accent");
        let painter = ui.painter().with_clip_rect(canvas);
        let moving = match self.drag {
            Some(RulerDrag { target: RulerDragTarget::ExistingGuide { id }, .. }) => Some(id),
            _ => None,
        };
        for g in session.guides.list() {
            if g.kind == GuideKind::Column && !session.guides.show_columns {
                continue;
            }
            if moving == Some(g.id) {
                continue;
            }
            for segment in g.inherited_segments(&session.document) {
                paint_dashed_guide(&painter, view, canvas, segment, egui::Stroke::new(1.0, colour));
            }
            let Some([a, b]) = screen_guide(&session.document, view, canvas, g) else { continue };
            dtool_design::canvas_line(&painter, a, b, colour);
            if let Some((rect, galley)) = guide_label(ui, canvas, g, [a, b]) {
                painter.rect_filled(rect, 2.0, dtool_design::color("tab_row"));
                if galley.elided {
                    ui.interact(rect, ui.scope_id().with(("guide_label", g.id.0)), Sense::hover())
                        .on_hover_text(guide_name(g));
                }
                painter.galley(rect.min + egui::vec2(3.0, 1.0), galley, dtool_design::color("text"));
            }
        }
    }

    fn paint_drag_preview(&self, ui: &Ui, session: &Session, view: &ViewState, canvas: Rect) {
        let drag = match self.drag {
            Some(drag) => drag,
            None => return,
        };
        if drag.over_ruler {
            return;
        }
        
        let snap_doc = SnapDocument {
            document: &session.document,
            guides: &session.guides,
            grid: &session.document.grid,
        };
        let prefs = &session.snap_prefs.query();
        let moving_owner = match drag.target {
            RulerDragTarget::ExistingGuide { id } => Some(id.0),
            _ => None,
        };
        
        if ui.input(|input| guide_snap_increment(input.modifiers).is_none()) {
            let guide_colour = dtool_design::color("smart_guide");
            for guide in smart_guides(&snap_doc, view, prefs, drag.doc_point, moving_owner) {
                let kind = match guide.kind {
                    SmartGuideKind::Horizontal => GuideKind::Horizontal,
                    SmartGuideKind::Vertical => GuideKind::Vertical,
                };
                Self::paint_line(ui, view, canvas, kind, guide.position, guide_colour);
            }
        }

        let original = match drag.target {
            RulerDragTarget::ExistingGuide { id } => session.guides.get(id).map(|g| g.kind),
            RulerDragTarget::NewGuide { .. } => None,
        };
        let kind = drag.kind(original);
        let target = dropped_surface(session, drag);
        let (page_id, master_id) = match target {
            Some(ViewTarget::Page(id)) => (Some(id), None),
            Some(ViewTarget::Artboard(id)) => (None, Some(id)),
            None => (None, None),
        };
        let guide = Guide {
            id: GuideId::default(), kind,
            position: local_drop_position(session, drag, kind, target),
            name: None, locked: false, page_id, master_id,
        };
        Self::paint_guide(ui, &session.document, view, canvas, &guide, dtool_design::color("accent"));
    }

    fn paint_ticks(&self, ui: &Ui, session: &Session, view: &ViewState, canvas: Rect, top: Rect, left: Rect) {
        if !ticks_visible(view) {
            return;
        }
        let steps = [0.0001f32, 0.0002, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05,
            0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0];
        let mut ruler = ruler_state(session);
        if let Some(origin) = self.origin_preview {
            ruler.origin = origin;
        }
        let scale = ruler.display_scale(session.document.units, session.document.dpi);
        let pixels_per_unit = view.zoom / scale;
        if !pixels_per_unit.is_finite() || pixels_per_unit <= 0.0 { return; }
        let major = steps.into_iter().find(|step| step * pixels_per_unit >= 90.0)
            .unwrap_or_else(|| (90.0 / pixels_per_unit).max(10000.0));
        let step = steps.into_iter().find(|step| {
            let count = major / step;
            count >= 1.0 && count <= 20.0001 && (count - count.round()).abs() < 0.0001
                && step * pixels_per_unit >= 4.0
        }).unwrap_or(major);
        let subdivisions = (major / step).round() as i64;
        let decimals = (-major.log10().floor()).max(0.0) as usize;
        let colour = dtool_design::color("text");
        let font = egui::TextStyle::Small.resolve(ui.style());
        let tick_w = dtool_design::stroke_width("thin").max(1.0);
        for (axis, strip) in [top, left].into_iter().enumerate() {
            let painter = ui.painter().with_clip_rect(strip);
            let start = screen_to_doc(view, canvas, strip.min)[axis];
            let end = screen_to_doc(view, canvas, strip.max)[axis];
            let first = ((start - ruler.origin[axis]) * scale / step).ceil() as i64;
            let last = ((end - ruler.origin[axis]) * scale / step).floor() as i64;
            for index in (first..=last).take(2048) {
                let full = index.rem_euclid(subdivisions) == 0;
                let halfway = subdivisions % 2 == 0
                    && index.rem_euclid(subdivisions) == (subdivisions / 2);
                let length = if full { 0.50 } else if halfway { 0.34 } else { 0.20 };
                let value = index as f32 * step;
                let mut point = ruler.origin;
                point[axis] += value / scale;
                let at = doc_to_screen(view, canvas, point);
                let stroke = egui::Stroke::new(tick_w, if full { colour } else { colour.gamma_multiply(0.67) });
                let label_at = if axis == 0 {
                    painter.vline(at.x, (strip.bottom() - strip.height() * length)..=strip.bottom(), stroke);
                    Pos2::new(at.x + 2.0, strip.top())
                } else {
                    painter.hline((strip.right() - strip.width() * length)..=strip.right(), at.y, stroke);
                    Pos2::new(strip.left() + 1.0, at.y - 2.0)
                };
                if full {
                    let label = format!("{value:.decimals$}");
                    if axis == 0 {
                        painter.text(label_at, egui::Align2::LEFT_TOP, label, font.clone(), colour);
                    } else {
                        let galley = painter.layout_no_wrap(label, font.clone(), colour);
                        painter.add(egui::epaint::TextShape::new(label_at, galley, colour)
                            .with_angle(-std::f32::consts::FRAC_PI_2));
                    }
                }
            }
        }
    }
}

pub fn view_menu_guides(ui: &mut Ui, session: &mut Session) {
    let mut show = session.guides.show;
    if dtool_design::toggle(ui, &mut show, text("menu.view.show.guides")).changed() {
        let _ = session.run_named("guides.show", serde_json::json!({ "show": show }));
    }
    let mut locked = session.guides.locked;
    if dtool_design::toggle(ui, &mut locked, text("menu.view.lock_guides")).changed() {
        let _ = session.run_named("guides.show", serde_json::json!({ "locked": locked }));
    }
}

pub fn view_menu_rulers(ui: &mut Ui, session: &mut Session) {
    let rulers = ruler_state(session);
    let mut show = rulers.show;
    if dtool_design::toggle(ui, &mut show, text(MENU_SHOW_RULERS)).changed() {
        let _ = run_ruler(session, RULER_SHOW, serde_json::json!({ "show": show }));
    }
    let mut show_text_ruler = rulers.show_text_ruler;
    if dtool_design::toggle(ui, &mut show_text_ruler, text(MENU_SHOW_TEXT_RULER)).changed() {
        let _ = run_ruler(session, RULER_SHOW, serde_json::json!({ "show_text_ruler": show_text_ruler }));
    }
}

pub fn origin_controls(ui: &mut Ui, session: &mut Session) {
    let rulers = ruler_state(session);
    let units = session.document.units;
    let scale = f64::from(rulers.display_scale(units, session.document.dpi));
    let suffix = if rulers.meters { "m" } else { dtool_core::commands::ruler_units::suffix(rulers.effective_units(units)) };
    let mut origin = rulers.origin;
    ui.horizontal_wrapped(|ui| {
        for (axis, key) in ["ruler.origin.x", "ruler.origin.y"].into_iter().enumerate() {
            let value = f64::from(origin[axis]) * scale;
            let id = ui.id().with(("ruler.origin", axis));
            let spec = dtool_design::NumericSpec::new(text(key)).suffix(suffix).decimals(6).width(96.0);
            let (_, edit) = dtool_design::numeric_field(ui, id, dtool_design::NumericValue::Value(value), &spec);
            if let dtool_design::NumericEvent::Commit(value) = edit {
                origin[axis] = (value / scale) as f32;
            }
        }
        if dtool_design::button(ui, text("menu.view.ruler_origin_reset")).clicked() {
            let _ = run_ruler(session, RULER_ORIGIN_RESET, serde_json::json!({}));
        } else if origin != rulers.origin {
            let _ = run_ruler(session, RULER_ORIGIN_SET, serde_json::json!({ "origin": origin }));
        }
    });
}

pub const GRID_MIN_SCREEN_GAP: f32 = 6.0;
pub const GRID_MAX_LINES: usize = 4000;
pub const PIXEL_GRID_MIN_ZOOM: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLine {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub major: bool,
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn grid_family(out: &mut Vec<GridLine>, origin: [f32; 2], dir: [f32; 2], step: [f32; 2], corners: &[[f32; 2]; 4], zoom: f32, major: bool, skip_every: i64) {
    let len = dot(dir, dir).sqrt();
    if !(len > 0.0) || !zoom.is_finite() {
        return;
    }
    let normal = [-dir[1] / len, dir[0] / len];
    let gap = dot(step, normal);
    if !gap.is_finite() || (gap.abs() * zoom) < GRID_MIN_SCREEN_GAP {
        return;
    }
    let ts = corners.map(|c| dot([c[0] - origin[0], c[1] - origin[1]], normal) / gap);
    let lo = ts.iter().copied().fold(f32::INFINITY, f32::min).floor() as i64;
    let hi = ts.iter().copied().fold(f32::NEG_INFINITY, f32::max).ceil() as i64;
    if hi < lo || (hi - lo) as usize > GRID_MAX_LINES {
        return;
    }
    for n in lo..=hi {
        if skip_every > 1 && n.rem_euclid(skip_every) == 0 {
            continue;
        }
        let p0 = [origin[0] + n as f32 * step[0], origin[1] + n as f32 * step[1]];
        let ss = corners.map(|c| dot([c[0] - p0[0], c[1] - p0[1]], dir) / (len * len));
        let smin = ss.iter().copied().fold(f32::INFINITY, f32::min);
        let smax = ss.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        out.push(GridLine { from: [p0[0] + smin * dir[0], p0[1] + smin * dir[1]], to: [p0[0] + smax * dir[0], p0[1] + smax * dir[1]], major });
    }
}

pub fn grid_lines(grid: &dtool_core::grid::Grid, zoom: f32, corners: [[f32; 2]; 4]) -> Vec<GridLine> {
    use dtool_core::grid::GridKind;
    let mut out = Vec::new();
    if grid.show {
        let [sx, sy] = grid.spacing;
        if sx > 0.0 && sy > 0.0 && sx.is_finite() && sy.is_finite() {
            let o = grid.origin;
            let families: Vec<([f32; 2], [f32; 2])> = match grid.kind {
                GridKind::Standard => vec![([0.0, 1.0], [sx, 0.0]), ([1.0, 0.0], [0.0, sy])],
                GridKind::Isometric => vec![([1.0, 0.0], [sx * 0.5, sy]), ([sx * 0.5, sy], [sx, 0.0]), ([-sx * 0.5, sy], [sx, 0.0])],
                GridKind::Axonometric => {
                    let [a, b] = [grid.angles[0].to_radians(), grid.angles[1].to_radians()];
                    let u = [sx * a.cos(), -sx * a.sin()];
                    let v = [-sy * b.cos(), -sy * b.sin()];
                    vec![(u, v), (v, u)]
                }
                GridKind::Custom => {
                    let a = grid.angles[0].to_radians();
                    let u = [sx * a.cos(), sx * a.sin()];
                    let v = [-sy * a.sin(), sy * a.cos()];
                    vec![(u, v), (v, u)]
                }
            };
            let sub = grid.subdivisions.max(1) as i64;
            for (dir, step) in families {
                if sub > 1 {
                    let minor = [step[0] / sub as f32, step[1] / sub as f32];
                    grid_family(&mut out, o, dir, minor, &corners, zoom, false, sub);
                }
                grid_family(&mut out, o, dir, step, &corners, zoom, true, 0);
            }
        }
    }
    if grid.show_pixel && zoom >= PIXEL_GRID_MIN_ZOOM {
        grid_family(&mut out, [0.0, 0.0], [0.0, 1.0], [1.0, 0.0], &corners, zoom, false, 0);
        grid_family(&mut out, [0.0, 0.0], [1.0, 0.0], [0.0, 1.0], &corners, zoom, false, 0);
    }
    out
}

pub fn visible_doc_corners(view: &ViewState, canvas: Rect) -> [[f32; 2]; 4] {
    [canvas.left_top(), canvas.right_top(), canvas.right_bottom(), canvas.left_bottom()].map(|p| screen_to_doc(view, canvas, p))
}

pub fn paint_grid(ui: &Ui, grid: &dtool_core::grid::Grid, view: &ViewState, canvas: Rect) -> usize {
    let lines = grid_lines(grid, view.zoom, visible_doc_corners(view, canvas));
    let painter = ui.painter().with_clip_rect(canvas);
    let major = dtool_design::color("separator");
    let minor = dtool_design::color("border");
    for line in &lines {
        let colour = if line.major { major } else { minor };
        dtool_design::canvas_line(&painter, doc_to_screen(view, canvas, line.from), doc_to_screen(view, canvas, line.to), colour);
    }
    lines.len()
}

pub use dtool_core::view::{bleed_margins, DEFAULT_BLEED_MM};
pub const BLEED_COLOUR: egui::Color32 = egui::Color32::from_rgb(220, 50, 50);

pub fn bleed_rects(session: &Session) -> Vec<[f32; 4]> {
    bleed_outlines(session).map(|corners| {
        let [x0, y0, x1, y1] = corners.into_iter().fold(
            [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
            |[x0, y0, x1, y1], [x, y]| [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
        );
        [x0, y0, x1 - x0, y1 - y0]
    }).collect()
}

fn bleed_outlines(session: &Session) -> impl Iterator<Item = [[f32; 2]; 4]> + '_ {
    let visible = session.has_document && session.view.canvas_flags.show_bleed;
    dtool_core::view::bleed_outlines(&session.document)
        .take(if visible { usize::MAX } else { 0 })
        .map(|(_, corners)| corners)
}

fn paint_bleed(ui: &Ui, session: &Session, view: &ViewState, canvas: Rect) {
    let painter = ui.painter().with_clip_rect(canvas);
    for outline in bleed_outlines(session) {
        let corners = outline.map(|p| doc_to_screen(view, canvas, p));
        for i in 0..4 {
            dtool_design::canvas_line(&painter, corners[i], corners[(i + 1) % 4], BLEED_COLOUR);
        }
    }
}

pub(crate) fn paint_margins(ui: &Ui, session: &Session, view: &ViewState, canvas: Rect, colour: &str) {
    let doc = &session.document;
    let margins = doc.setup.margins;
    let flags = &view.canvas_flags;
    if !session.has_document || !flags.show_margins || flags.preview_mode || !margins.enabled {
        return;
    }
    let offsets = [margins.left, margins.top, margins.right, margins.bottom];
    if offsets.iter().any(|side| !side.is_finite() || *side < 0.0) || offsets == [0.0; 4] {
        return;
    }
    let [left, top, right, bottom] = offsets;
    let painter = ui.painter().with_clip_rect(canvas);
    let colour = egui::Color32::from_hex(colour).unwrap_or(dtool_design::color("smart_guide"));
    let draw = |[x, y, w, h]: [f32; 4], matrix: Option<[f32; 6]>| {
        if w <= left + right || h <= top + bottom {
            return;
        }
        let [x0, y0, x1, y1] = [x + left, y + top, x + w - right, y + h - bottom];
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| {
            let world = matrix.map_or(p, |m| dtool_core::geometry::apply_matrix(&m, p));
            doc_to_screen(view, canvas, world)
        });
        painter.add(egui::Shape::closed_line(corners.to_vec(), egui::Stroke::new(1.0, colour)));
    };
    for (index, rect) in dtool_core::scene::page_rects(doc).into_iter().enumerate() {
        if !dtool_core::artboard::edit::is_backing_page(doc, index) {
            draw(rect, None);
        }
    }
    for board in dtool_core::view::ViewTarget::artboards(doc) {
        draw([board.pos[0], board.pos[1], board.size[0].abs(), board.size[1].abs()],
            Some(dtool_core::geometry::object_matrix(board)));
    }
}
