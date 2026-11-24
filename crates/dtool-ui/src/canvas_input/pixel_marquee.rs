use dtool_core::selection::magnetic::{MagneticImage, MAX_ANCHORS};
use dtool_core::selection::{PixelSelection, PixelSurface, SelectionOp};
use dtool_core::{Command, LassoMode, Session};
use dtool_tools::selection_tools::{modifier_op, FloodSelectState, LassoState, SelectionBrushState, SelectionToolOptions};
use egui::{Color32, Pos2, Rect, Stroke, Ui};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Rectangle, Ellipse, Row, Column, Lasso, Flood, Brush }

impl Kind {
    pub fn from_tool(tool: &str) -> Option<Self> {
        match tool {
            "tool.marquee.rectangle" => Some(Self::Rectangle),
            "tool.marquee.ellipse" => Some(Self::Ellipse),
            "tool.marquee.row" => Some(Self::Row),
            "tool.marquee.column" => Some(Self::Column),
            "tool.marquee.freehand" => Some(Self::Lasso),
            "tool.magic_wand" => Some(Self::Flood),
            "tool.selection_brush" => Some(Self::Brush),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Gesture {
    kind: Option<Kind>,
    surface: Option<PixelSurface>,
    start: Option<[f32; 2]>,
    preview: Option<(Kind, [f32; 4])>,
    lasso: LassoState,
    magnetic: MagneticPreview,
    brush: SelectionBrushState,
    brush_outline: Option<(Pos2, f32)>,
}

#[derive(Debug, Clone, Default)]
struct MagneticPreview {
    image: Option<std::sync::Arc<MagneticImage>>,
    generation: u64,
    fixed: Vec<[f32; 2]>,
    hover: Vec<[f32; 2]>,
    target: Option<[f32; 2]>,
}

impl MagneticPreview {
    fn add_anchor(&mut self, previous: Option<[f32; 2]>, point: [f32; 2], session: &Session, surface: PixelSurface) {
        if let Some(previous) = previous {
            self.update_hover(previous, Some(point));
            self.fixed.extend(self.hover.drain(..).skip(1));
        } else {
            self.image = MagneticImage::capture_surface(&session.document, surface).map(std::sync::Arc::new);
            self.generation = session.generation;
            self.fixed.push(self.image.as_ref().map_or(point, |image| image.snap(point)));
        }
        self.target = None;
    }

    fn update_hover(&mut self, previous: [f32; 2], target: Option<[f32; 2]>) {
        if target == self.target { return; }
        self.target = target;
        self.hover.clear();
        if let Some(point) = target {
            self.hover = self.image.as_ref().map_or_else(
                || vec![previous, point],
                |image| image.trace(previous, point),
            );
        }
    }
}

impl Gesture {
    pub fn active(&self) -> bool { self.start.is_some() || !self.lasso.points.is_empty() || !self.brush.points.is_empty() }

    pub fn cancel(&mut self) { *self = Self::default(); }

    pub fn input(
        &mut self, ui: &Ui, response: &egui::Response, session: &Session, kind: Kind,
        to_doc: impl Fn(Pos2) -> [f32; 2],
    ) -> Option<Command> {
        if self.kind != Some(kind) {
            self.cancel();
            self.kind = Some(kind);
        }
        let options = SelectionToolOptions::from_store(&session.tool_options, &session.active_tool);
        let (down, up) = crate::input::primary_button_positions(ui);
        let target = |at| PixelSurface::at_point(&session.document, to_doc(at));
        if !self.active() && let Some(at) = down.filter(|at| can_start(ui, response, *at)) {
            self.surface = target(at);
        }
        let surface = self.surface.or_else(|| up.or_else(|| response.hover_pos()).and_then(target))?;
        let to_pixels = |at| surface.doc_to_pixel(to_doc(at));
        if response.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair); }
        if kind == Kind::Lasso {
            return self.lasso_input(ui, response, session, options, surface, to_pixels);
        }
        let [width, height] = surface.size.map(|side| side as f32);
        let in_page = |[x, y]: [f32; 2]| x >= 0.0 && x < width && y >= 0.0 && y < height;
        if kind == Kind::Flood {
            let up = up?;
            self.surface = None;
            let [x, y] = to_pixels(up);
            if !response.clicked() || !can_start(ui, response, up) || !in_page([x, y]) {
                return None;
            }
            response.request_focus();
            let mut wand = FloodSelectState { options };
            let mods = release_modifiers(ui);
            wand.options.op = modifier_op(mods.shift, mods.alt).unwrap_or(wand.options.op);
            let mut command = wand.click(x as u32, y as u32);
            command.params["page_id"] = json!(surface.page_id);
            return Some(command);
        }
        if kind == Kind::Brush {
            self.brush.options = options;
            self.brush.size_px = session.tool_options.number(&session.active_tool, "brush.width").unwrap_or(64.0) as f32;
            self.brush.hardness = if self.brush.options.soft_edges { 0.5 } else { 1.0 };
            let radius = (self.brush.size_px * session.view.zoom / surface.scale * 0.5).max(2.0);
            self.brush_outline = response.hover_pos().map(|point| (point, radius));
            if self.brush_outline.is_some() { ui.ctx().set_cursor_icon(egui::CursorIcon::None); }
            let result = self.brush_input(ui, response, to_pixels, in_page).map(|mut command| {
                command.params["page_id"] = json!(surface.page_id);
                command
            });
            if self.brush.points.is_empty() { self.surface = None; }
            return result;
        }
        if let Some(at) = down.filter(|at| can_start(ui, response, *at) && in_page(to_pixels(*at)))
        {
            self.start = Some(to_pixels(at));
            response.request_focus();
        }
        let start = self.start?;
        let latest = up.or_else(|| ui.input(|input| input.pointer.latest_pos()));
        let Some(end) = latest.map(to_pixels) else { self.cancel(); return None };
        let extent = |axis: usize, fixed: f32| {
            let delta: f32 = end[axis] - start[axis];
            let signed = if fixed > 0.0 { fixed.copysign(delta) } else { delta };
            if options.from_center {
                let half = if fixed > 0.0 { fixed * 0.5 } else { delta.abs() };
                (start[axis] - half, 2.0 * half)
            } else {
                (start[axis].min(start[axis] + signed), signed.abs())
            }
        };
        let [x, y, w, h] = match kind {
            Kind::Row => [0.0, end[1].floor(), width, 1.0],
            Kind::Column => [end[0].floor(), 0.0, 1.0, height],
            _ => {
                let (x, w) = extent(0, options.width);
                let (y, h) = extent(1, options.height);
                [x, y, w, h]
            }
        };
        if up.is_none() {
            if ui.input(|input| input.pointer.primary_down()) {
                self.preview = Some((kind, [x, y, w, h]));
                ui.ctx().request_repaint();
            } else {
                self.cancel();
            }
            return None;
        }
        self.cancel();
        if x >= width || y >= height || x + w <= 0.0 || y + h <= 0.0 { return None; }
        let mods = release_modifiers(ui);
        let op = modifier_op(mods.shift, mods.alt).unwrap_or(options.op);
        if w < 0.5 || h < 0.5 {
            return (op == SelectionOp::New).then(|| Command::new("selection.none"));
        }
        Some(Command::with_params("selection.rect", json!({
            "page_id": surface.page_id,
            "shape": if kind == Kind::Ellipse { "ellipse" } else { "rectangle" },
            "rect": {"x":x, "y":y, "w":w, "h":h},
            "op": op,
            "antialias": options.antialias,
            "feather": options.feather,
        })))
    }

    fn brush_input(
        &mut self, ui: &Ui, response: &egui::Response, to_pixels: impl Fn(Pos2) -> [f32; 2],
        in_page: impl Fn([f32; 2]) -> bool,
    ) -> Option<Command> {
        let one_pixel = self.brush.size_px == 1.0;
        let to_brush = |position| {
            let point = to_pixels(position);
            if one_pixel { point.map(|coordinate| coordinate.floor() + 0.5) } else { point }
        };
        for event in ui.input(|input| input.events.clone()) {
            match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                    if can_start(ui, response, pos) && in_page(to_pixels(pos)) =>
                {
                    let [x, y] = to_brush(pos);
                    self.brush.pointer_down(x, y, 1.0);
                    response.request_focus();
                }
                egui::Event::PointerMoved(pos)
                    if self.brush.points.len() < dtool_core::commands::pixel_brush::MAX_POINTS - 1 =>
                {
                    let [x, y] = to_brush(pos);
                    self.brush.pointer_move(x, y, 1.0);
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers }
                    if !self.brush.points.is_empty() =>
                {
                    let [x, y] = to_brush(pos);
                    self.brush.pointer_move(x, y, 1.0);
                    if modifiers.alt { self.brush.options.op = SelectionOp::Subtract; }
                    return self.brush.pointer_up();
                }
                egui::Event::PointerGone => self.brush.points.clear(),
                _ => {}
            }
        }
        if !ui.input(|input| input.pointer.primary_down()) { self.brush.points.clear(); }
        None
    }

    fn lasso_input(
        &mut self, ui: &Ui, response: &egui::Response, session: &Session,
        options: SelectionToolOptions, surface: PixelSurface, to_pixels: impl Fn(Pos2) -> [f32; 2],
    ) -> Option<Command> {
        const MAX_POINTS: usize = 1_000_000;
        let mode = options.lasso_mode;
        let click_anchors = mode != LassoMode::Freehand;
        let magnetic = mode == LassoMode::Magnetic;
        if self.lasso.mode != mode || (magnetic && self.active() && self.magnetic.generation != session.generation) {
            self.lasso.cancel();
            self.magnetic = MagneticPreview::default();
            self.lasso.mode = mode;
        }
        let max_points = if magnetic { MAX_ANCHORS } else { MAX_POINTS };
        self.lasso.antialias = options.antialias;
        let finish = |command: Option<Command>| command.map(|mut command| {
            command.params["feather"] = json!(options.feather);
            command.params["page_id"] = json!(surface.page_id);
            command
        });
        let in_page = |at| {
            let [x, y] = to_pixels(at);
            x >= 0.0 && y >= 0.0 && x < surface.size[0] as f32 && y < surface.size[1] as f32
        };
        let use_modifiers = |lasso: &mut LassoState, mods: egui::Modifiers| {
            lasso.op = modifier_op(mods.shift, mods.alt).unwrap_or(options.op);
        };
        for event in ui.input(|input| input.events.clone()) {
            match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                    if !click_anchors && can_start(ui, response, pos) && in_page(pos) =>
                {
                    self.lasso.pointer_down(to_pixels(pos));
                    response.request_focus();
                }
                egui::Event::PointerMoved(pos) if !click_anchors && self.lasso.points.len() < MAX_POINTS - 1 => {
                    self.lasso.pointer_move(to_pixels(pos));
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers }
                    if !click_anchors && !self.lasso.points.is_empty() =>
                {
                    use_modifiers(&mut self.lasso, modifiers);
                    let command = self.lasso.pointer_up(to_pixels(pos));
                    self.lasso.cancel();
                    return finish(command);
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers }
                    if click_anchors && response.clicked() && can_start(ui, response, pos)
                        && (self.active() || in_page(pos)) =>
                {
                    response.request_focus();
                    use_modifiers(&mut self.lasso, modifiers);
                    let closes = self.near_lasso_start(session, response.rect, surface, pos);
                    if closes || self.lasso.points.len() == max_points {
                        self.magnetic = MagneticPreview::default();
                        return finish(self.lasso.finish());
                    }
                    let point = to_pixels(pos);
                    if response.double_clicked()
                        && (self.lasso.points.len() >= 3
                            || (self.lasso.points.len() == 2 && self.lasso.points.last() != Some(&point)))
                    {
                        self.magnetic = MagneticPreview::default();
                        return finish(self.lasso.double_click(point));
                    }
                    if magnetic && self.lasso.points.last() != Some(&point) {
                        self.magnetic.add_anchor(self.lasso.points.last().copied(), point, session, surface);
                    }
                    self.lasso.pointer_down(point);
                }
                egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. }
                    if click_anchors && response.has_focus() && self.lasso.points.len() >= 3 =>
                {
                    use_modifiers(&mut self.lasso, modifiers);
                    self.magnetic = MagneticPreview::default();
                    return finish(self.lasso.enter());
                }
                egui::Event::PointerGone if !click_anchors => self.lasso.cancel(),
                _ => {}
            }
        }
        if magnetic && let Some(&previous) = self.lasso.points.last() {
            let target = response.hover_pos().map(|pos| {
                if self.near_lasso_start(session, response.rect, surface, pos) { self.lasso.points[0] } else { to_pixels(pos) }
            });
            self.magnetic.update_hover(previous, target);
        }
        if !click_anchors && !ui.input(|input| input.pointer.primary_down()) {
            self.lasso.cancel();
        }
        None
    }

    fn near_lasso_start(&self, session: &Session, canvas: Rect, surface: PixelSurface, at: Pos2) -> bool {
        self.lasso.points.len() >= 3 && {
            let start = self.magnetic.fixed.first().unwrap_or(&self.lasso.points[0]);
            at.distance(pixel_to_screen(session, canvas, surface, *start)) <= 8.0
        }
    }

    pub fn paint(&self, ui: &Ui, canvas: Rect, session: &Session) {
        let surface = self.surface.or_else(|| PixelSurface::for_page(&session.document, None));
        let Some(surface) = surface else { return; };
        let to_screen = |point| pixel_to_screen(session, canvas, surface, point);
        if self.kind == Some(Kind::Brush) {
            let painter = ui.painter_at(canvas);
            let width = self.brush.size_px * session.view.zoom / pixels_per_unit(session);
            let colour = Color32::from_rgba_unmultiplied(104, 185, 250, 70);
            let points: Vec<_> = self.brush.points.iter().map(|[x, y, _]| to_screen([*x, *y])).collect();
            if let [only] = points.as_slice() {
                painter.circle_filled(*only, width * 0.5, colour);
            } else if points.len() > 1 {
                painter.add(egui::Shape::line(points, Stroke::new(width, colour)));
            }
            if let Some((point, radius)) = self.brush_outline {
                painter.circle_stroke(point, radius, Stroke::new(2.0, Color32::from_black_alpha(180)));
                painter.circle_stroke(point, radius, Stroke::new(1.0, Color32::WHITE));
            }
            return;
        }
        if let Some(first) = self.lasso.points.first() {
            let painter = ui.painter_at(canvas);
            let magnetic = self.lasso.mode == LassoMode::Magnetic;
            let mut points: Vec<_> = if magnetic {
                self.magnetic.fixed.iter().copied().chain(self.magnetic.hover.iter().copied().skip(1)).map(to_screen).collect()
            } else {
                self.lasso.points.iter().copied().map(to_screen).collect()
            };
            if magnetic {
                if let Some(&start) = points.first() {
                    painter.circle_stroke(start, 4.0, Stroke::new(1.25, Color32::WHITE));
                }
            } else if self.lasso.mode == LassoMode::Polygonal {
                points.extend(ui.input(|input| input.pointer.hover_pos()).filter(|p| canvas.contains(*p)));
                painter.circle_stroke(to_screen(*first), 4.0, Stroke::new(1.25, Color32::WHITE));
            }
            if points.len() > 1 { dashed(&painter, &points); }
            return;
        }
        let Some((kind, [x, y, w, h])) = self.preview else { return };
        let mut points: Vec<_> = if kind == Kind::Ellipse {
            (0..64).map(|i| {
                let angle = i as f32 * std::f32::consts::TAU / 64.0;
                to_screen([x + w * (1.0 + angle.cos()) * 0.5, y + h * (1.0 + angle.sin()) * 0.5])
            }).collect()
        } else {
            [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].into_iter().map(to_screen).collect()
        };
        points.push(points[0]);
        dashed(&ui.painter_at(canvas), &points);
    }
}

fn can_start(ui: &Ui, response: &egui::Response, at: Pos2) -> bool {
    response.enabled() && response.interact_rect.contains(at)
        && ui.ctx().layer_id_at(at) == Some(response.layer_id)
        && !egui::Popup::is_any_open(ui.ctx())
}

fn release_modifiers(ui: &Ui) -> egui::Modifiers {
    ui.input(|input| input.events.iter().rev().find_map(|event| match event {
        egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, modifiers, .. } => Some(*modifiers),
        _ => None,
    }).unwrap_or(input.modifiers))
}

fn pixels_per_unit(session: &Session) -> f32 {
    dtool_core::units::to_px(1.0, session.document.units, session.document.dpi).unwrap_or(1.0)
}

fn pixel_to_screen(session: &Session, canvas: Rect, surface: PixelSurface, point: [f32; 2]) -> Pos2 {
    super::screen(canvas, &session.view, surface.pixel_to_doc(point))
}

fn dashed(painter: &egui::Painter, points: &[Pos2]) {
    painter.add(egui::Shape::line(points.to_vec(), Stroke::new(1.25, Color32::WHITE)));
    painter.extend(egui::Shape::dashed_line(points, Stroke::new(1.25, Color32::BLACK), 4.0, 4.0));
}

type MaskTextureKey = (
    Option<dtool_core::workspace::ContextId>, u64, u64, dtool_core::commands::quickmask::Display, dtool_core::document::PageId, Option<u64>,
);

#[derive(Default)]
pub(crate) struct QuickMaskOverlay {
    texture: Option<(MaskTextureKey, egui::TextureHandle)>,
}

impl QuickMaskOverlay {
    pub(crate) fn paint(&mut self, painter: &egui::Painter, canvas: Rect, session: &Session, draft: Option<&super::PixelBrushDraft>) {
        use dtool_core::commands::quickmask::Display;

        if !session.quick_mask.active || session.view.canvas_flags.preview_mode {
            self.texture = None;
            return;
        }
        let doc = &session.document;
        let mask = draft.map_or(doc, |preview| preview.document.as_ref()).pixel_selection.as_ref();
        let surface = match mask {
            Some(mask) => PixelSurface::for_page(doc, mask.page_id),
            None => {
                let viewport = [canvas.width(), canvas.height()];
                PixelSurface::at_view_center(doc, &session.view, viewport).or_else(|| PixelSurface::for_page(doc, None))
            }
        };
        let Some(surface) = surface else { return };
        let [width, height] = surface.size;
        let display = session.quick_mask.display;
        let key = (session.context, session.generation, session.document.version, display, surface.page_id, draft.map(|preview| preview.revision));
        let texture = match &self.texture {
            Some((last, texture)) if *last == key => texture.id(),
            _ => {
                let limit = 2048;
                let longer = u64::from(width.max(height));
                let size = if mask.is_none() && display != Display::Transparent {
                    [1, 1]
                } else if longer <= limit {
                    [width as usize, height as usize]
                } else {
                    [width, height].map(|v| (u64::from(v) * limit / longer).max(1) as usize)
                };
                let mut pixels = Vec::with_capacity(size[0] * size[1]);
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        let src_x = (x as u64 * u64::from(width) / size[0] as u64) as u32;
                        let src_y = (y as u64 * u64::from(height) / size[1] as u64) as u32;
                        let a = 255 - mask.map_or(0, |mask| mask.get(src_x, src_y));
                        let [r, g, b, a] = match display {
                            Display::Overlay => [255, 0, 0, a / 2],
                            Display::Black => [0, 0, 0, a],
                            Display::White => [255, 255, 255, a],
                            Display::Transparent => {
                                let shade = if (x / 8 + y / 8) % 2 == 0 { 168 } else { 208 };
                                [shade, shade, shade, a]
                            }
                        };
                        pixels.push(Color32::from_rgba_unmultiplied(r, g, b, a));
                    }
                }
                let texture = painter.ctx().load_texture(
                    "quick-mask", egui::ColorImage::new(size, pixels), egui::TextureOptions::LINEAR,
                );
                let id = texture.id();
                self.texture = Some((key, texture));
                id
            }
        };
        let [w, h] = [width as f32, height as f32];
        let mut mesh = egui::Mesh::with_texture(texture);
        for (point, uv) in [
            ([0.0, 0.0], [0.0, 0.0]), ([w, 0.0], [1.0, 0.0]),
            ([w, h], [1.0, 1.0]), ([0.0, h], [0.0, 1.0]),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: pixel_to_screen(session, canvas, surface, point),
                uv: egui::pos2(uv[0], uv[1]),
                color: Color32::WHITE,
            });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }
}

#[derive(Default)]
pub(crate) struct Outline {
    key: Option<(Option<dtool_core::workspace::ContextId>, u64, u64)>,
    segments: Vec<[[f32; 2]; 2]>,
}

impl Outline {
    pub(crate) fn paint(&mut self, painter: &egui::Painter, canvas: Rect, session: &Session) {
        if session.quick_mask.active || session.view.canvas_flags.preview_mode || !session.view.canvas_flags.show_pixel_selection { return; }
        let Some(selection) = &session.document.pixel_selection else {
            self.key = None;
            self.segments.clear();
            return;
        };
        let Some(surface) = PixelSurface::for_page(&session.document, selection.page_id) else { return; };
        let key = (session.context, session.generation, session.document.version);
        if self.key != Some(key) {
            self.key = Some(key);
            self.segments.clear();
            self.trace(selection);
        }
        for segment in &self.segments {
            let screen = segment.map(|point| pixel_to_screen(session, canvas, surface, point));
            if canvas.intersects(Rect::from_two_pos(screen[0], screen[1]).expand(1.0)) {
                dashed(painter, &screen);
            }
        }
    }

    fn trace(&mut self, selection: &PixelSelection) {
        let Some([x, y, w, h]) = selection.bounds() else { return };
        for (vertical, across, along) in [(false, y..=y + h, x..x + w), (true, x..=x + w, y..y + h)] {
            let point = |a: u32, b: u32| if vertical { [a, b] } else { [b, a] };
            for a in across {
                let mut start = None;
                for b in along.clone() {
                    let [x, y] = point(a, b);
                    let ahead = selection.get(x, y) >= 128;
                    let behind = a.checked_sub(1).is_some_and(|a| {
                        let [x, y] = point(a, b);
                        selection.get(x, y) >= 128
                    });
                    if ahead != behind {
                        start.get_or_insert(b);
                    } else if let Some(first) = start.take() {
                        self.segments.push([point(a, first).map(|v| v as f32), point(a, b).map(|v| v as f32)]);
                    }
                }
                if let Some(first) = start {
                    self.segments.push([point(a, first).map(|v| v as f32), point(a, along.end).map(|v| v as f32)]);
                }
            }
        }
    }
}
