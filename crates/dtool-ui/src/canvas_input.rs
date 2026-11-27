use dtool_core::commands::picture_frame::FrameShape;
use dtool_core::command::{Session, TransformSelectionBoxArgs};
use dtool_core::object::ObjectId;
use dtool_core::paint::GradientKind;
use dtool_core::scene::Scene;
use dtool_core::scene_hit::{hit_test_in_view, selection_box, selection_frame, HIT_TOLERANCE_PX};
use dtool_core::selection::{selection_marquee_with_flags, MarqueeMode, SelectOp};
use dtool_core::shape::ShapeKind;
use dtool_core::view_state::ViewState;
use dtool_protocol::Command;
use dtool_tools::move_tool::{GestureKind, Handle, MoveTool, MoveToolAction, PointerEvent};
use dtool_tools::shape_tool::{self, ShapeParamKind};
use egui::{Pos2, Rect, Response, Stroke, Ui};
use serde_json::json;
use crate::input::primary_button_positions;

pub(crate) const SELECTION_BLUE: egui::Color32 = egui::Color32::from_rgb(90, 166, 251);

mod area;
mod artboard;
mod blend;
mod curve_energy;
mod gradient_edit;
pub(crate) use gradient_edit::Gradient as GradientPreview;
mod measure;
mod pixel_brush;
pub(crate) use pixel_brush::Draft as PixelBrushDraft;
pub mod pixel_marquee;
mod point_transform;
mod shape_builder;
mod snap_feedback;
mod stroke_width;
mod vector_crop;
pub(crate) mod warp_mesh;

pub const HANDLE_PX: f32 = 6.0;
pub const ROTATE_PX: f32 = 24.0;
pub const MIN_DRAG_DOC: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavMode {
    Pan,
    Zoom,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolKind {
    Move,
    Shape(ShapeKind),
    PictureFrame(FrameShape),
    Eyedropper,
    ArtText,
    FrameText,
    Table,
    Crop,
    VectorCrop,
    Slice,
    Gradient(Option<GradientKind>),
    Transparency,
    Line,
    Pen { polyline: bool },
    Pencil,
    VectorBrush,
    PixelBrush(dtool_core::brush::BrushOp),
    PixelFlood { erase: bool },
    RedEye,
    PixelMarquee(pixel_marquee::Kind),
    StrokeWidth,
    Knife,
    ShapeBuilder,
    PointTransform,
    StylePicker,
    Measure,
    Area,
    Blend,
    Node,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drag {
    Shape { kind: ShapeKind, start: [f32; 2], current: [f32; 2] },
    ArtText { start: [f32; 2], current: [f32; 2] },
    Crop { target: ObjectId, start: [f32; 2], current: [f32; 2] },
    Slice { start: [f32; 2], current: [f32; 2], edit: Option<SliceDrag> },
    Gradient { kind: Option<GradientKind>, transparency: bool, target: ObjectId, start: [f32; 2], current: [f32; 2] },
    MaskGradient { kind: GradientKind, start: [f32; 2], current: [f32; 2] },
    Line { start: [f32; 2], current: [f32; 2] },
    RedEye { target: ObjectId, start: [f32; 2], current: [f32; 2] },
    ShapeParam { target: ObjectId, kind: ShapeParamKind, start: [f32; 2], current: [f32; 2], grab_offset: [f32; 2] },
    PictureFrame { shape: FrameShape, start: [f32; 2], current: [f32; 2] },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliceDrag {
    id: dtool_core::slice::SliceId,
    bounds: [f32; 4],
    corner: Option<usize>,
}

impl SliceDrag {
    fn bounds_at(self, start: [f32; 2], end: [f32; 2]) -> [f32; 4] {
        let [x, y, right, bottom] = self.bounds;
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let min = 0.01;
        match self.corner {
            Some(0) => [(x + dx).min(right - min), (y + dy).min(bottom - min), right, bottom],
            Some(1) => [x, (y + dy).min(bottom - min), (right + dx).max(x + min), bottom],
            Some(2) => [x, y, (right + dx).max(x + min), (bottom + dy).max(y + min)],
            Some(_) => [(x + dx).min(right - min), y, right, (bottom + dy).max(y + min)],
            None => [x + dx, y + dy, right + dx, bottom + dy],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GestureOwner {
    pub context: Option<u64>,
    pub session: usize,
    pub generation: u64,
    pub version: u64,
    pub tool: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerCheck {
    Same,
    ToolChanged,
    Lost,
}

#[derive(Debug, Clone)]
struct KnifeGesture {
    freehand: bool,
    targets: Vec<ObjectId>,
    points: Vec<[f32; 2]>,
}

impl KnifeGesture {
    fn add_point(&mut self, point: [f32; 2], min_step: f32) {
        if !self.freehand {
            self.points.truncate(1);
        }
        let last = self.points[self.points.len() - 1];
        if (point[0] - last[0]).hypot(point[1] - last[1]) > min_step {
            self.points.push(point);
        }
    }

    fn command(self, min_drag: f32, click: bool, hit_radius: f32) -> Option<Command> {
        let [x, y] = self.points[0];
        let dragged = self.points[1..].iter().any(|p| (p[0] - x).hypot(p[1] - y) > min_drag);
        if click && !dragged {
            return Some(Command::with_params(dtool_core::boolean::PATH_KNIFE, json!({
                "point": self.points[0], "ids": self.targets, "hit_radius": hit_radius,
            })));
        }
        dragged.then(|| Command {
            name: dtool_core::boolean::PATH_KNIFE.into(),
            params: json!({ "cut": self.points, "ids": self.targets }),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct CanvasInput {
    pub move_tool: MoveTool,
    pub drag: Option<Drag>,
    pub marquee: Option<[f32; 4]>,
    pub preview: Option<TransformSelectionBoxArgs>,
    pub last_error: Option<String>,
    pub preselected: bool,
    pub press_at: Option<[f32; 2]>,
    move_press: Option<Pos2>,
    pub start_box: Option<[f32; 4]>,
    pub snap_mark: Option<[f32; 2]>,
    pub scope: Option<ObjectId>,
    pub pen: crate::canvas_curves::Pen,
    pen_press: Option<Pos2>,
    pub nodes: crate::canvas_curves::NodeEdit,
    pub pencil: crate::canvas_curves::Pencil,
    knife: Option<KnifeGesture>,
    shape_builder: shape_builder::ShapeBuilder,
    point_transform: point_transform::PointTransform,
    style_picker: dtool_tools::style_picker::StylePickerTool,
    style_drag: Option<[f32; 2]>,
    measure: measure::Measure,
    area: area::Area,
    blend: blend::Blend,
    gradient_edit: gradient_edit::GradientEditor,
    pixel_brush: pixel_brush::Gesture,
    pixel_marquee: pixel_marquee::Gesture,
    stroke_width: stroke_width::WidthTool,
    vector_crop: vector_crop::Crop,
    pub(crate) warp_mesh: warp_mesh::WarpMeshEditor,
    pub hover: Option<[f32; 2]>,
    pub nudge_prefs: Option<[f32; 2]>,
    pub guide_lines: Vec<crate::canvas_tools::GuideLine>,
    gap_guides: Vec<crate::canvas_tools::GapGuide>,
    snap_axes: [Option<f32>; 2],
    pub error_key: Option<String>,
    pub context: Option<u64>,
    pub owner: Option<GestureOwner>,
    pub last_point: Option<[f32; 2]>,
    shape_press: Option<Pos2>,
    artboard_drag: Option<artboard::ArtboardDrag>,
    art_selection: Option<ObjectId>,
    art_draft: Option<ArtTextDraft>,
    origin_drag: Option<OriginDrag>,
}

#[derive(Debug, Clone)]
struct OriginDrag {
    offset: [f32; 2],
    point: [f32; 2],
    ids: Vec<ObjectId>,
}

#[derive(Debug, Clone)]
struct ArtTextDraft {
    context: Option<u64>,
    object: ObjectId,
    story: dtool_core::StoryId,
    generation: u64,
    previous_version: u64,
    previous_selection: dtool_core::Selection,
    previous_steps: usize,
    previous_history: Option<dtool_core::undo::UndoStack>,
}

fn canonical(active: &str) -> String {
    dtool_core::context_toolbar::active_id(active)
}

pub fn nav_mode(active_tool: &str, space: bool, middle: bool) -> NavMode {
    let tool = canonical(active_tool);
    if space || middle || tool == "tool.view" {
        NavMode::Pan
    } else if tool == "tool.zoom" {
        NavMode::Zoom
    } else {
        NavMode::Input
    }
}

fn kind_is_pen(active_tool: &str) -> bool {
    matches!(tool_kind(active_tool), ToolKind::Pen { .. })
}

pub fn tool_kind(active_tool: &str) -> ToolKind {
    let tool = canonical(active_tool);
    if dtool_tools::frame_text::FrameTextTool::is_active(&tool) || dtool_tools::frame_text::FrameTextTool::is_active(active_tool) {
        return ToolKind::FrameText;
    }
    if let Some(op) = dtool_core::commands::pixel_brush::operation(&tool) {
        return ToolKind::PixelBrush(op);
    }
    if let Some(kind) = pixel_marquee::Kind::from_tool(&tool) {
        return ToolKind::PixelMarquee(kind);
    }
    match tool.as_str() {
        "" | "move" | "tool.move" | "tool.select.object" | "tool.transform" | "tool.transform.scale" | "tool.transform.rotate" => ToolKind::Move,
        "tool.text.artistic" | "tool.text" | "tool.art_text" => ToolKind::ArtText,
        "tool.table" => ToolKind::Table,
        dtool_core::commands::red_eye::TOOL => ToolKind::RedEye,
        "tool.crop" => ToolKind::Crop,
        "tool.flood_fill" => ToolKind::PixelFlood { erase: false },
        "tool.eraser.flood" => ToolKind::PixelFlood { erase: true },
        "tool.vector_crop" => ToolKind::VectorCrop,
        "tool.slice" => ToolKind::Slice,
        "tool.transparency" => ToolKind::Transparency,
        "tool.node" => ToolKind::Node,
        "tool.knife" => ToolKind::Knife,
        "tool.shape_builder" => ToolKind::ShapeBuilder,
        "tool.point_transform" => ToolKind::PointTransform,
        "tool.style_picker" => ToolKind::StylePicker,
        "tool.measure" => ToolKind::Measure,
        "tool.area" => ToolKind::Area,
        "tool.blend" => ToolKind::Blend,
        "tool.paint.vector_brush" => ToolKind::VectorBrush,
        "tool.stroke_width" => ToolKind::StrokeWidth,
        "tool.qr" | "tool.artboard" => ToolKind::Shape(ShapeKind::Rectangle),
        "tool.shape.line" => ToolKind::Line,
        "tool.picture_frame" | "tool.picture_frame.rect" => ToolKind::PictureFrame(FrameShape::Rect),
        "tool.picture_frame.ellipse" => ToolKind::PictureFrame(FrameShape::Ellipse),
        "tool.eyedropper" => ToolKind::Eyedropper,
        t if t == "tool.pen" || t.starts_with("tool.pen.") => match (active_tool, t) {
            ("tool.pen.polyline", _) | (_, "tool.pen.polyline") => ToolKind::Pen { polyline: true },
            ("tool.pen.pencil", _) | (_, "tool.pen.pencil") => ToolKind::Pencil,
            (_, "tool.pen.smart" | "tool.pen.spline" | "tool.pen.b_spline") => ToolKind::Pen { polyline: false },
            (a, _) if a.starts_with("tool.pen.") && a != "tool.pen.pen" => ToolKind::Other,
            (_, "tool.pen" | "tool.pen.pen") => ToolKind::Pen { polyline: false },
            _ => ToolKind::Other,
        },
        t if t == "tool.gradient" || t.starts_with("tool.gradient.") || active_tool.starts_with("tool.gradient") => ToolKind::Gradient(crate::canvas_tools::gradient_kind(active_tool)),
        _ => tool
            .strip_prefix("tool.shape.")
            .and_then(|key| serde_json::from_value::<ShapeKind>(json!(key)).ok())
            .map(ToolKind::Shape)
            .unwrap_or(ToolKind::Other),
    }
}

const CURSOR_EDGE: egui::Color32 = egui::Color32::from_gray(92);
const CURSOR_FACE: egui::Color32 = egui::Color32::WHITE;

pub(crate) fn paint_canvas_cursor(
    ui: &Ui, response: &Response, active_tool: Option<&str>, icons: &mut crate::icon_cache::IconCache,
) {
    use egui::{Color32, CursorIcon};

    let area = response.interact_rect;
    let Some(point) = response.hover_pos().or_else(|| {
        response.dragged().then(|| response.interact_pointer_pos()).flatten()
            .filter(|point| area.contains(*point))
    }) else { return };
    if egui::Popup::is_any_open(ui.ctx()) {
        return;
    }
    let cursor = ui.ctx().output(|output| output.cursor_icon);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, response.id.with("tool_cursor")))
        .with_clip_rect(area);
    let ink = CURSOR_EDGE;
    let white = CURSOR_FACE;
    let tool = active_tool.and_then(dtool_core::context_toolbar::canonical_tool);
    let kind = tool.map(tool_kind);
    let p = |x, y| point + egui::vec2(x, y);
    let mut lines = Vec::new();
    let add_arrow = |lines: &mut Vec<_>, axis: egui::Vec2| {
        let normal = axis.rot90();
        lines.push([point - axis * 7.0, point + axis * 7.0]);
        for sign in [-1.0, 1.0] {
            let tip = point + axis * (7.0 * sign);
            let base = point + axis * (4.0 * sign);
            lines.push([tip, base + normal * 3.0]);
            lines.push([tip, base - normal * 3.0]);
        }
    };
    match cursor {
        CursorIcon::Default | CursorIcon::Copy => {
            let arrow = [point, p(11.0, 9.0), p(6.5, 9.0), p(9.4, 14.2),
                p(6.3, 15.8), p(3.5, 10.7), p(0.0, 13.7)];
            let mut fill = egui::Mesh::default();
            for vertex in arrow {
                fill.colored_vertex(vertex, white);
            }
            for [a, b, c] in [[0, 1, 2], [0, 2, 6], [2, 5, 6], [2, 3, 4], [2, 4, 5]] {
                fill.add_triangle(a, b, c);
            }
            painter.add(egui::Shape::mesh(fill));
            painter.add(egui::Shape::closed_line(arrow.to_vec(), Stroke::new(1.0, ink)));
            if cursor == CursorIcon::Copy {
                lines.extend([[p(10.0, 13.0), p(16.0, 13.0)], [p(13.0, 10.0), p(13.0, 16.0)]]);
            }
        }
        CursorIcon::Crosshair => lines.extend([
            [p(-6.5, 0.0), p(-3.0, 0.0)], [p(3.0, 0.0), p(6.5, 0.0)],
            [p(0.0, -6.5), p(0.0, -3.0)], [p(0.0, 3.0), p(0.0, 6.5)],
        ]),
        CursorIcon::PointingHand => {
            let palm = vec![p(-5.0, 6.0), p(-2.0, 4.0), p(4.0, 5.0), p(7.0, 8.0),
                p(6.0, 13.0), p(3.0, 16.0), p(-1.0, 16.0), p(-5.0, 10.0)];
            painter.add(egui::Shape::convex_polygon(palm, white, Stroke::new(1.1, ink)));
            painter.line_segment([p(0.0, 1.0), p(0.0, 8.0)], Stroke::new(4.0, ink));
            painter.line_segment([p(0.0, 1.0), p(0.0, 8.0)], Stroke::new(2.5, white));
            ui.ctx().set_cursor_icon(CursorIcon::None);
        }
        CursorIcon::Text | CursorIcon::VerticalText => {
            let offset = |x, y| if cursor == CursorIcon::Text { p(x, y) } else { p(y, x) };
            lines.extend([
                [offset(0.0, -7.0), offset(0.0, 7.0)],
                [offset(-3.0, -7.0), offset(3.0, -7.0)],
                [offset(-3.0, 7.0), offset(3.0, 7.0)],
            ]);
        }
        CursorIcon::Move => {
            add_arrow(&mut lines, egui::Vec2::X);
            add_arrow(&mut lines, egui::Vec2::Y);
        }
        CursorIcon::ResizeHorizontal | CursorIcon::ResizeColumn => add_arrow(&mut lines, egui::Vec2::X),
        CursorIcon::ResizeVertical | CursorIcon::ResizeRow => add_arrow(&mut lines, egui::Vec2::Y),
        CursorIcon::ResizeNwSe => add_arrow(&mut lines, egui::vec2(1.0, 1.0).normalized()),
        CursorIcon::ResizeNeSw => add_arrow(&mut lines, egui::vec2(1.0, -1.0).normalized()),
        CursorIcon::ZoomIn | CursorIcon::ZoomOut => {
            for stroke in [Stroke::new(3.5, ink), Stroke::new(2.3, white)] {
                painter.circle_stroke(point, 5.0, stroke);
                painter.line_segment([p(3.5, 3.5), p(8.0, 8.0)], stroke);
            }
            lines.push([p(-2.5, 0.0), p(2.5, 0.0)]);
            if cursor == CursorIcon::ZoomIn {
                lines.push([p(0.0, -2.5), p(0.0, 2.5)]);
            }
        }
        CursorIcon::Alias => {
            if active_tool.is_some_and(|tool| tool_kind(tool) == ToolKind::FrameText) {
                for stroke in [Stroke::new(3.3, ink), Stroke::new(2.1, white)] {
                    painter.circle_stroke(p(-3.0, 2.0), 3.5, stroke);
                    painter.circle_stroke(p(3.0, -2.0), 3.5, stroke);
                    painter.line_segment([p(-2.0, 1.5), p(2.0, -1.5)], stroke);
                }
            } else {
                let arc = [p(-6.0, 4.0), p(-7.0, -1.0), p(-4.0, -6.0), p(1.0, -7.0), p(5.0, -5.0), p(7.0, -1.0)];
                lines.extend(arc.windows(2).map(|ends| [ends[0], ends[1]]));
                lines.extend([[p(7.0, -1.0), p(8.0, -5.0)], [p(7.0, -1.0), p(3.0, -2.0)]]);
            }
            ui.ctx().set_cursor_icon(CursorIcon::None);
        }
        CursorIcon::Grab | CursorIcon::Grabbing => {
            let silhouette = if cursor == CursorIcon::Grab {
                vec![p(-7.0, -2.0), p(-5.0, -4.0), p(-3.0, -2.0), p(-3.0, -7.0),
                    p(5.0, -7.0), p(7.0, -3.0), p(6.0, 4.0), p(3.0, 8.0), p(-3.0, 8.0)]
            } else {
                vec![p(-6.0, -2.0), p(-4.0, -5.0), p(4.0, -5.0), p(7.0, -2.0),
                    p(6.0, 4.0), p(3.0, 8.0), p(-3.0, 8.0), p(-6.0, 4.0)]
            };
            // The open hand has a thumb notch. Fill the palm and thumb separately.
            let palm = if cursor == CursorIcon::Grab { silhouette[2..].to_vec() } else { silhouette.clone() };
            painter.add(egui::Shape::convex_polygon(palm, white, Stroke::NONE));
            if cursor == CursorIcon::Grab {
                painter.add(egui::Shape::convex_polygon(
                    vec![silhouette[0], silhouette[1], silhouette[2], silhouette[8]], white, Stroke::NONE,
                ));
                lines.extend([[-1.0, -7.0], [2.0, -7.0]].map(|[x, y]| [p(x, y), p(x, -2.0)]));
            } else {
                lines.push([p(-3.0, 0.0), p(3.0, 0.0)]);
            }
            painter.add(egui::Shape::closed_line(silhouette, Stroke::new(1.2, ink)));
            for line in lines.drain(..) {
                painter.line_segment(line, Stroke::new(1.0, ink));
            }
            ui.ctx().set_cursor_icon(CursorIcon::None);
        }
        _ => {}
    }
    for stroke in [Stroke::new(3.4, ink), Stroke::new(2.4, white)] {
        for &line in &lines {
            painter.line_segment(line, stroke);
        }
    }
    if matches!(cursor, CursorIcon::Default | CursorIcon::Copy) || !lines.is_empty() {
        ui.ctx().set_cursor_icon(CursorIcon::None);
    }
    let Some(tool) = tool else { return };
    if matches!(tool, "tool.move" | "tool.select.object" | "tool.stroke_width") {
        return;
    }
    // Keep the brush outline visible alongside the glyph that identifies the tool.
    let brush = tool == "tool.selection_brush"
        || dtool_core::commands::pixel_brush::operation(tool).is_some();
    let offset = match cursor {
        CursorIcon::Default => egui::vec2(11.0, 12.0),
        CursorIcon::Crosshair => egui::vec2(6.0, 6.0),
        CursorIcon::Text | CursorIcon::VerticalText
            if matches!(tool_kind(tool), ToolKind::ArtText | ToolKind::FrameText) => egui::vec2(6.0, 6.0),
        CursorIcon::None if brush => egui::vec2(10.0, 10.0),
        _ => return,
    };
    let size = egui::Vec2::splat(14.0);
    if area.width() < size.x + 2.0 || area.height() < size.y + 2.0 {
        return;
    }
    let mut origin = point + offset;
    if origin.x + size.x > area.right() { origin.x = point.x - size.x - 8.0; }
    if origin.y + size.y > area.bottom() { origin.y = point.y - size.y - 8.0; }
    let margin = egui::Vec2::splat(1.0);
    let origin = origin.clamp(area.min + margin, area.max - size - margin);
    let glyph = Rect::from_min_size(origin, size);
    let at = |x, y| origin + egui::vec2(x, y);
    // Solid silhouettes leave room for a white face at cursor scale.
    let silhouette: &[(f32, f32)] = match kind {
        Some(ToolKind::Pen { .. }) => &[(1.5, 12.5), (3.0, 5.0), (8.5, 1.0), (13.0, 5.5), (9.0, 11.0)],
        Some(ToolKind::Pencil) => &[(1.5, 12.5), (2.5, 8.5), (9.5, 1.5), (12.5, 4.5), (5.5, 11.5)],
        Some(ToolKind::Eyedropper) => &[(1.5, 12.5), (2.0, 9.0), (9.5, 1.5), (12.5, 4.5), (5.0, 12.0)],
        _ => &[],
    };
    if !silhouette.is_empty() {
        painter.add(egui::Shape::convex_polygon(
            silhouette.iter().map(|&(x, y)| at(x, y)).collect(), white, Stroke::new(1.0, ink),
        ));
        if matches!(kind, Some(ToolKind::Pen { .. })) {
            painter.line_segment([at(1.5, 12.5), at(6.0, 8.0)], Stroke::new(0.9, ink));
            painter.circle_filled(at(6.0, 8.0), 1.15, ink);
        } else {
            let seam = if kind == Some(ToolKind::Pencil) { [at(7.7, 3.3), at(10.7, 6.3)] }
                else { [at(6.5, 3.7), at(10.3, 7.5)] };
            painter.line_segment(seam, Stroke::new(1.0, ink));
        }
        return;
    }
    let symbols: &[&[(f32, f32)]] = match kind {
        Some(ToolKind::Node) => &[
            &[(1.5, 11.5), (7.0, 7.0), (12.5, 2.5)],
        ],
        Some(ToolKind::VectorCrop | ToolKind::Crop) => &[
            &[(3.0, 1.0), (3.0, 8.0), (11.0, 8.0)],
            &[(1.0, 3.0), (8.0, 3.0), (8.0, 11.0)],
        ],
        _ => &[],
    };
    if !symbols.is_empty() {
        for stroke in [Stroke::new(3.2, ink), Stroke::new(2.2, white)] {
            for points in symbols {
                painter.add(egui::Shape::line(points.iter().map(|&(x, y)| at(x, y)).collect(), stroke));
            }
        }
        if kind == Some(ToolKind::Node) {
            painter.rect(
                Rect::from_center_size(at(7.0, 7.0), egui::Vec2::splat(5.0)),
                0.5, white, Stroke::new(1.0, ink), egui::StrokeKind::Outside,
            );
        }
        return;
    }
    let texture_px = dtool_design::icon_texture_px(ui);
    let Some(outline) = icons.tool_texture(ui.ctx(), tool, ink, texture_px) else { return };
    let Some(icon) = icons.tool_texture(ui.ctx(), tool, white, texture_px) else { return };
    let uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    // Use the tool's own silhouette as a light glyph with a narrow dark outline.
    for direction in [egui::Vec2::X, -egui::Vec2::X, egui::Vec2::Y, -egui::Vec2::Y] {
        painter.image(outline.id(), glyph.translate(direction * 0.45), uv, Color32::WHITE);
    }
    painter.image(icon.id(), glyph, uv, Color32::WHITE);
}

pub fn normalize_rect_params(command: &mut Command) {
    let get = |v: &serde_json::Value| -> Option<[f32; 2]> { Some([v.get(0)?.as_f64()? as f32, v.get(1)?.as_f64()? as f32]) };
    let (Some(mut pos), Some(mut size)) = (get(&command.params["pos"]), get(&command.params["size"])) else { return };
    for i in 0..2 {
        if size[i] < 0.0 {
            pos[i] += size[i];
            size[i] = -size[i];
        }
    }
    command.params["pos"] = json!(pos);
    command.params["size"] = json!(size);
}

fn editable_shape_params(session: &Session) -> Option<&dtool_core::Object> {
    let object = shape_tool::selected_parametric_shape(session)?;
    let shape = object.shape.as_ref()?.kind;
    match tool_kind(&session.active_tool) {
        // These shapes already resize through the Move tool's corner handles.
        ToolKind::Move if !move_selection_box_visible(session) || matches!(shape, ShapeKind::Rectangle | ShapeKind::Ellipse) => None,
        ToolKind::Move => Some(object),
        ToolKind::Shape(tool) if (tool == shape || shape_tool::is_p0(tool) && shape_tool::is_p0(shape))
            && !matches!(session.active_tool.as_str(), "tool.qr" | "tool.artboard") => Some(object),
        _ => None,
    }
}

fn move_tool_option(session: &Session, option: &str, default: bool) -> bool {
    session.tool_options.flag(&session.active_tool, option).unwrap_or(default)
}

pub(crate) fn move_selection_box_visible(session: &Session) -> bool {
    move_tool_option(session, dtool_core::context_toolbar::MOVE_SHOW_SELECTION_BOX, true)
}

fn shape_param_drag(session: &Session, point: [f32; 2], radius: f32) -> Option<Drag> {
    let object = editable_shape_params(session)?;
    let kind = shape_tool::hit_param_handle(object, point, radius)?;
    let (_, at) = shape_tool::param_handles(object).into_iter().find(|(candidate, _)| *candidate == kind)?;
    Some(Drag::ShapeParam {
        target: object.id, kind, start: point, current: point,
        grab_offset: [at[0] - point[0], at[1] - point[1]],
    })
}

pub fn shape_command(kind: ShapeKind, start: [f32; 2], end: [f32; 2], mods: Mods) -> Option<Command> {
    if (end[0] - start[0]).abs() < MIN_DRAG_DOC || (end[1] - start[1]).abs() < MIN_DRAG_DOC {
        return None;
    }
    let mut command = dtool_tools::shape_tool::ShapeTool { kind }.create_with_modifiers(start, end, mods.shift, mods.ctrl);
    normalize_rect_params(&mut command);
    Some(command)
}

fn configured_shape_command(
    session: &Session, kind: ShapeKind, start: [f32; 2], end: [f32; 2], mut mods: Mods,
) -> Option<Command> {
    use dtool_core::context_toolbar::{SHAPE_CORNER_RADIUS, SHAPE_FROM_CENTER, SHAPE_SIDES};
    let options = &session.tool_options;
    let tool = &session.active_tool;
    mods.ctrl |= options.flag(tool, SHAPE_FROM_CENTER).unwrap_or(false);
    mods.shift |= tool == "tool.qr";
    let mut command = shape_command(kind, start, end, mods)?;
    match kind {
        ShapeKind::RoundedRectangle => {
            if let Some(radius) = options.number(tool, SHAPE_CORNER_RADIUS) {
                let radii = [radius; 4];
                command.params["corner"]["radii"] = json!(radii);
            }
        }
        ShapeKind::Polygon => {
            if let Some(sides) = options.number(tool, SHAPE_SIDES) {
                command.params["polygon"]["number_of_sides"] = json!(sides.round() as u32);
            }
        }
        _ => {}
    }
    Some(command)
}

pub fn picture_frame_command(shape: FrameShape, start: [f32; 2], end: [f32; 2], mods: Mods) -> Option<Command> {
    if (end[0] - start[0]).abs() < MIN_DRAG_DOC || (end[1] - start[1]).abs() < MIN_DRAG_DOC {
        return None;
    }
    let mut size = [end[0] - start[0], end[1] - start[1]];
    if mods.shift {
        let side = size[0].abs().max(size[1].abs());
        size = [side.copysign(size[0]), side.copysign(size[1])];
    }
    let pos = if mods.ctrl { [start[0] - size[0], start[1] - size[1]] } else { start };
    if mods.ctrl {
        size = [size[0] * 2.0, size[1] * 2.0];
    }
    let name = match shape {
        FrameShape::Rect => "rect",
        FrameShape::Ellipse => "ellipse",
    };
    let mut command = Command::with_params(dtool_core::commands::picture_frame::FRAME_IMAGE_ADD, json!({ "shape": name, "pos": pos, "size": size }));
    normalize_rect_params(&mut command);
    Some(command)
}

fn art_text_command(start: [f32; 2], end: [f32; 2], units_per_point: f32) -> Command {
    let tool = dtool_tools::art_text::ArtTextTool::new();
    if (end[1] - start[1]).abs() >= units_per_point {
        tool.drag_in_document(start, end, units_per_point)
    } else {
        tool.click(start)
    }
}

fn new_art_text_is_vertical(session: &Session) -> bool {
    session.tool_options.flag(&session.active_tool, dtool_core::context_toolbar::ART_TEXT_VERTICAL) == Some(true)
}

fn art_text_path_at(scene: &Scene, session: &Session, point: [f32; 2]) -> Option<ObjectId> {
    use dtool_core::scene::Prim;
    use dtool_core::scene_hit::{near_polyline, prim_object};

    let reach = 5.0 / session.view.zoom.max(1e-4);
    scene.prims.iter().rev().find_map(|prim| {
        use dtool_core::object::ObjectKind;
        let object = session.document.get_object(prim_object(prim)?)?;
        if !matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Compound(_)) || object.locked || !object.pickable { return None; }
        let hit = match prim {
            Prim::Stroke { points, closed, width, .. } => near_polyline(points, *closed, point, reach.max(width * 0.5)),
            Prim::Fill { contours, .. } => contours.iter().any(|points| near_polyline(points, true, point, reach)),
            _ => false,
        };
        (hit && dtool_core::outline::object_outline(object).is_some()).then_some(object.id)
    })
}

fn paint_art_text_path_cursor(ui: &Ui, rect: Rect, at: Pos2) {
    let painter = ui.painter_at(rect);
    for (width, colour) in [(3.3, CURSOR_EDGE), (2.2, CURSOR_FACE)] {
        let stroke = Stroke::new(width, colour);
        for (a, b) in [((-3.0, -8.0), (3.0, -8.0)), ((0.0, -8.0), (0.0, 8.0)), ((-3.0, 8.0), (3.0, 8.0))] {
            painter.line_segment([at + egui::vec2(a.0, a.1), at + egui::vec2(b.0, b.1)], stroke);
        }
        let curve = [(4.0, 7.0), (6.0, 4.0), (9.0, 4.0), (12.0, 7.0), (15.0, 7.0)];
        painter.add(egui::Shape::line(curve.map(|(x, y)| at + egui::vec2(x, y)).to_vec(), stroke));
    }
}

pub fn click_select_command(hit: Option<ObjectId>, selected: &[ObjectId], shift: bool) -> Option<Command> {
    let (ids, op) = match (hit, shift) {
        (Some(id), true) if selected.contains(&id) => (vec![id.0], "Subtract"),
        (Some(id), true) => (vec![id.0], "Add"),
        (Some(id), false) => (vec![id.0], "Replace"),
        (None, true) => return None,
        (None, false) if selected.is_empty() => return None,
        (None, false) => (Vec::new(), "Replace"),
    };
    Some(Command::with_params("selection.set", json!({ "ids": ids, "op": op })))
}

pub fn move_action_command(action: &MoveToolAction) -> Option<Command> {
    match action {
        MoveToolAction::MarqueeCommit { rect, mode, op } => Some(Command::with_params("selection.marquee", json!({ "rect": rect, "mode": mode, "op": op }))),
        MoveToolAction::TransformCommit { args } => Some(Command::with_params("transform.selection_box", serde_json::to_value(args).ok()?)),
        MoveToolAction::DuplicateCommit { offset } => Some(Command::with_params(dtool_tools::move_tool::ALT_DRAG_DUPLICATE_COMMAND, json!({ "offset": offset }))),
        _ => None,
    }
}

pub fn handle_cursor(handle: Handle) -> egui::CursorIcon {
    match handle {
        Handle::TopLeft | Handle::BottomRight | Handle::FrameTextScale => egui::CursorIcon::ResizeNwSe,
        Handle::TopRight | Handle::BottomLeft => egui::CursorIcon::ResizeNeSw,
        Handle::Top | Handle::Bottom => egui::CursorIcon::ResizeVertical,
        Handle::Left | Handle::Right => egui::CursorIcon::ResizeHorizontal,
        Handle::Rotate => egui::CursorIcon::Alias,
        Handle::ShearTop | Handle::ShearBottom => egui::CursorIcon::ResizeColumn,
    }
}

fn slice_hit(session: &Session, rect: Rect, pointer: Pos2, [x, y]: [f32; 2]) -> Option<SliceDrag> {
    let slices = &session.document.slices;
    if let Some(slice) = slices.iter().find(|slice| session.active_slice == Some(slice.id))
        && let Ok(bounds @ [left, top, right, bottom]) = slice.resolved_bounds(&session.document)
    {
        let corners = [[left, top], [right, top], [right, bottom], [left, bottom]];
        if let Some(corner) = corners.iter().position(|&point| screen(rect, &session.view, point).distance(pointer) <= 8.0) {
            return Some(SliceDrag { id: slice.id, bounds, corner: Some(corner) });
        }
    }
    slices.iter().rev().find_map(|slice| {
        let bounds @ [left, top, right, bottom] = slice.resolved_bounds(&session.document).ok()?;
        (x >= left && x <= right && y >= top && y <= bottom)
            .then_some(SliceDrag { id: slice.id, bounds, corner: None })
    })
}

fn screen(rect: Rect, view: &ViewState, point: [f32; 2]) -> Pos2 {
    let v = dtool_core::doc_to_view(view, point);
    egui::pos2(rect.min.x + v[0], rect.min.y + v[1])
}

pub fn zoom_at(view: &mut ViewState, point: [f32; 2], factor: f32) {
    let anchor = dtool_core::view_to_doc(view, point);
    view.zoom = dtool_core::clamp_zoom(view.zoom * factor);
    let mapped = dtool_core::doc_to_view(view, anchor);
    view.scroll[0] += point[0] - mapped[0];
    view.scroll[1] += point[1] - mapped[1];
}

impl CanvasInput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn nudge_distances(&mut self) -> [f32; 2] {
        *self.nudge_prefs.get_or_insert_with(crate::canvas_tools::nudge_distances)
    }

    pub fn take_error_key(&mut self) -> Option<String> {
        self.error_key.take()
    }

    pub(crate) fn pixel_brush_draft(&self, session: &Session) -> Option<PixelBrushDraft> {
        self.pixel_brush.draft(session)
    }

    pub fn busy(&self) -> bool {
        self.origin_drag.is_some() || self.artboard_drag.is_some() || self.shape_press.is_some() || self.drag.is_some() || self.move_tool.dragging
            || self.nodes.grab.is_some() || self.nodes.marquee.is_some() || self.pencil.active()
            || self.knife.is_some() || self.shape_builder.active() || self.point_transform.active()
            || self.style_drag.is_some() || self.measure.active() || self.area.active() || self.vector_crop.active()
            || self.blend.active() || self.gradient_edit.active() || self.warp_mesh.active() || self.stroke_width.active()
            || self.pixel_brush.active()
            || self.pixel_marquee.active()
    }

    pub fn cancel(&mut self) {
        self.move_tool.escape();
        self.origin_drag = None;
        self.drag = None;
        self.shape_press = None;
        self.move_press = None;
        self.artboard_drag = None;
        self.art_selection = None;
        self.marquee = None;
        self.preview = None;
        self.preselected = false;
        self.snap_mark = None;
        self.guide_lines.clear();
        self.gap_guides.clear();
        self.snap_axes = [None; 2];
        self.nodes.cancel();
        self.pen.release();
        self.pen_press = None;
        self.pencil.cancel();
        self.knife = None;
        self.shape_builder.cancel();
        self.point_transform.cancel();
        self.style_drag = None;
        self.measure.clear();
        self.area.clear();
        self.vector_crop.cancel();
        self.blend.cancel();
        self.gradient_edit.cancel();
        self.warp_mesh.cancel();
        self.stroke_width.cancel();
        self.pixel_brush.cancel();
        self.pixel_marquee.cancel();
    }

    pub fn close_pen(&mut self, session: &mut Session) -> bool {
        if !self.pen.can_close() {
            return false;
        }
        let command = self.pen.finish_command(true);
        self.run(session, command)
    }

    pub fn bind_context(&mut self, context: Option<u64>) {
        if self.context != context {
            self.clear_style_picker();
            self.measure.clear();
            self.area.clear();
            self.vector_crop.cancel();
            self.blend.clear();
            self.gradient_edit = Default::default();
            self.warp_mesh = Default::default();
        }
        self.context = context;
    }

    pub fn style_picker_loaded(&self) -> bool {
        self.style_picker.has_style()
    }

    pub fn show_measure_bar(&mut self, ui: &mut Ui, session: &Session) -> Option<Command> {
        if session.active_tool == "tool.area" { self.area.bar(ui, session) } else { self.measure.bar(ui, session) }
    }

    pub fn show_blend_bar(&mut self, ui: &mut Ui, session: &Session) -> Option<Command> {
        self.blend.bar(ui, session)
    }

    pub fn show_gradient_controls(&mut self, ui: &mut Ui, session: &Session) -> Option<Command> {
        self.gradient_edit.toolbar(ui, session)
    }

    pub(crate) fn gradient_preview(&self, session: &Session) -> Option<&GradientPreview> {
        self.gradient_edit.preview(session)
    }

    pub fn clear_style_picker(&mut self) {
        self.style_picker.clear();
        if self.style_drag.take().is_some() {
            self.marquee = None;
        }
    }

    pub fn gesture_active(&self) -> bool {
        self.busy() || self.pen.active()
    }

    fn owner_of(&self, session: &Session) -> GestureOwner {
        GestureOwner {
            context: self.context,
            session: session as *const Session as usize,
            generation: session.generation,
            version: session.document.version,
            tool: session.active_tool.clone(),
        }
    }

    pub fn claim(&mut self, session: &Session) {
        self.owner = Some(self.owner_of(session));
    }

    pub fn owner_check(&self, session: &Session) -> OwnerCheck {
        let Some(owner) = &self.owner else { return OwnerCheck::Same };
        let current = self.owner_of(session);
        if owner.context != current.context || owner.session != current.session || owner.generation != current.generation || owner.version != current.version {
            return OwnerCheck::Lost;
        }
        if owner.tool != current.tool {
            return OwnerCheck::ToolChanged;
        }
        OwnerCheck::Same
    }

    pub fn abandon(&mut self) {
        self.cancel();
        self.pen = crate::canvas_curves::Pen::default();
        self.owner = None;
        self.last_point = None;
    }

    pub fn commit_gesture(&mut self, session: &mut Session, scene: &Scene) -> bool {
        let mods = Mods { shift: false, ctrl: false, alt: false };
        let mut changed = false;
        if self.art_selection.take().is_some() {
            self.drag = None;
        }
        if let Some(drag) = self.drag {
            let end = match drag {
                Drag::Shape { current, .. } | Drag::ArtText { current, .. } | Drag::Crop { current, .. } | Drag::Gradient { current, .. } | Drag::MaskGradient { current, .. } | Drag::Line { current, .. } | Drag::ShapeParam { current, .. } | Drag::PictureFrame { current, .. } | Drag::Slice { current, .. } | Drag::RedEye { current, .. } => current,
            };
            changed |= self.finish_drag(session, end, mods);
        }
        if self.move_tool.dragging {
            if let Some(point) = self.last_point {
                self.release_move(scene, session, point, mods);
                changed = true;
            }
        }
        if self.nodes.grab.is_some() {
            let command = self.nodes.release();
            changed |= self.run(session, command);
        }
        if self.pencil.active() {
            let px = 1.0 / session.view.zoom.max(1e-4);
            let command = self.pencil.finish_command(1.5 * px, HANDLE_PX * px);
            changed |= self.run(session, command);
        }
        if self.pen.active() {
            let command = self.pen.finish_command(false);
            changed |= self.run(session, command);
        }
        self.cancel();
        self.owner = None;
        self.last_point = None;
        changed
    }

    pub fn resolve_owner(&mut self, session: &mut Session, scene: &Scene) -> bool {
        match self.owner_check(session) {
            OwnerCheck::Same => false,
            OwnerCheck::Lost => {
                self.abandon();
                true
            }
            OwnerCheck::ToolChanged => {
                self.commit_gesture(session, scene);
                true
            }
        }
    }

    pub fn snap_drag(&mut self, session: &Session, scene: &Scene, point: [f32; 2], mods: Mods) -> [f32; 2] {
        self.snap_mark = None;
        self.snap_axes = [None; 2];
        use crate::canvas_tools::{axis_snaps, box_guide_offset, gap_snap, guides_enabled};
        use dtool_core::snap_candidates::CandidateKind;
        let prefs = session.snap_prefs.query();
        let tolerance = prefs.screen_tolerance as f32 / session.view.zoom.max(1e-4);
        let exclude = dtool_core::command::transform_target_ids(&session.document, &session.selection.ids);
        let (Some(press), Some(b)) = (self.press_at, self.start_box) else { return point };
        let delta = [point[0] - press[0], point[1] - press[1]];
        let snapped = match self.move_tool.gesture {
            Some(GestureKind::Move) => {
                let delta = if mods.shift {
                    if delta[0].abs() >= delta[1].abs() { [delta[0], 0.0] } else { [0.0, delta[1]] }
                } else { delta };
                let bounds = [b[0] + delta[0], b[1] + delta[1], b[2], b[3]];
                let axes = axis_snaps(session, &crate::canvas_tools::box_points(b, delta), &exclude);
                let gaps = prefs.enabled && prefs.object_snapping && prefs.snap_to_gaps_and_sizes;
                let boxes = if guides_enabled(&prefs) || gaps {
                    crate::canvas_tools::object_boxes(session, scene, &exclude)
                } else { Vec::new() };
                let guides = box_guide_offset(&boxes, bounds, prefs.include_bbox_midpoints, tolerance);
                let correction = std::array::from_fn(|axis| {
                    if mods.shift && delta[axis] == 0.0 { return 0.0; }
                    let snap = axes[axis];
                    let guide = prefs.snap_to_object_bounds.then_some(guides[axis]).flatten();
                    let edge = match (guide, snap) {
                        (Some(g), Some(s)) if s.delta.abs() < g.abs() => Some(s.delta),
                        (Some(g), _) => Some(g),
                        (None, s) => s.map(|s| s.delta),
                    };
                    if gaps && let Some(gap) = gap_snap(&boxes, bounds, axis, tolerance)
                        && edge.is_none_or(|d| gap.delta.abs() <= d.abs() || snap.is_some_and(|s| s.kind == CandidateKind::Pixel && d == s.delta))
                    {
                        return gap.delta;
                    }
                    if prefs.dynamic_guides && let Some(s) = snap.filter(|s| Some(s.delta) == edge && s.kind != CandidateKind::Pixel) {
                        self.snap_axes[axis] = Some(s.point[axis]);
                    }
                    edge.unwrap_or(0.0)
                });
                (correction != [0.0; 2]).then_some((correction, [bounds[0] + correction[0], bounds[1] + correction[1]]))
            }
            Some(GestureKind::Scale(handle)) if !self.move_tool.transform_separately => {
                let f = crate::canvas_tools::handle_fraction(handle);
                let frame = self.move_tool.selection_frame;
                let grip = frame.grip(f);
                let current = [grip[0] + delta[0], grip[1] + delta[1]];
                let local_grip = frame.to_local(grip);
                let local = frame.to_local(current);
                let projected = frame.to_world(std::array::from_fn(|i| if f[i] == 0.5 { local_grip[i] } else { local[i] }));
                let mut axes = axis_snaps(session, &[projected], &exclude);
                let mut offset = axes.map(|snap| snap.map_or(0.0, |snap| snap.delta));
                if prefs.enabled && prefs.object_snapping && prefs.snap_to_gaps_and_sizes
                    && frame.axes == [0.0; 2] && !mods.alt && !mods.shift
                    && self.move_tool.transform_origin.is_none()
                    && !(self.move_tool.proportional_text_corners && f.iter().all(|&edge| edge != 0.5))
                    && handle != Handle::FrameTextScale && !mods.ctrl
                {
                    let boxes = crate::canvas_tools::object_boxes(session, scene, &exclude);
                    for axis in 0..2 {
                        if f[axis] == 0.5 { continue; }
                        let fixed = frame.bounds[axis] + frame.bounds[axis + 2] * (1.0 - f[axis]);
                        let delta = projected[axis] - fixed;
                        if let Some(other) = crate::canvas_tools::matched_size(&boxes, frame.bounds, axis, delta.abs(), tolerance) {
                            let size_offset = other[axis + 2].copysign(delta) - delta;
                            if axes[axis].is_none_or(|snap| size_offset.abs() < snap.delta.abs() || snap.kind == CandidateKind::Pixel) {
                                axes[axis] = None;
                                offset[axis] = size_offset;
                            }
                        }
                    }
                }
                let base = frame.to_local(projected);
                let snapped = frame.to_local([projected[0] + offset[0], projected[1] + offset[1]]);
                let target = frame.to_world(std::array::from_fn(|i| if f[i] == 0.5 { base[i] } else { snapped[i] }));
                if prefs.dynamic_guides && !mods.shift {
                    self.snap_axes = std::array::from_fn(|axis| axes[axis]
                        .filter(|snap| snap.kind != CandidateKind::Pixel
                            && (target[axis] - snap.point[axis]).abs() <= crate::canvas_tools::GUIDE_EPSILON)
                        .map(|snap| snap.point[axis]));
                }
                let correction = [target[0] - projected[0], target[1] - projected[1]];
                (offset != [0.0; 2] && correction != [0.0; 2]).then_some((correction, target))
            }
            Some(GestureKind::Rotate) if !mods.shift && session.snap_prefs.query().enabled => {
                let origin = self.move_tool.rotation_origin(mods.alt);
                return match crate::canvas_tools::snap_rotation(origin, press, point) {
                    Some(p) => {
                        self.snap_mark = Some(p);
                        p
                    }
                    None => point,
                };
            }
            _ => None,
        };
        match snapped {
            Some((offset, mark)) => {
                self.snap_mark = Some(mark);
                [point[0] + offset[0], point[1] + offset[1]]
            }
            None => point,
        }
    }

    pub fn update_guides(&mut self, session: &Session, scene: &Scene, point: [f32; 2]) {
        use crate::canvas_tools::{box_alignment_guides, gap_guides, guides_enabled, object_boxes, matched_size, size_guides, GUIDE_EPSILON};
        self.gap_guides.clear();
        let prefs = session.snap_prefs.query();
        if !self.move_tool.dragging || !prefs.enabled || !prefs.dynamic_guides || !prefs.object_snapping {
            self.guide_lines.clear();
            return;
        }
        self.guide_lines = match (self.move_tool.gesture, self.press_at, self.start_box) {
            (Some(GestureKind::Move), Some(press), Some(b)) => {
                let translate = if self.move_tool.duplicate_on_drag {
                    [point[0] - press[0], point[1] - press[1]]
                } else {
                    self.preview.as_ref().map_or([point[0] - press[0], point[1] - press[1]], |args| args.translate)
                };
                let rect = [b[0] + translate[0], b[1] + translate[1], b[2], b[3]];
                let exclude = dtool_core::command::transform_target_ids(&session.document, &session.selection.ids);
                let boxes = object_boxes(session, scene, &exclude);
                self.gap_guides = gap_guides(&boxes, rect, prefs.snap_to_gaps_and_sizes);
                if guides_enabled(&prefs) { box_alignment_guides(&boxes, rect, prefs.include_bbox_midpoints) } else { Vec::new() }
            }
            (Some(GestureKind::Scale(_)), _, Some(b)) if prefs.snap_to_gaps_and_sizes
                && !self.move_tool.transform_separately && self.move_tool.selection_frame.axes == [0.0; 2] =>
            {
                if let Some(args) = &self.preview {
                    let matrix = dtool_core::command::selection_box_matrix(args);
                    let min = dtool_core::geometry::apply_matrix(&matrix, [b[0], b[1]]);
                    let max = dtool_core::geometry::apply_matrix(&matrix, [b[0] + b[2], b[1] + b[3]]);
                    let rect = [min[0].min(max[0]), min[1].min(max[1]), (max[0] - min[0]).abs(), (max[1] - min[1]).abs()];
                    let exclude = dtool_core::command::transform_target_ids(&session.document, &session.selection.ids);
                    let boxes = object_boxes(session, scene, &exclude);
                    for axis in 0..2 {
                        if (args.scale[axis] - 1.0).abs() > GUIDE_EPSILON
                            && let Some(other) = matched_size(&boxes, rect, axis, rect[axis + 2], GUIDE_EPSILON)
                        {
                            self.gap_guides.extend(size_guides(rect, other, axis));
                        }
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        };
    }

    pub fn snap_free(&mut self, session: &Session, point: [f32; 2]) -> [f32; 2] {
        let (snapped, mark) = crate::canvas_tools::snap_point(session, point, &[]);
        self.snap_mark = mark;
        snapped
    }

    fn sync(&mut self, scene: &Scene, session: &Session) {
        let zoom = session.view.zoom.max(1e-4);
        self.move_tool.handle_radius = HANDLE_PX / zoom;
        self.move_tool.rotate_offset = ROTATE_PX / zoom;
        self.move_tool.show_handles = move_selection_box_visible(session);
        self.move_tool.transform_separately = session.selection.ids.len() > 1
            && move_tool_option(session, dtool_core::context_toolbar::MOVE_TRANSFORM_SEPARATELY, false);
        self.move_tool.transform_origin = session.selection_transform_origin();
        let text_kind = match session.selection.ids.as_slice() {
            [id] => session.document.get_object(*id).map(|object| &object.kind),
            _ => None,
        };
        self.move_tool.proportional_text_corners = matches!(text_kind, Some(dtool_core::ObjectKind::ArtText { .. }));
        self.move_tool.frame_text_scale_handle = matches!(text_kind, Some(dtool_core::ObjectKind::FrameText { .. }));
        self.move_tool.set_selection_frame(selection_frame(scene, &session.document, &session.selection.ids));
    }

    pub(crate) fn displayed_transform_origin(&self, session: &Session) -> Option<[f32; 2]> {
        if tool_kind(&session.active_tool) != ToolKind::Move || session.active_tool == "tool.artboard" {
            return None;
        }
        session.selection_transform_origin().map(|point| self.origin_drag.as_ref().map_or(point, |drag| drag.point))
    }

    fn run(&mut self, session: &mut Session, command: Option<Command>) -> bool {
        let Some(command) = command else { return false };
        match session.run(&command) {
            Ok(_) => {
                self.last_error = None;
                true
            }
            Err(e) => {
                self.error_key = Some(e.message_key.clone());
                self.last_error = Some(e.code);
                false
            }
        }
    }

    fn create_art_text(&mut self, session: &mut Session, start: [f32; 2], end: [f32; 2]) -> bool {
        self.finish_art_text_input(session);
        let previous_selection = session.selection.clone();
        let previous_version = session.document.version;
        let previous_steps = session.undo.applied_steps();
        // The add and its selection each push history. Keep a backup only
        // when a push would displace an existing undo or redo entry.
        let previous_history = (session.undo.can_redo() || previous_steps.saturating_add(2) > session.undo.limit())
            .then(|| session.undo.clone());
        let scale = dtool_core::units::units_per_point(session.document.units, session.document.dpi);
        let mut command = art_text_command(start, end, scale);
        if new_art_text_is_vertical(session) {
            command.params["vertical"] = true.into();
        }
        match session.run(&command) {
            Ok(reply) => {
                let (Some(id), Some(story)) = (reply["id"].as_u64(), reply["story"].as_u64()) else { return false };
                let selected = self.enter_text(session, ObjectId(id), dtool_core::StoryId(story), 0);
                if selected {
                    self.art_draft = Some(ArtTextDraft {
                        context: self.context,
                        object: ObjectId(id),
                        story: dtool_core::StoryId(story),
                        generation: session.generation,
                        previous_version,
                        previous_selection,
                        previous_steps,
                        previous_history,
                    });
                }
                selected
            }
            Err(error) => {
                self.error_key = Some(error.message_key);
                self.last_error = Some(error.code);
                false
            }
        }
    }

    fn finish_art_text_input(&mut self, session: &mut Session) {
        let draft = self.art_draft.take().filter(|draft| {
            draft.context == self.context && session.text_edit.as_ref().and_then(|edit| edit.object) == Some(draft.object)
        });
        crate::canvas::frame_text_input::finish_text_input(session);
        let Some(draft) = draft else { return };
        if draft.generation != session.generation
            || session.document.version != draft.previous_version.saturating_add(1)
            || !session.document.stories.iter().any(|story| story.id == draft.story && story.text.is_empty())
        {
            return;
        }
        let undo = Command::with_params("text.art.undo_add", json!({ "id": draft.object.0, "story": draft.story.0 }));
        if dtool_core::text_cmd::apply_art_undo_add(&undo, &mut session.document).is_ok() {
            if let Some(history) = draft.previous_history {
                session.undo = history;
            } else {
                while session.undo.applied_steps() > draft.previous_steps {
                    session.undo.pop_last();
                }
            }
            session.selection = draft.previous_selection;
            session.document.version = draft.previous_version;
            session.generation += 1;
        }
    }

    fn art_text_target(&mut self, scene: &Scene, session: &Session, point: [f32; 2]) -> Option<(ObjectId, dtool_core::StoryId, usize)> {
        crate::canvas::frame_text_input::path_text_at(scene, session, point).or_else(|| {
            let mut id = self.hit(scene, session, point)?;
            let doc = &session.document;
            let mut object = doc.get_object(id)?;
            let group_hit = object.is_group();
            let tolerance = HIT_TOLERANCE_PX / session.view.zoom.max(1e-4);
            for _ in 0..doc.objects.len() {
                if !object.is_group() { break; }
                id = hit_test_in_view(scene, session, point, tolerance, Some(id))?;
                object = doc.get_object(id)?;
            }
            if group_hit && !dtool_core::scene::has_live_text_warp(doc, id) { return None; }
            let dtool_core::object::ObjectKind::ArtText { story } = object.kind else { return None; };
            Some((id, story, Self::art_caret_at(session, id, point)?))
        })
    }

    fn art_caret_at(session: &Session, id: ObjectId, point: [f32; 2]) -> Option<usize> {
        use dtool_tools::dtool_text as text;
        let doc = &session.document;
        let object = doc.get_object(id)?;
        if object.is_path_text() {
            return crate::canvas::frame_text_input::closest_path_insertion(doc, object, point)
                .map(|(index, _)| index);
        }
        let point = dtool_core::commands::warp::unwarp_point(doc, id, point);
        let library = text::FontLibrary::shared();
        let story = doc.stories.iter().find(|story| Some(story.id) == object.story_id())?;
        let story = text::bridge::story_in_document(story, doc);
        let lines = text::layout_story(&library, &story, None);
        let inverse = dtool_core::geometry::invert_matrix(&dtool_core::geometry::text_matrix(object))?;
        let [x, y] = dtool_core::geometry::apply_matrix(&inverse, point);
        let local = [x - object.pos[0], y - object.pos[1]];
        Some(text::frame::caret_at_point(&lines, local))
    }

    fn click_art_text(&mut self, scene: &Scene, session: &mut Session, point: [f32; 2]) -> bool {
        if let Some((id, story, caret)) = self.art_text_target(scene, session, point) {
            return self.enter_text(session, id, story, caret);
        }
        if session.text_edit.is_some() {
            self.finish_art_text_input(session);
            return true;
        }
        if let Some(path) = art_text_path_at(scene, session, point) {
            let command = Command::with_params("text.path_text.add", json!({ "path": path.0, "pos": point }));
            return match session.run(&command) {
                Ok(reply) => reply["story"].as_u64().is_some_and(|story| self.enter_text(session, path, dtool_core::StoryId(story), 0)),
                Err(error) => {
                    self.error_key = Some(error.message_key);
                    self.last_error = Some(error.code);
                    false
                }
            };
        }
        self.create_art_text(session, point, point)
    }

    fn enter_text(&mut self, session: &mut Session, id: ObjectId, story: dtool_core::StoryId, caret: usize) -> bool {
        if self.art_draft.as_ref().is_some_and(|draft| draft.object != id) {
            self.finish_art_text_input(session);
        }
        if session.selection.ids.as_slice() != [id] {
            self.run(session, Some(Command::with_params("selection.set", json!({ "ids": [id.0] }))));
        }
        self.run(session, Some(Command::with_params("text.select", json!({
            "id": id.0, "story": story.0, "range": { "start": caret, "end": caret }
        }))))
    }

    fn style_picker_click(&mut self, session: &mut Session, hit: ObjectId, sample_again: bool) -> bool {
        let targets: Vec<u64> = if sample_again || !self.style_picker.has_style() {
            if let Err(error) = self.style_picker.pick_object(session, hit.0) {
                self.error_key = Some(error.message_key);
                self.last_error = Some(error.code);
                return false;
            }
            session.selection.ids.iter().filter(|id| **id != hit)
                .filter(|id| session.document.get_object(**id).is_some_and(|object| !object.locked))
                .map(|id| id.0).collect()
        } else {
            vec![hit.0]
        };
        self.apply_picked_style(session, targets)
    }

    fn apply_picked_style(&mut self, session: &mut Session, targets: Vec<u64>) -> bool {
        if targets.is_empty() {
            return false;
        }
        let option = |part| session.tool_options.flag("tool.style_picker", part).unwrap_or(true);
        self.style_picker.parts = dtool_tools::style_picker::StyleParts {
            fill: option("fill"),
            stroke: option("stroke"),
            paint_order: option("paint_order"),
            opacity: option("opacity"),
            effects: option("effects"),
            blend_mode: option("blend_mode"),
        };
        let command = self.style_picker.apply_command().map(|mut command| {
            command.params["ids"] = json!(targets);
            command
        });
        self.run(session, command)
    }

    pub fn hit(&mut self, scene: &Scene, session: &Session, point: [f32; 2]) -> Option<ObjectId> {
        let tolerance = HIT_TOLERANCE_PX / session.view.zoom.max(1e-4);
        if let Some(scope) = self.scope.filter(|id| session.document.get_object(*id).is_some()) {
            let hit = hit_test_in_view(scene, session, point, tolerance, Some(scope));
            if hit.is_some_and(|id| dtool_core::scene_hit::pick_parent(&session.document, id) == Some(scope)) {
                return hit;
            }
        }
        self.scope = None;
        hit_test_in_view(scene, session, point, tolerance, None)
    }

    pub fn enter_group(&mut self, scene: &Scene, session: &mut Session, point: [f32; 2]) -> bool {
        let Some(group) = self.hit(scene, session, point).filter(|id| session.document.get_object(*id).is_some_and(|o| o.is_group())) else { return false };
        let tolerance = HIT_TOLERANCE_PX / session.view.zoom.max(1e-4);
        let child = hit_test_in_view(scene, session, point, tolerance, Some(group)).filter(|id| dtool_core::scene_hit::pick_parent(&session.document, *id) == Some(group));
        let Some(child) = child else { return false };
        self.scope = Some(group);
        self.run(session, Some(Command::with_params("selection.set", json!({ "ids": [child.0], "op": "Replace" }))))
    }

    pub fn exit_group(&mut self, session: &mut Session) -> bool {
        let Some(group) = self.scope.take() else { return false };
        session.document.get_object(group).is_some() && self.run(session, Some(Command::with_params("selection.set", json!({ "ids": [group.0], "op": "Replace" }))))
    }

    pub fn press_move(&mut self, scene: &Scene, session: &mut Session, point: [f32; 2], mods: Mods) -> MoveToolAction {
        self.sync(scene, session);
        let on_handle = self.move_tool.selection_box.and_then(|b| self.move_tool.hit_handle(b, point)).is_some();
        self.preselected = false;
        if !on_handle {
            let hit = self.hit(scene, session, point);
            let on_selected_master = hit.is_some_and(|id| {
                session.document.master_pages.inherited_content().any(|binding| {
                    binding.group.is_some_and(|group| session.selection.ids.contains(&group)
                        && session.document.subtree_ids(group).contains(&id))
                })
            });
            if let Some(id) = hit.filter(|id| !on_selected_master && !session.selection.ids.contains(id)) {
                let op = if mods.shift { "Add" } else { "Replace" };
                self.preselected = self.run(session, Some(Command::with_params("selection.set", json!({ "ids": [id.0], "op": op }))));
                self.move_tool.set_selection_frame(selection_frame(scene, &session.document, &session.selection.ids));
            } else if hit.is_none() && (!self.move_tool.show_handles || session.selection.ids.iter().any(|id| {
                session.document.get_object(*id).is_some_and(dtool_core::artboard::is_artboard)
            })) {
                // Empty paper starts a marquee when the box is hidden or an artboard is selected.
                self.move_tool.set_selection_frame(None);
            }
        }
        let action = self.move_tool.on_pointer(PointerEvent { x: point[0], y: point[1], pressed: true, released: false, shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt });
        self.press_at = Some(point);
        self.start_box = selection_box(scene, &session.document, &session.selection.ids);
        self.apply_preview(&action);
        action
    }

    pub fn drag_move(&mut self, point: [f32; 2], mods: Mods) -> MoveToolAction {
        let action = self.move_tool.on_pointer(PointerEvent { x: point[0], y: point[1], pressed: false, released: false, shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt });
        self.apply_preview(&action);
        action
    }

    pub fn release_move(&mut self, scene: &Scene, session: &mut Session, point: [f32; 2], mods: Mods) -> MoveToolAction {
        let action = self.move_tool.on_pointer(PointerEvent { x: point[0], y: point[1], pressed: false, released: true, shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt });
        self.marquee = None;
        self.preview = None;
        match &action {
            MoveToolAction::HitSelect { point, .. } => {
                if !std::mem::take(&mut self.preselected) {
                    let hit = self.hit(scene, session, *point);
                    let ids = session.selection.ids.clone();
                    if mods.shift || hit.is_none_or(|h| ids != vec![h]) {
                        self.select_move_hit(session, hit, mods);
                    }
                }
            }
            other => {
                self.run(session, move_action_command(other));
            }
        }
        action
    }

    pub fn click_move(&mut self, scene: &Scene, session: &mut Session, point: [f32; 2], mods: Mods) {
        let hit = self.hit(scene, session, point);
        self.select_move_hit(session, hit, mods);
    }

    fn select_move_hit(&mut self, session: &mut Session, hit: Option<ObjectId>, mods: Mods) {
        let ids = &session.selection.ids;
        let command = if mods.alt && !mods.shift && ids.len() > 1
            && let Some(id) = hit.filter(|id| ids.contains(id))
        {
            Some(Command::with_params(dtool_core::commands::selection_key::KEY_SET, json!({ "id": id.0 })))
        } else {
            click_select_command(hit, ids, mods.shift)
        };
        self.run(session, command);
    }

    fn apply_preview(&mut self, action: &MoveToolAction) {
        match action {
            MoveToolAction::MarqueeUpdate { rect } => self.marquee = Some(*rect),
            MoveToolAction::TransformPreview { args } => self.preview = Some(args.clone()),
            MoveToolAction::DuplicatePreview { offset } => {
                self.preview = Some(TransformSelectionBoxArgs { translate: *offset, ..Default::default() })
            }
            MoveToolAction::MarqueeStart { .. } | MoveToolAction::GestureStart { .. } => {
                self.marquee = None;
                self.preview = None;
            }
            _ => {}
        }
    }

    pub fn finish_drag(&mut self, session: &mut Session, end: [f32; 2], mods: Mods) -> bool {
        let command = match self.drag.take() {
            Some(Drag::Shape { kind, start, .. }) => configured_shape_command(session, kind, start, end, mods).and_then(|mut command| {
                if session.active_tool == "tool.qr" {
                    let pos = serde_json::from_value(command.params["pos"].clone()).ok()?;
                    let size = serde_json::from_value(command.params["size"].clone()).ok()?;
                    return Some(dtool_tools::qr_tool::create(pos, size));
                }
                if session.active_tool == "tool.artboard" {
                    let [x, y]: [f32; 2] = serde_json::from_value(command.params["pos"].clone()).ok()?;
                    let [w, h]: [f32; 2] = serde_json::from_value(command.params["size"].clone()).ok()?;
                    let page = dtool_core::artboard::edit::page_at_point(&session.document, start);
                    return Some(Command::with_params("artboard.create", json!({
                        "source": "drag", "rect": [x, y, w, h], "page_index": page,
                    })));
                }
                let defaults = dtool_core::commands::defaults::current();
                let mut stroke = defaults.stroke_for_document(&session.document);
                if kind == ShapeKind::Spiral && (stroke.width <= 0.0 || matches!(stroke.paint, dtool_core::paint::Paint::None)) {
                    stroke = dtool_core::commands::curve_edit::default_stroke();
                    stroke.width *= dtool_core::units::units_per_point(session.document.units, session.document.dpi);
                }
                command.params["stroke"] = json!(dtool_core::commands::context_toolbar::drawing_stroke(session, &session.active_tool, stroke));
                if kind != ShapeKind::Spiral {
                    command.params["fill"] = json!(defaults.fill);
                }
                if dtool_core::context_toolbar::is_advanced_shape_tool(&session.active_tool) {
                    command.params["select"] = json!(true);
                }
                Some(command)
            }),
            Some(Drag::PictureFrame { shape, start, .. }) => picture_frame_command(shape, start, end, mods),
            Some(Drag::ArtText { start, .. }) => return self.create_art_text(session, start, end),
            Some(Drag::Crop { target, start, .. }) => crate::canvas_tools::crop_command(session, target, start, end),
            Some(Drag::Slice { start, edit, .. }) => {
                let zoom = session.view.zoom;
                let delta = [(start[0] - end[0]).abs() * zoom, (start[1] - end[1]).abs() * zoom];
                if let Some(edit) = edit {
                    (delta[0].hypot(delta[1]) >= 2.0).then(|| {
                        Command::with_params("slice.update", json!({ "id": edit.id, "rect": edit.bounds_at(start, end) }))
                    })
                } else {
                    (delta[0] >= 4.0 && delta[1] >= 4.0).then(|| Command::with_params("slice.create", json!({
                        "rect": [start[0].min(end[0]), start[1].min(end[1]), start[0].max(end[0]), start[1].max(end[1])]
                    })))
                }
            }
            Some(Drag::Gradient { kind, transparency, target, start, .. }) => {
                crate::canvas_tools::gradient_command_for_tool(session, target, kind, transparency, start, end)
            }
            Some(Drag::MaskGradient { kind, start, .. }) => {
                let end = if mods.shift { crate::canvas_curves::constrain_angle(start, end) } else { end };
                (session.quick_mask.active && (end[0] - start[0]).hypot(end[1] - start[1]) * session.view.zoom >= 2.0)
                    .then(|| Command::with_params(dtool_core::commands::quickmask::GRADIENT,
                        json!({ "kind": kind, "start": start, "end": end })))
            }
            Some(Drag::RedEye { target, start, .. }) => dtool_core::commands::red_eye::canvas_command(target, start, end),
            Some(Drag::Line { start, .. }) => {
                let end = if mods.shift { crate::canvas_curves::constrain_angle(start, end) } else { end };
                crate::canvas_curves::line_command(start, end).map(|mut command| {
                    command.params["stroke"] = json!(crate::canvas_curves::drawing_curve_stroke(session));
                    command
                })
            }
            Some(Drag::ShapeParam { target, kind, grab_offset, .. }) => editable_shape_params(session)
                .filter(|object| object.id == target)
                .and_then(|object| shape_tool::apply_param_drag(object, kind, [end[0] + grab_offset[0], end[1] + grab_offset[1]])),
            None => None,
        };
        self.run(session, command)
    }

    pub fn tool_drag(&mut self, kind: ToolKind, scene: &Scene, session: &Session, start: [f32; 2]) -> Option<Drag> {
        let single = match session.selection.ids.as_slice() {
            [id] => Some(*id),
            _ => None,
        };
        match kind {
            ToolKind::Crop => single.map(|target| Drag::Crop { target, start, current: start }),
            ToolKind::Gradient(_) | ToolKind::Transparency => {
                let transparency = kind == ToolKind::Transparency;
                let gradient = match kind { ToolKind::Gradient(g) => g, _ => None };
                if session.quick_mask.active {
                    let choice = session.tool_options.choice(&session.active_tool, "gradient.type");
                    let kind = choice.as_deref().and_then(crate::canvas_tools::gradient_kind).or(gradient)
                        .unwrap_or(GradientKind::Linear);
                    return Some(Drag::MaskGradient { kind, start, current: start });
                }
                single.or_else(|| self.hit(scene, session, start))
                    .filter(|id| session.document.get_object(*id).is_some_and(|object| {
                        !object.locked && !matches!(
                            dtool_core::commands::context_toolbar::gradient::Target::for_object(session, &session.active_tool, object)
                                .and_then(|target| target.paint(object)),
                            Some(dtool_core::paint::Paint::Bitmap(_))
                        )
                    }))
                    .map(|target| Drag::Gradient { kind: gradient, transparency, target, start, current: start })
            }
            _ => None,
        }
    }

    pub fn handle(&mut self, ui: &mut Ui, response: &Response, session: &mut Session, scene: &Scene) {
        self.update(ui, response, session, scene, true);
    }

    pub(crate) fn update(&mut self, ui: &mut Ui, response: &Response, session: &mut Session, scene: &Scene, paint_overlay: bool) {
        if crate::rulers::guide_drag_active(ui.ctx()) {
            crate::rulers::set_canvas_gesture(ui.ctx(), false);
            return;
        }
        self.handle_input(ui, response, session, scene, paint_overlay);
        crate::rulers::set_canvas_gesture(ui.ctx(), self.gesture_active());
    }

    fn handle_input(&mut self, ui: &mut Ui, response: &Response, session: &mut Session, scene: &Scene, paint_overlay: bool) {
        let rect = response.rect;
        if self.resolve_owner(session, scene) {
            ui.ctx().request_repaint();
        }
        let view = session.view.clone();
        let kind = tool_kind(&session.active_tool);
        if kind != ToolKind::Move { self.move_press = None; }
        if kind != ToolKind::Eyedropper {
            session.panel_colour_pick_previous = None;
        }
        if kind != ToolKind::Blend { self.blend.clear(); }
        if kind != ToolKind::VectorCrop { self.vector_crop.cancel(); }
        if !matches!(kind, ToolKind::PixelMarquee(_)) { self.pixel_marquee.cancel(); }
        if kind != ToolKind::Measure && self.measure.has_state() {
            self.measure.clear();
        }
        if kind != ToolKind::Area {
            if self.area.active() { self.marquee = None; }
            self.area.clear();
        }
        if !matches!(kind, ToolKind::Pen { .. }) {
            self.pen_press = None;
            if self.pen.active() {
                let command = self.pen.finish_command(false);
                self.run(session, command);
            }
        }
        if kind != ToolKind::Node && self.nodes != crate::canvas_curves::NodeEdit::default() {
            self.nodes = crate::canvas_curves::NodeEdit::default();
        }
        if kind == ToolKind::Table {
            if let Err(error) = crate::table::canvas::handle(ui, response, session, scene, &view, self.context) {
                self.error_key = Some(error.message_key);
                self.last_error = Some(error.code);
            }
            return;
        }
        if kind == ToolKind::FrameText {
            crate::canvas::frame_text_input::handle(ui, session, response, &view);
            return;
        }
        if kind == ToolKind::ArtText {
            crate::canvas::frame_text_input::text_input(ui, session, response, &view);
        } else {
            crate::canvas::frame_text_input::commit_pending(session);
        }
        let (mods, escape, pointer) = ui.input(|i| {
            (Mods { shift: i.modifiers.shift, ctrl: i.modifiers.command, alt: i.modifiers.alt }, i.key_pressed(egui::Key::Escape), i.pointer.latest_pos())
        });
        let keyboard = ui.ctx().memory(|m| m.focused().map_or(true, |id| id == response.id));
        let escape = escape && keyboard;
        if escape && let Some(previous) = session.panel_colour_pick_previous.take() {
            session.active_tool = previous;
            ui.ctx().request_repaint();
            return;
        }
        if self.warp_mesh.active() || !self.busy() {
            let (handled, command) = self.warp_mesh.input(ui, response, session, escape);
            if handled {
                if self.warp_mesh.active() && self.owner.is_none() {
                    self.claim(session);
                } else if !self.warp_mesh.active() {
                    self.owner = None;
                }
                self.run(session, command);
                self.warp_mesh.paint(ui, rect, session);
                return;
            }
        }
        if escape && kind == ToolKind::ArtText && !egui::Popup::is_any_open(ui.ctx()) {
            self.cancel();
            self.finish_art_text_input(session);
            let command = Command::with_params("ui.tool.set", json!({ "id": "tool.move" }));
            if let Err(error) = crate::tools_panel::handle_tool_command(session, &command) {
                self.error_key = Some(error.message_key);
                self.last_error = Some(error.code);
            }
            ui.ctx().request_repaint();
            return;
        }
        if escape && kind == ToolKind::Measure && self.measure.has_state() {
            self.measure.clear();
            ui.ctx().request_repaint();
            return;
        }
        if escape && kind == ToolKind::StylePicker && self.style_picker_loaded() && self.style_drag.is_none() {
            self.clear_style_picker();
            ui.ctx().request_repaint();
            return;
        }
        let primary = ui.input(|i| i.pointer.primary_down() || i.pointer.primary_released());
        let to_doc = |p: Pos2| dtool_core::view_to_doc(&view, [p.x - rect.min.x, p.y - rect.min.y]);
        if escape && self.pen.active() && kind_is_pen(&session.active_tool) {
            self.pen.cancel();
            self.pen_press = None;
            ui.ctx().request_repaint();
            return;
        }
        if escape && self.busy() {
            self.cancel();
            return;
        }
        if escape && tool_kind(&session.active_tool) == ToolKind::Node && self.nodes.selected_count() != 0 {
            self.nodes.clear_selection();
            ui.ctx().request_repaint();
            return;
        }
        if escape && self.exit_group(session) {
            ui.ctx().request_repaint();
        }
        if keyboard && session.text_edit.is_none() {
            if let Some(Err(e)) = crate::canvas_tools::nudge(ui.ctx(), &self.move_tool, session, || *self.nudge_prefs.get_or_insert_with(crate::canvas_tools::nudge_distances)) {
                self.error_key = Some(e.message_key.clone());
                self.last_error = Some(e.code);
            }
        }
        let press_origin = ui.input(|i| i.pointer.press_origin());
        let press = press_origin.map(to_doc);
        let point = response.interact_pointer_pos().or(pointer).map(to_doc);
        self.hover = pointer.filter(|_| response.hovered()).map(to_doc);
        let mut changed = false;
        let (released, double, enter) = ui.input(|i| (i.pointer.primary_released(), i.pointer.button_double_clicked(egui::PointerButton::Primary), i.key_pressed(egui::Key::Enter)));
        let enter = enter && keyboard;
        let near = HANDLE_PX / view.zoom.max(1e-4);
        if self.origin_drag.as_ref().is_some_and(|drag| drag.ids != session.selection.ids) {
            self.origin_drag = None;
        }
        if let Some(origin) = self.displayed_transform_origin(session).filter(|_| !view.canvas_flags.preview_mode) {
            let (down, up) = primary_button_positions(ui);
            let at = screen(rect, &view, origin);
            if !self.move_tool.dragging && self.origin_drag.is_none()
                && let Some(start) = down.filter(|p| p.distance(at) <= 8.0 && response.enabled()
                    && response.interact_rect.contains(*p) && ui.ctx().layer_id_at(*p) == Some(response.layer_id)
                    && !egui::Popup::is_any_open(ui.ctx()))
            {
                let start = to_doc(start);
                self.origin_drag = Some(OriginDrag {
                    offset: [origin[0] - start[0], origin[1] - start[1]],
                    point: origin,
                    ids: session.selection.ids.clone(),
                });
                response.request_focus();
            }
            if let Some(drag) = &mut self.origin_drag {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if let Some(point) = up.or(pointer).map(to_doc) {
                    drag.point = [point[0] + drag.offset[0], point[1] + drag.offset[1]];
                }
                if up.is_some() {
                    let _ = session.run_named("transform.origin.set", serde_json::json!({"point": drag.point}));
                    self.origin_drag = None;
                } else if !primary {
                    self.origin_drag = None;
                }
                ui.ctx().request_repaint();
                return;
            }
            if response.hovered() && pointer.is_some_and(|p| p.distance(at) <= 8.0) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
        } else {
            self.origin_drag = None;
        }
        if session.active_tool == "tool.artboard" && self.artboard_neighbours(ui, response, session) {
            return;
        }
        if (kind == ToolKind::Move || session.active_tool == "tool.artboard")
            && self.artboard_gesture(ui, response, session, scene, pointer, primary, mods)
        {
            if paint_overlay {
                self.paint_preview(ui, rect, session, scene);
            }
            return;
        }
        match kind {
            ToolKind::Move => {
                let (down, up) = crate::input::primary_button_positions(ui);
                let down = down.filter(|pos| {
                    response.enabled()
                        && response.interact_rect.contains(*pos)
                        && ui.ctx().layer_id_at(*pos) == Some(response.layer_id)
                        && !egui::Popup::is_any_open(ui.ctx())
                });
                if down.is_some() {
                    self.move_press = down;
                }
                let moved_from_press = self.move_press.filter(|start| {
                    up.or(pointer).is_some_and(|end| {
                        start.distance(end) > ui.ctx().options(|options| options.input_options.max_click_dist)
                    })
                });
                // Keep fast drags even when their press and
                // release arrive before egui reports a widget drag.
                if !self.move_tool.dragging && self.drag.is_none()
                    && let Some(start) = down.or(moved_from_press).map(to_doc)
                {
                    self.sync(scene, session);
                    let handle = self.move_tool.selection_box.and_then(|b| self.move_tool.hit_handle(b, start));
                    let hit = self.hit(scene, session, start);
                    let selected_group = hit.is_some_and(|id| {
                        session.selection.ids.contains(&id)
                            && session.document.get_object(id).is_some_and(dtool_core::Object::is_group)
                    });
                    if let Some(drag) = shape_param_drag(session, start, near) {
                        self.drag = Some(drag);
                        response.request_focus();
                    } else if handle.is_some() || selected_group || moved_from_press.is_some() {
                        self.press_move(scene, session, start, mods);
                        response.request_focus();
                    }
                }
                if !self.move_tool.dragging && self.drag.is_none() && response.drag_started() && primary {
                    if let Some(start) = press.or(point) {
                        self.press_move(scene, session, start, mods);
                    }
                }
                if self.move_tool.dragging || self.drag.is_some() || up.is_some() || !primary {
                    self.move_press = None;
                }
                if let (Some(Drag::ShapeParam { current, .. }), Some(p)) = (self.drag.as_mut(), point) {
                    *current = p;
                }
                let param_gesture = matches!(self.drag, Some(Drag::ShapeParam { .. }));
                if param_gesture {
                    let end = up.map(to_doc).or_else(|| response.drag_stopped().then_some(point).flatten());
                    if let Some(p) = end {
                        let threshold = ui.ctx().options(|options| options.input_options.max_click_dist);
                        if let Some(Drag::ShapeParam { start, .. }) = self.drag
                            && (p[0] - start[0]).hypot(p[1] - start[1]) * view.zoom > threshold
                        {
                            changed = self.finish_drag(session, p, mods);
                        } else {
                            self.drag = None;
                        }
                    } else if !primary {
                        self.drag = None;
                    }
                }
                if (response.dragged() || primary && self.move_tool.gesture == Some(GestureKind::Move))
                    && self.move_tool.dragging
                {
                    if let Some(p) = point {
                        let p = self.snap_drag(session, scene, p, mods);
                        self.drag_move(p, mods);
                        self.update_guides(session, scene, p);
                    }
                }
                let transform_drag = self.move_tool.gesture.is_some_and(|kind| kind != GestureKind::Marquee);
                let end = up.map(to_doc).or_else(|| response.drag_stopped().then_some(point).flatten());
                if self.move_tool.dragging && let Some(p) = end {
                    let click = self.press_at.filter(|start| {
                        transform_drag && !response.drag_stopped()
                            && (p[0] - start[0]).hypot(p[1] - start[1]) * view.zoom <= 3.0
                    });
                    let p = click.unwrap_or_else(|| self.snap_drag(session, scene, p, mods));
                    self.release_move(scene, session, p, mods);
                    changed = true;
                } else if response.clicked() && !param_gesture {
                    if let Some(p) = point {
                        self.click_move(scene, session, p, mods);
                        changed = true;
                    }
                }
                if response.double_clicked() && !param_gesture && !mods.alt {
                    if let Some(p) = point {
                        let hit = self.hit(scene, session, p).filter(|id|
                            session.document.get_object(*id).is_none_or(|object| object.kind != dtool_core::ObjectKind::Artboard));
                        let opened = crate::table::canvas::edit_at(session, scene, p, hit);
                        match opened {
                            Ok(true) => { response.request_focus(); changed = true; }
                            Ok(false) => changed |= self.enter_group(scene, session, p),
                            Err(error) => { self.error_key = Some(error.message_key); self.last_error = Some(error.code); }
                        }
                    }
                }
                if let (Some(p), false) = (point.filter(|_| response.hovered()), self.move_tool.dragging) {
                    self.sync(scene, session);
                    if param_gesture && primary {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    } else if shape_param_drag(session, p, near).is_some() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    } else if let Some(handle) = self.move_tool.selection_box.and_then(|b| self.move_tool.hit_handle(b, p)) {
                        ui.ctx().set_cursor_icon(handle_cursor(handle));
                    }
                }
                if self.move_tool.dragging && self.move_tool.gesture == Some(GestureKind::Move) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                }
            }
            ToolKind::Shape(_) | ToolKind::PictureFrame(_) => {
                if response.hovered() {
                    let over_handle = point.is_some_and(|p| shape_param_drag(session, p, near).is_some());
                    ui.ctx().set_cursor_icon(if matches!(self.drag, Some(Drag::ShapeParam { .. })) {
                        egui::CursorIcon::Grabbing
                    } else if over_handle {
                        egui::CursorIcon::Grab
                    } else {
                        egui::CursorIcon::Crosshair
                    });
                }
                // The canvas owns the press even if press, move and release
                // all arrive before egui reports a widget drag.
                let (down, up) = crate::input::primary_button_positions(ui);
                if self.drag.is_none() && let Some(start) = down.filter(|pos| {
                    response.enabled()
                        && response.interact_rect.contains(*pos)
                        && ui.ctx().layer_id_at(*pos) == Some(response.layer_id)
                        && !egui::Popup::is_any_open(ui.ctx())
                }) {
                    self.shape_press = Some(start);
                    response.request_focus();
                }
                let endpoint = up.or(pointer);
                let threshold = ui.ctx().options(|options| options.input_options.max_click_dist);
                if let (Some(start), Some(end)) = (self.shape_press, endpoint)
                    && start.distance(end) > threshold
                {
                    self.shape_press = None;
                    let start = to_doc(start);
                    self.drag = Some(match kind {
                        ToolKind::PictureFrame(shape) => {
                            let start = self.snap_free(session, start);
                            Drag::PictureFrame { shape, start, current: start }
                        }
                        ToolKind::Shape(shape) => {
                            if let Some(drag) = shape_param_drag(session, start, near) {
                                drag
                            } else {
                                let start = self.snap_free(session, start);
                                Drag::Shape { kind: shape, start, current: start }
                            }
                        }
                        _ => unreachable!(),
                    });
                }
                if up.is_some() || !primary {
                    self.shape_press = None;
                }
                let point = endpoint.map(to_doc);
                let point = match (&self.drag, point) {
                    (Some(Drag::Shape { .. } | Drag::PictureFrame { .. }), Some(p)) => Some(self.snap_free(session, p)),
                    _ => point,
                };
                if let (Some(Drag::Shape { current, .. } | Drag::ShapeParam { current, .. } | Drag::PictureFrame { current, .. }), Some(p)) = (self.drag.as_mut(), point) {
                    *current = p;
                }
                if matches!(self.drag, Some(Drag::Shape { .. } | Drag::ShapeParam { .. } | Drag::PictureFrame { .. })) {
                    if let (true, Some(p)) = (up.is_some(), point) {
                        changed = self.finish_drag(session, p, mods);
                    } else if !primary {
                        self.cancel();
                    }
                } else if session.active_tool == "tool.artboard" && response.clicked() {
                    if mods.ctrl && let Some(point) = point {
                        crate::artboard_bar::open_custom(ui.ctx(), session, point);
                    } else {
                        changed = self.run(session, click_select_command(None, &session.selection.ids, false));
                    }
                }
            }
            ToolKind::Eyedropper => {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                if response.clicked() && let Some(p) = point {
                    if let Some(previous) = session.panel_colour_pick_previous.take() {
                        session.active_tool = previous;
                        let command = Command::with_params(dtool_core::commands::colour_pick::COLOR_PICK,
                            json!({ "x": p[0], "y": p[1], "zoom": session.view.zoom, "apply": false }));
                        match session.run(&command).and_then(|result| {
                            dtool_core::commands::swatches::parse_colour(&result["colour"]["hex"])
                        }) {
                            Ok(colour) => {
                                session.panel_sampled_colour = Some(colour);
                                self.last_error = None;
                            }
                            Err(error) => {
                                self.error_key = Some(error.message_key);
                                self.last_error = Some(error.code);
                            }
                        }
                        ui.ctx().request_repaint();
                    } else {
                        let command = Command::with_params(dtool_core::commands::colour_pick::COLOR_PICK, json!({ "x": p[0], "y": p[1], "zoom": session.view.zoom }));
                        changed = self.run(session, Some(command));
                    }
                }
            }
            ToolKind::ArtText => {
                if let Some(pointer) = response.hover_pos() {
                    if self.drag.is_none() && art_text_path_at(scene, session, to_doc(pointer)).is_some() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                        paint_art_text_path_cursor(ui, response.rect, pointer);
                    } else if new_art_text_is_vertical(session) || session.text_edit.as_ref()
                        .and_then(|edit| edit.object)
                        .and_then(|id| session.document.get_object(id))
                        .is_some_and(dtool_core::Object::is_vertical_art_text)
                    {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::VerticalText);
                    } else {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                    }
                }
                let (down, up) = crate::input::primary_button_positions(ui);
                if let Some(start) = down.filter(|pos| response.interact_rect.contains(*pos)).map(to_doc) {
                    self.art_selection = None;
                    self.drag = Some(Drag::ArtText { start, current: start });
                }
                if let (Some(Drag::ArtText { current, .. }), Some(pos)) = (self.drag.as_mut(), up.or(pointer)) {
                    *current = to_doc(pos);
                }
                if let Some(Drag::ArtText { start, current }) = self.drag {
                    let moved = (start[0] - current[0]).hypot(start[1] - current[1]) * view.zoom >= 4.0;
                    if moved && self.art_selection.is_none() {
                        if let Some((id, story, caret)) = self.art_text_target(scene, session, start) {
                            if self.enter_text(session, id, story, caret) {
                                self.art_selection = Some(id);
                                response.request_focus();
                            }
                        }
                    }
                    if let Some(id) = self.art_selection {
                        if let Some(caret) = Self::art_caret_at(session, id, current) {
                            crate::canvas::frame_text_input::extend_selection(session, caret);
                        }
                    }
                    if up.is_some() {
                        if self.art_selection.take().is_some() {
                            self.drag = None;
                        } else if moved && session.text_edit.is_some() {
                            self.drag = None;
                            self.finish_art_text_input(session);
                            changed = true;
                        } else if moved {
                            changed = self.finish_drag(session, current, mods);
                        } else {
                            self.drag = None;
                            changed = self.click_art_text(scene, session, current);
                        }
                    }
                }
                if changed { response.request_focus(); }
            }
            ToolKind::VectorCrop => {
                let scope = self.scope;
                let command = self.vector_crop.input(ui, response, session, scene, mods, to_doc, |point| {
                    hit_test_in_view(scene, session, point, near, scope)
                });
                changed = self.run(session, command);
            }
            ToolKind::Gradient(_) | ToolKind::Transparency => {
                let (down, up) = primary_button_positions(ui);
                if let Some(start) = down.filter(|at| response.enabled()
                    && response.interact_rect.contains(*at)
                    && ui.ctx().layer_id_at(*at) == Some(response.layer_id)
                    && !egui::Popup::is_any_open(ui.ctx()))
                {
                    let at = to_doc(start);
                    self.drag = if !session.quick_mask.active && self.gradient_edit.press(session, rect, start, at) {
                        None
                    } else {
                        self.tool_drag(kind, scene, session, at)
                    };
                }
                if !session.quick_mask.active && self.gradient_edit.active() {
                    let command = up.map(to_doc).or(point)
                        .and_then(|point| self.gradient_edit.motion(session, point, up.is_some()));
                    changed = self.run(session, command);
                    if up.is_some() || !primary {
                        self.gradient_edit.cancel();
                    }
                } else if session.quick_mask.active {
                    if let (Some(Drag::MaskGradient { start, current, .. }), Some(at)) = (self.drag.as_mut(), point) {
                        *current = if mods.shift { crate::canvas_curves::constrain_angle(*start, at) } else { at };
                    }
                    if let Some(end) = up {
                        changed = self.finish_drag(session, to_doc(end), mods);
                    } else if !primary {
                        self.drag = None;
                    }
                } else {
                    if let (Some(Drag::Gradient { current, .. }), Some(at)) = (self.drag.as_mut(), point) {
                        *current = at;
                    }
                    if let Some(end) = up {
                        if response.double_clicked() {
                            self.drag = None;
                            let command = self.gradient_edit.insert(session, rect, end, to_doc(end));
                            changed = self.run(session, command);
                        } else if response.clicked() {
                            self.drag = None;
                            if session.selection.ids.is_empty() {
                                let hit = self.hit(scene, session, to_doc(end));
                                changed = self.run(session, click_select_command(hit, &session.selection.ids, mods.shift));
                            }
                        } else {
                            changed = self.finish_drag(session, to_doc(end), mods);
                        }
                    } else if !primary {
                        self.drag = None;
                    }
                }
                if !session.quick_mask.active && self.gradient_edit.active() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if let Some(at) = response.hover_pos() {
                    let cursor = if session.quick_mask.active { None } else {
                        gradient_edit::GradientEditor::handle_cursor(session, rect, at)
                    }.unwrap_or(egui::CursorIcon::Crosshair);
                    ui.ctx().set_cursor_icon(cursor);
                }
            }
            ToolKind::Slice => {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                let (down, up) = primary_button_positions(ui);
                if let Some(start) = down.filter(|pos| response.enabled() && response.interact_rect.contains(*pos)
                    && ui.ctx().layer_id_at(*pos) == Some(response.layer_id) && !egui::Popup::is_any_open(ui.ctx()))
                {
                    let pointer = start;
                    let start = to_doc(pointer);
                    let edit = if mods.shift { None } else { slice_hit(session, rect, pointer, start) };
                    let _ = session.run_named("slice.select", json!({ "id": edit.map(|target| target.id) }));
                    self.drag = Some(Drag::Slice { start, current: start, edit });
                }
                if let (Some(Drag::Slice { current, .. }), Some(p)) = (self.drag.as_mut(), point) {
                    *current = p;
                    ui.ctx().request_repaint();
                }
                if let Some(end) = up {
                    changed = self.finish_drag(session, to_doc(end), mods);
                } else if !primary {
                    self.drag = None;
                }
            }
            ToolKind::Crop => {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                if response.drag_started() && primary {
                    if let Some(start) = press.or(point) {
                        self.drag = self.tool_drag(kind, scene, session, start);
                    }
                }
                if let (Some(Drag::Crop { current, .. }), Some(p)) = (self.drag.as_mut(), point) {
                    *current = p;
                }
                if response.drag_stopped() {
                    if let Some(p) = point {
                        changed = self.finish_drag(session, p, mods);
                    }
                } else if response.clicked() && kind == ToolKind::Crop {
                    let command = session.selection.ids.first().and_then(|id| crate::canvas_tools::clear_crop_command(session, *id));
                    changed = self.run(session, command);
                }
            }
            ToolKind::Line => {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                // A quick mouse gesture can press and release before egui marks a drag.
                let (down, up) = crate::input::primary_button_positions(ui);
                if let Some(start) = down.filter(|pos| {
                    response.enabled()
                        && response.interact_rect.contains(*pos)
                        && ui.ctx().layer_id_at(*pos) == Some(response.layer_id)
                        && !egui::Popup::is_any_open(ui.ctx())
                }) {
                    let start = self.snap_free(session, to_doc(start));
                    self.drag = Some(Drag::Line { start, current: start });
                    response.request_focus();
                }
                if let Some(Drag::Line { start, .. }) = self.drag {
                    if let Some(p) = up.or(pointer).map(to_doc) {
                        let end = if mods.shift { crate::canvas_curves::constrain_angle(start, p) } else { self.snap_free(session, p) };
                        if up.is_some() {
                            changed = self.finish_drag(session, end, mods);
                        } else if primary {
                            self.drag = Some(Drag::Line { start, current: end });
                        }
                    }
                    if !primary {
                        self.drag = None;
                    }
                }
            }
            ToolKind::Pen { polyline } => {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                if !self.pen.active() {
                    self.pen.stroke = Some(crate::canvas_curves::drawing_curve_stroke(session));
                }
                self.pen.polyline = polyline;
                self.pen.alt = mods.alt;
                self.pen.shift = mods.shift;
                self.pen.space = keyboard && ui.input(|i| i.key_down(egui::Key::Space));
                let option = session.tool_options.value(&session.active_tool, "pen.mode");
                self.pen.mode = crate::canvas_curves::PenMode::for_tool(&session.active_tool, option.as_ref().and_then(|v| v.as_str()));
                self.pen.rubber_band = session.tool_options.flag(&session.active_tool, "pen.rubber_band").unwrap_or(false);
                let construction = session.tool_options.flag(
                    &session.active_tool, dtool_core::context_toolbar::PEN_CONSTRUCTION_SNAP,
                ).unwrap_or(false);
                let tolerance = session.snap_prefs.query().screen_tolerance as f32 / view.zoom.max(1e-4);
                // Keep the button positions distinct even if the whole drag arrives in one frame.
                let events = ui.input(|input| input.events.iter().filter(|event| matches!(event,
                    egui::Event::PointerButton { button: egui::PointerButton::Primary, .. }
                        | egui::Event::PointerMoved(_) | egui::Event::PointerGone
                )).cloned().collect::<Vec<_>>());
                for event in events {
                    match event {
                        egui::Event::PointerButton { pos, pressed: true, modifiers, .. }
                            if response.enabled() && response.interact_rect.contains(pos)
                                && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
                                && !egui::Popup::is_any_open(ui.ctx()) =>
                        {
                            self.pen_press = Some(pos);
                            self.pen.alt = modifiers.alt;
                            self.pen.shift = modifiers.shift;
                            let point = to_doc(pos);
                            let snapped = self.snap_free(session, point);
                            let point = self.pen.start_at(session, scene, point, snapped, near);
                            let close = self.pen.press(point, near) == crate::canvas_curves::PenStep::Close;
                            if close || self.pen.line_done() {
                                let command = self.pen.finish_command(close);
                                changed |= self.run(session, command);
                                self.pen_press = None;
                            }
                        }
                        egui::Event::PointerMoved(pos) if self.pen_press.is_some_and(|start| start.distance(pos) > 0.5) => {
                            self.pen.drag_with_construction(to_doc(pos), construction, tolerance);
                        }
                        egui::Event::PointerButton { pos, pressed: false, modifiers, .. } => {
                            if self.pen_press.take().is_some_and(|start| start.distance(pos) > 0.5) {
                                self.pen.alt = modifiers.alt;
                                self.pen.shift = modifiers.shift;
                                self.pen.drag_with_construction(to_doc(pos), construction, tolerance);
                            }
                            self.pen.release();
                        }
                        egui::Event::PointerGone => {
                            self.pen_press = None;
                            self.pen.release();
                        }
                        _ => {}
                    }
                }
                if response.double_clicked() && self.pen.active() {
                    self.pen.drop_duplicate_tail(near);
                    let command = self.pen.finish_command(false);
                    changed |= self.run(session, command);
                }
                if enter && self.pen.active() {
                    let command = self.pen.finish_command(false);
                    self.pen_press = None;
                    changed |= self.run(session, command);
                }
            }
            ToolKind::Pencil | ToolKind::VectorBrush => {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                let px = 1.0 / view.zoom.max(1e-4);
                let (down, up) = primary_button_positions(ui);
                if let Some(at) = down.filter(|at| response.interact_rect.contains(*at)
                    && ui.ctx().layer_id_at(*at) == Some(response.layer_id)
                    && !egui::Popup::is_any_open(ui.ctx()))
                {
                    let at = to_doc(at);
                    let snapped = self.snap_free(session, at);
                    if kind == ToolKind::VectorBrush {
                        self.pencil.begin_brush(session, snapped);
                    } else {
                        self.pencil.begin(session, scene, at, snapped, near);
                    }
                }
                if self.pencil.active() {
                    ui.input(|input| {
                        let mut drawing = down.is_none();
                        for event in &input.events {
                            match event {
                                egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed, .. } => {
                                    if !pressed { break; }
                                    drawing = true;
                                }
                                egui::Event::PointerMoved(at) if drawing => self.pencil.drag(to_doc(*at), 0.5 * px),
                                _ => {}
                            }
                        }
                    });
                    if let Some(at) = up {
                        self.pencil.drag(to_doc(at), 0.5 * px);
                        let command = self.pencil.finish_command(1.5 * px, near);
                        changed = self.run(session, command);
                    } else if !primary {
                        self.pencil.cancel();
                    }
                }
            }
            ToolKind::Knife => {
                if response.hovered() || self.knife.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                let (press_event, release_event) = primary_button_positions(ui);
                let captures_press = response.contains_pointer() || response.is_pointer_button_down_on() || response.drag_started();
                if self.knife.is_none() && !session.selection.ids.is_empty() && captures_press {
                    let origin = press_event.or_else(|| response.drag_started().then_some(press_origin).flatten());
                    if let Some(origin) = origin.filter(|p| response.interact_rect.contains(*p)) {
                        let freehand = session.tool_options.choice("tool.knife", "knife.mode").as_deref() == Some("freehand");
                        self.knife = Some(KnifeGesture { freehand, targets: session.selection.ids.clone(), points: vec![to_doc(origin)] });
                    }
                }
                if let Some(knife) = &mut self.knife {
                    let doc_per_pixel = 1.0 / view.zoom.max(1e-4);
                    if knife.freehand {
                        ui.input(|i| {
                            let mut tracking = press_event.is_none();
                            for event in &i.events {
                                match event {
                                    egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, .. } => tracking = true,
                                    egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, .. } => break,
                                    egui::Event::PointerMoved(pos) if tracking => knife.add_point(to_doc(*pos), 0.5 * doc_per_pixel),
                                    _ => {}
                                }
                            }
                        });
                    }
                    if let Some(p) = release_event.map(to_doc).or_else(|| point.filter(|_| !knife.freehand)) {
                        knife.add_point(p, 0.0);
                    }
                    if released {
                        let click = response.clicked_by(egui::PointerButton::Primary);
                        let command = self.knife.take().and_then(|knife| knife.command(2.0 * doc_per_pixel, click, 4.0 * doc_per_pixel));
                        changed = self.run(session, command);
                    } else if !primary {
                        self.knife = None;
                    }
                    ui.ctx().request_repaint();
                }
            }
            ToolKind::ShapeBuilder => {
                let stamp = self.owner_of(session);
                let command = self.shape_builder.input(ui, response, session, scene, stamp, to_doc);
                changed = self.run(session, command);
            }
            ToolKind::PointTransform => {
                if session.selection.ids.is_empty() {
                    self.point_transform.clear();
                    if response.clicked() {
                        let hit = point.and_then(|point| hit_test_in_view(scene, session, point, near, self.scope));
                        changed = self.run(session, click_select_command(hit, &session.selection.ids, mods.shift));
                    }
                } else {
                    let (preview, command) = self.point_transform.input(ui, response, scene, session, self.context, to_doc);
                    self.preview = preview;
                    changed = self.run(session, command);
                }
            }
            ToolKind::StylePicker => {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                let (press, release) = primary_button_positions(ui);
                let captures_press = response.contains_pointer() || response.is_pointer_button_down_on();
                if self.style_picker_loaded() && !mods.alt && captures_press {
                    if let Some(at) = press.filter(|p| response.interact_rect.contains(*p)) {
                        self.style_drag = Some(to_doc(at));
                    }
                }
                let end = release.map(to_doc).or(point);
                if let (Some([x, y]), Some([end_x, end_y])) = (self.style_drag, end) {
                    let [w, h] = [(end_x - x).abs(), (end_y - y).abs()];
                    self.marquee = (w.hypot(h) > 3.0 / view.zoom.max(1e-4))
                        .then_some([x.min(end_x), y.min(end_y), w, h]);
                    ui.ctx().request_repaint();
                }
                if released {
                    self.style_drag = None;
                    if let Some(area) = self.marquee.take() {
                        let mode = if mods.ctrl { MarqueeMode::Enclosed } else { MarqueeMode::Touch };
                        let targets = selection_marquee_with_flags(&session.document, area, mode, SelectOp::Replace, &session.selection, &view.canvas_flags)
                            .ids.into_iter().filter(|id| scene.quad(*id).is_some()).map(|id| id.0).collect();
                        changed = self.apply_picked_style(session, targets);
                    } else if response.clicked_by(egui::PointerButton::Primary) {
                        if let Some(hit) = end.and_then(|p| self.hit(scene, session, p)) {
                            changed = self.style_picker_click(session, hit, mods.alt);
                            ui.ctx().request_repaint();
                        }
                    }
                } else if !primary {
                    self.style_drag = None;
                    self.marquee = None;
                }
            }
            ToolKind::Measure => self.measure.input(ui, response, session, to_doc),
            ToolKind::Blend => {
                let scope = self.scope;
                let command = self.blend.input(ui, response, session, to_doc, |point| {
                    hit_test_in_view(scene, session, point, near, scope)
                });
                changed = self.run(session, command);
            }
            ToolKind::Area => {
                let (marquee, command) = self.area.input(ui, response, session, scene, mods, to_doc);
                self.marquee = marquee;
                changed = self.run(session, command);
            }
            ToolKind::Node => {
                let (press_event, release_event) = primary_button_positions(ui);
                let captures_press = response.contains_pointer() || response.is_pointer_button_down_on();
                let pressed_node = press_event.filter(|pos| captures_press && response.interact_rect.contains(*pos));
                self.nodes.sync(session);
                let focused = ui.ctx().memory(|m| m.focused().is_some());
                let delete = !focused && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
                self.nodes.alt = mods.alt;
                self.nodes.shift = mods.shift;
                self.nodes.ctrl = mods.ctrl;
                self.nodes.transform_mode = session.tool_options.flag("tool.node", "node.transform_mode").unwrap_or(false);
                if delete && self.nodes.claims_delete() && self.nodes.grab.is_none() {
                    for command in self.nodes.delete_commands(session) {
                        changed |= self.run(session, Some(command));
                    }
                    self.nodes.sync(session);
                }
                if double && response.hovered() && !self.nodes.primitive && !self.nodes.dragging_transform_handle() {
                    if let Some(p) = release_event.map(to_doc).or(point) {
                        let command = self.nodes.double_click(session, p, near, 0.25 / f64::from(view.zoom.max(1e-4)));
                        changed |= self.run(session, command);
                    }
                } else if let Some(screen_point) = pressed_node {
                    let p = to_doc(screen_point);
                    let result = if self.nodes.primitive { crate::canvas_curves::NodePress::Miss }
                        else { self.nodes.press(session, p, mods.shift, mods.alt, near) };
                    match result {
                        crate::canvas_curves::NodePress::Grab => {}
                        crate::canvas_curves::NodePress::Run(command) => changed = self.run(session, Some(command)),
                        crate::canvas_curves::NodePress::Miss => {
                            let hit = self.hit(scene, session, p);
                            if let Some(hit) = hit.filter(|h| !session.selection.ids.contains(h)) {
                                let op = if mods.shift { "Add" } else { "Replace" };
                                let command = Some(Command::with_params("selection.set", json!({ "ids": [hit.0], "op": op })));
                                changed = self.run(session, command);
                                self.nodes.sync(session);
                            } else if self.nodes.target.is_some() && !self.nodes.primitive {
                                self.nodes.marquee = Some((screen_point, screen_point, mods.shift));
                            }
                        }
                    }
                }
                if let Some((start, end, add)) = self.nodes.marquee {
                    let end = release_event.or(pointer).unwrap_or(end);
                    if release_event.is_some() || released {
                        self.nodes.marquee = None;
                        if start.distance(end) >= 3.0 {
                            self.nodes.select_rect(session, egui::Rect::from_two_pos(start, end), add, |point| {
                                screen(rect, &view, point)
                            });
                        } else if !add {
                            self.nodes.clear_selection();
                        }
                    } else if primary {
                        self.nodes.marquee = Some((start, end, add));
                        ui.ctx().request_repaint();
                    } else {
                        self.nodes.marquee = None;
                    }
                }
                if self.nodes.grab.is_some() {
                    use crate::canvas_curves::NodeAnchorSnap;
                    self.snap_axes = [None; 2];
                    if let Some(p) = release_event.map(to_doc).or(point) {
                        let exclude: Vec<_> = self.nodes.drag_targets().collect();
                        let tolerance = session.snap_prefs.query().screen_tolerance as f32 / view.zoom.max(1e-4);
                        let (p, mark) = self.nodes.snap_transform_handle(session, p).or_else(|| {
                            self.nodes.snap_handle(session, p, tolerance)
                        }).unwrap_or_else(|| {
                            let align = session.tool_options.flag("tool.node", "node.align_to_nodes").unwrap_or(false);
                            let geometry = session.tool_options.flag("tool.node", "node.snap_to_geometry").unwrap_or(false);
                            let alignment = if align || geometry {
                                let all = session.tool_options.flag("tool.node", "node.snap_all_selected").unwrap_or(false);
                                self.nodes.snap_anchors(session, p, tolerance, align, geometry, all)
                            } else {
                                None
                            };
                            if let Some(NodeAnchorSnap::Point { offset, target }) = alignment {
                                return ([p[0] + offset[0], p[1] + offset[1]], Some(target));
                            }
                            let (global, mark) = match self.nodes.moving_points(p) {
                                Some(points) => match crate::canvas_tools::snap_offset(session, &points, &exclude) {
                                    Some((offset, mark)) => ([p[0] + offset[0], p[1] + offset[1]], Some(mark)),
                                    None => (p, None),
                                },
                                None => crate::canvas_tools::snap_point(session, p, &exclude),
                            };
                            if let Some(NodeAnchorSnap::Axes(axes)) = alignment {
                                self.snap_axes = axes.map(|axis| axis.map(|(_, target)| target));
                                let point = std::array::from_fn(|i| axes[i].map_or(global[i], |(offset, _)| p[i] + offset));
                                (point, None)
                            } else {
                                (global, mark)
                            }
                        });
                        self.snap_mark = mark;
                        self.nodes.drag(p);
                    }
                }
                if released && self.nodes.grab.is_some() {
                    let command = self.nodes.release();
                    self.snap_axes = [None; 2];
                    changed = self.run(session, command);
                }
                if let Some(point) = point.filter(|_| response.hovered() || self.nodes.grab.is_some())
                    && let Some(part) = self.nodes.pointer_part(session, point, near)
                {
                    use crate::canvas_curves::NodePart;
                    let cursor = match part {
                        NodePart::Scale => egui::CursorIcon::ResizeNwSe,
                        NodePart::Rotate => egui::CursorIcon::Alias,
                        NodePart::Shear(side) if side.axis() == 0 => egui::CursorIcon::ResizeColumn,
                        NodePart::Shear(_) => egui::CursorIcon::ResizeRow,
                        NodePart::Align(handle) if handle.axis == 0 => egui::CursorIcon::ResizeHorizontal,
                        NodePart::Align(_) => egui::CursorIcon::ResizeVertical,
                        _ if self.nodes.grab.is_some() => egui::CursorIcon::Grabbing,
                        _ => egui::CursorIcon::Grab,
                    };
                    ui.ctx().set_cursor_icon(cursor);
                }
                if response.secondary_clicked() {
                    if let Some(p) = point {
                        if !self.nodes.primitive { self.nodes.select_at(session, p, near); }
                    }
                }
                let mut picked = None;
                if crate::canvas_curves::node_context_menu(session) {
                    response.context_menu(|ui| crate::menu_tree::context_command_menu(ui, |ui| {
                        for (kind, label) in crate::canvas_curves::NODE_CONVERSION_CHOICES {
                            let command = self.nodes.convert_selected(session, kind);
                            if crate::menu_tree::command_menu_item(ui, dtool_strings::text(label), command.is_some()).clicked() {
                                picked = command;
                                ui.close();
                            }
                        }
                        ui.separator();
                        for (label, command) in [
                            ("ctx.node.split_after", self.nodes.split_after_node_command(session)),
                            ("ctx.node.break", self.nodes.break_command(session)),
                            ("ctx.node.join", crate::canvas_curves::join_command(session)),
                            ("ctx.node.close", self.nodes.close_command(session)),
                            ("ctx.node.smooth", self.nodes.smooth_command(session)),
                        ] {
                            if crate::menu_tree::command_menu_item(ui, dtool_strings::text(label), command.is_some()).clicked() {
                                picked = command;
                                ui.close();
                            }
                        }
                    }));
                }
                if picked.is_some() {
                    changed |= self.run(session, picked);
                    self.nodes.sync(session);
                }
            }
            ToolKind::StrokeWidth => {
                let command = self.stroke_width.input(ui, response, session, scene, to_doc, keyboard);
                changed = self.run(session, command);
            }
            ToolKind::PixelBrush(op) => {
                let command = self.pixel_brush.input(ui, response, session, op, to_doc);
                changed = self.run(session, command);
            }
            ToolKind::RedEye => {
                let target = dtool_core::commands::red_eye::canvas_target(session);
                if response.hovered() {
                    ui.ctx().set_cursor_icon(if target.is_some() { egui::CursorIcon::Crosshair } else { egui::CursorIcon::NotAllowed });
                }
                let (down, up) = crate::input::primary_button_positions(ui);
                if let Some(target) = target
                    && let Some(start) = down.filter(|pos| {
                        response.enabled() && response.interact_rect.contains(*pos)
                            && ui.ctx().layer_id_at(*pos) == Some(response.layer_id)
                            && !egui::Popup::is_any_open(ui.ctx())
                    })
                {
                    let start = to_doc(start);
                    self.drag = Some(Drag::RedEye { target, start, current: start });
                    response.request_focus();
                }
                if let (Some(Drag::RedEye { current, .. }), Some(pos)) = (self.drag.as_mut(), up.or(pointer)) {
                    *current = to_doc(pos);
                }
                if let Some(Drag::RedEye { current, .. }) = self.drag {
                    if up.is_some() {
                        changed = self.finish_drag(session, current, mods);
                    } else if !primary {
                        self.drag = None;
                    }
                }
            }
            ToolKind::PixelFlood { erase } => {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                if response.clicked_by(egui::PointerButton::Primary)
                    && !egui::Popup::is_any_open(ui.ctx())
                    && let Some(position) = response.interact_pointer_pos()
                {
                    let command = dtool_core::commands::fill::canvas_command(session, erase, to_doc(position));
                    changed = self.run(session, Some(command));
                }
            }
            ToolKind::PixelMarquee(kind) => {
                let command = self.pixel_marquee.input(ui, response, session, kind, to_doc);
                changed = self.run(session, command);
            }
            ToolKind::FrameText | ToolKind::Table | ToolKind::Other => {}
        }
        if self.gesture_active() {
            self.last_point = point.or(self.last_point);
            if self.owner.is_none() {
                self.claim(session);
            }
        } else {
            self.owner = None;
            self.last_point = None;
        }
        if !self.busy() {
            self.snap_mark = None;
            self.guide_lines.clear();
            self.gap_guides.clear();
            self.snap_axes = [None; 2];
        }
        if changed {
            ui.ctx().request_repaint();
        }
        if paint_overlay {
            self.paint_preview(ui, rect, session, scene);
        }
    }

    pub fn paint_preview(&self, ui: &Ui, rect: Rect, session: &Session, scene: &Scene) {
        if session.view.canvas_flags.preview_mode && !self.busy() && self.pen.nodes.is_empty() {
            return;
        }
        let painter = ui.painter_at(rect);
        let view = &session.view;
        let accent = if tool_kind(&session.active_tool) == ToolKind::Node { SELECTION_BLUE } else { dtool_design::color("accent") };
        let stroke = Stroke::new(dtool_design::stroke_width("md"), accent);
        self.pixel_brush.paint(ui, rect, session);
        self.pixel_marquee.paint(ui, rect, session);
        self.paint_artboard_drag(ui, rect, session, stroke);
        if !session.view.canvas_flags.preview_mode
            && session.tool_options.flag(&session.active_tool, dtool_core::context_toolbar::CURVE_ENERGY) == Some(true)
        {
            curve_energy::paint(&painter, session, scene, &self.nodes, |point| screen(rect, view, point), accent);
        }
        self.warp_mesh.paint(ui, rect, session);
        if tool_kind(&session.active_tool) == ToolKind::ShapeBuilder {
            self.shape_builder.paint(ui, rect, session, stroke);
        }
        if tool_kind(&session.active_tool) == ToolKind::PointTransform {
            self.point_transform.paint(ui, rect, session, scene, stroke, self.preview.as_ref());
        }
        if tool_kind(&session.active_tool) == ToolKind::Measure {
            self.measure.paint(ui, rect, session, stroke);
        }
        if tool_kind(&session.active_tool) == ToolKind::Area {
            self.area.paint(ui, rect, session, stroke);
        }
        if tool_kind(&session.active_tool) == ToolKind::Blend {
            self.blend.paint(ui, rect, session, stroke);
        }
        if tool_kind(&session.active_tool) == ToolKind::StrokeWidth {
            self.stroke_width.paint(ui, rect, session, stroke);
        }
        if matches!(tool_kind(&session.active_tool), ToolKind::Gradient(_) | ToolKind::Transparency)
            && !session.quick_mask.active
            && !matches!(self.drag, Some(Drag::Gradient { .. }))
        {
            self.gradient_edit.paint(ui, rect, session, stroke);
        }
        if tool_kind(&session.active_tool) == ToolKind::VectorCrop {
            self.vector_crop.paint(ui, rect, session, scene, stroke);
        }
        if tool_kind(&session.active_tool) == ToolKind::Slice {
            let border = Stroke::new(1.0, crate::canvas_input::SELECTION_BLUE);
            let corners = |[x, y, right, bottom]: [f32; 4]| {
                [[x, y], [right, y], [right, bottom], [x, bottom]].map(|point| screen(rect, view, point))
            };
            for slice in &session.document.slices {
                if let Ok(bounds) = slice.resolved_bounds(&session.document) {
                    let points = corners(bounds);
                    let selected = session.active_slice == Some(slice.id);
                    let line = if selected { Stroke::new(2.0, border.color) } else { border };
                    painter.add(egui::Shape::closed_line(points.to_vec(), line));
                    if selected {
                        for corner in points {
                            painter.rect_filled(Rect::from_center_size(corner, egui::vec2(6.0, 6.0)), 1.0, line.color);
                        }
                    }
                    painter.text(points[0] + egui::vec2(4.0, 3.0), egui::Align2::LEFT_TOP, &slice.name,
                        egui::FontId::proportional(11.0), border.color);
                }
            }
            if let Some(Drag::Slice { start, current, edit }) = self.drag {
                let bounds = edit.map_or([start[0], start[1], current[0], current[1]], |edit| edit.bounds_at(start, current));
                let points = corners(bounds);
                painter.add(egui::Shape::convex_polygon(points.to_vec(), border.color.gamma_multiply(0.10), border));
            }
        }
        let area = |a: [f32; 2], b: [f32; 2]| Rect::from_two_pos(screen(rect, view, a), screen(rect, view, b));
        if let Some(m) = self.marquee {
            let r = area([m[0], m[1]], [m[0] + m[2], m[1] + m[3]]);
            painter.rect_filled(r, 0.0, accent.gamma_multiply(0.15));
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        }
        if let Some(knife) = &self.knife {
            let opts = &session.tool_options;
            let auto_close = opts.flag("tool.knife", "knife.auto_close").unwrap_or(true);
            let smoothness = opts.number("tool.knife", "knife.smoothness").unwrap_or(0.0);
            let stabiliser = opts.number("tool.knife", "knife.stabiliser").unwrap_or(0.0);
            let cut = knife.points.iter().map(|p| p.map(f64::from)).collect();
            let cut = dtool_core::context_toolbar::prepare_knife_cut(cut, auto_close, smoothness, stabiliser);
            let points: Vec<Pos2> = cut.into_iter().map(|p| screen(rect, view, p.map(|v| v as f32))).collect();
            if points.len() >= 2 {
                painter.extend(egui::Shape::dashed_line(&points, stroke, 5.0, 3.0));
            }
        }
        match self.drag {
            Some(Drag::Shape { kind, start, current }) => {
                let mods = ui.input(|i| Mods { shift: i.modifiers.shift, ctrl: i.modifiers.command, alt: i.modifiers.alt });
                if let Some(command) = configured_shape_command(session, kind, start, current, mods)
                    && let Ok(pos) = serde_json::from_value::<[f32; 2]>(command.params["pos"].clone())
                    && let Ok(recipe) = serde_json::from_value::<dtool_core::shape::ShapeRecipe>(command.params)
                {
                    let object = dtool_core::Object { pos, size: recipe.size, shape: Some(recipe), ..Default::default() };
                    let tolerance = 0.1 / f64::from((view.zoom * ui.ctx().pixels_per_point()).max(1e-4));
                    for (points, closed) in dtool_core::outline::object_contours(&object, tolerance) {
                        let points = points.into_iter().map(|p| screen(rect, view, [p[0] + pos[0], p[1] + pos[1]])).collect();
                        painter.add(if closed { egui::Shape::closed_line(points, stroke) } else { egui::Shape::line(points, stroke) });
                    }
                }
            }
            Some(Drag::PictureFrame { shape: FrameShape::Rect, start, current }) => {
                painter.rect_stroke(area(start, current), 0.0, stroke, egui::StrokeKind::Inside);
            }
            Some(Drag::PictureFrame { shape: FrameShape::Ellipse, start, current }) => {
                let r = area(start, current);
                painter.add(egui::Shape::ellipse_stroke(r.center(), r.size() / 2.0, stroke));
            }
            Some(Drag::RedEye { start, current, .. }) => {
                let centre = [(start[0] + current[0]) * 0.5, (start[1] + current[1]) * 0.5];
                let radius = [(current[0] - start[0]).abs() * 0.5, (current[1] - start[1]).abs() * 0.5];
                let outline = (0..64).map(|step| {
                    let (sin, cos) = (std::f32::consts::TAU * step as f32 / 64.0).sin_cos();
                    screen(rect, view, [centre[0] + cos * radius[0], centre[1] + sin * radius[1]])
                }).collect();
                painter.add(egui::Shape::closed_line(outline, Stroke::new(1.0, SELECTION_BLUE)));
            }
            Some(Drag::ArtText { start, current }) if self.art_selection.is_none() && (current[1] - start[1]).abs() * view.zoom >= 4.0 => {
                let scale = dtool_core::units::units_per_point(session.document.units, session.document.dpi);
                let command = art_text_command(start, current, scale);
                let size = command.params["size_pt"].as_f64().unwrap_or(12.0);
                let top = screen(rect, view, [start[0].min(current[0]), start[1].min(current[1])]);
                let height = (size as f32 * scale * view.zoom).max(1.0);
                let width = height * 0.65;
                let preview = Stroke::new((height * 0.08).clamp(1.0, 18.0), egui::Color32::from_black_alpha(87));
                painter.add(egui::Shape::line(vec![top + egui::vec2(0.0, height), top + egui::vec2(width * 0.5, 0.0), top + egui::vec2(width, height)], preview));
                painter.line_segment([top + egui::vec2(width * 0.2, height * 0.6), top + egui::vec2(width * 0.8, height * 0.6)], preview);
                let badge = egui::Rect::from_min_size(top + egui::vec2(width + 8.0, 2.0), egui::vec2(60.0, 20.0));
                painter.rect_filled(badge, 3.0, egui::Color32::from_black_alpha(180));
                painter.text(badge.center(), egui::Align2::CENTER_CENTER, format!("{size:.1} pt"), egui::FontId::proportional(12.0), egui::Color32::WHITE);
            }
            Some(Drag::Crop { start, current, .. }) => {
                painter.rect_stroke(area(start, current), 0.0, stroke, egui::StrokeKind::Inside);
            }
            Some(Drag::Line { start, current }) => {
                painter.line_segment([screen(rect, view, start), screen(rect, view, current)], stroke);
            }
            Some(Drag::Gradient { start, current, .. } | Drag::MaskGradient { start, current, .. }) => {
                let (a, b) = (screen(rect, view, start), screen(rect, view, current));
                painter.line_segment([a, b], stroke);
                painter.circle(a, HANDLE_PX * 0.75, egui::Color32::WHITE, stroke);
                painter.circle(b, HANDLE_PX * 0.75, egui::Color32::WHITE, stroke);
            }
            Some(Drag::ArtText { .. } | Drag::ShapeParam { .. } | Drag::Slice { .. }) | None => {}
        }
        if let Some(object) = editable_shape_params(session) {
            let marker = egui::Color32::from_rgb(181, 80, 54);
            let preview = match self.drag {
                Some(Drag::ShapeParam { target, kind, current, grab_offset, .. }) if target == object.id => {
                    shape_tool::preview_recipe_drag(object, kind, [current[0] + grab_offset[0], current[1] + grab_offset[1]])
                }
                _ => None,
            };
            if let Some(path) = preview.as_ref().and_then(dtool_core::outline::object_outline_world) {
                let tolerance = 0.5 / f64::from(view.zoom.max(1e-4));
                for (points, closed) in dtool_core::outline::flatten_subpaths(&path, tolerance) {
                    let points = points.into_iter().map(|p| screen(rect, view, crate::canvas_curves::p32(p))).collect();
                    let outline = Stroke::new(1.25, marker);
                    painter.add(if closed { egui::Shape::closed_line(points, outline) } else { egui::Shape::line(points, outline) });
                }
            }
            for (kind, at) in shape_tool::param_handles(preview.as_ref().unwrap_or(object)) {
                let pos = match self.drag {
                    Some(Drag::ShapeParam { target, kind: dragging, current, grab_offset, .. })
                        if target == object.id && dragging == kind && !matches!(kind, ShapeParamKind::Recipe(_) | ShapeParamKind::Corner(_)) =>
                            [current[0] + grab_offset[0], current[1] + grab_offset[1]],
                    _ => at,
                };
                let at = screen(rect, view, pos);
                if matches!(kind, ShapeParamKind::Recipe(_) | ShapeParamKind::Corner(_)) {
                    let r = HANDLE_PX * 0.65;
                    painter.add(egui::Shape::convex_polygon(
                        vec![at + egui::vec2(0.0, -r), at + egui::vec2(r, 0.0), at + egui::vec2(0.0, r), at + egui::vec2(-r, 0.0)],
                        if matches!(kind, ShapeParamKind::Corner(_)) { egui::Color32::from_rgb(225, 58, 58) } else { marker },
                        Stroke::new(1.0, egui::Color32::WHITE),
                    ));
                } else {
                    painter.circle(at, HANDLE_PX * 0.75, egui::Color32::WHITE, stroke);
                }
            }
        }
        if let Some(scope) = self.scope {
            let children: Vec<ObjectId> = session.document.objects.iter().filter(|o| !o.is_layer_folder() && dtool_core::scene_hit::pick_parent(&session.document, o.id) == Some(scope)).map(|o| o.id).collect();
            if let Some(b) = selection_box(scene, &session.document, &children) {
                let r = area([b[0], b[1]], [b[0] + b[2], b[1] + b[3]]);
                let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(dtool_design::stroke_width("md"), accent.gamma_multiply(0.7)), 4.0, 3.0));
            }
        }
        let tolerance = 0.5 / f64::from(view.zoom.max(1e-4));
        let curve = |contour: &dtool_core::curve::Contour| {
            for (points, closed) in crate::canvas_curves::flatten(contour, tolerance) {
                let pts: Vec<Pos2> = points.iter().map(|p| screen(rect, view, *p)).collect();
                painter.add(if closed { egui::Shape::closed_line(pts, stroke) } else { egui::Shape::line(pts, stroke) });
            }
        };
        let node_box = |p: [f64; 2], filled: bool| {
            let r = Rect::from_center_size(screen(rect, view, crate::canvas_curves::p32(p)), egui::vec2(HANDLE_PX, HANDLE_PX));
            painter.rect_filled(r, 0.0, if filled { accent } else { egui::Color32::WHITE });
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        };
        let handles = |node: &dtool_core::curve::PathPoint| {
            let at = screen(rect, view, crate::canvas_curves::p32(node.pos));
            for h in [node.handle_in, node.handle_out].into_iter().flatten() {
                let hp = screen(rect, view, crate::canvas_curves::p32(h));
                painter.line_segment([at, hp], Stroke::new(1.0, accent));
                painter.circle(hp, HANDLE_PX * 0.5, egui::Color32::WHITE, stroke);
            }
        };
        if let Some(contour) = self.pencil.preview() {
            if let Some((style, opacity, _)) = self.pencil.brush()
                && let Some(rgba) = dtool_core::scene::representative_colour(&style.paint)
            {
                let width = style.width * view.zoom;
                let alpha = (f32::from(rgba.a) * opacity).round() as u8;
                let colour = egui::Color32::from_rgba_unmultiplied(rgba.r, rgba.g, rgba.b, alpha);
                let path: Vec<_> = contour.points.iter().map(|node| screen(rect, view, crate::canvas_curves::p32(node.pos))).collect();
                painter.add(egui::Shape::line(path, Stroke::new(width, colour)));
            } else {
                curve(&contour);
            }
        }
        if let Some(contour) = self.pen.preview(self.hover.filter(|_| self.pen.rubber_band || self.pen.mode == crate::canvas_curves::PenMode::Line)) {
            curve(&contour);
        }
        if self.pen.mode == crate::canvas_curves::PenMode::BSpline && self.pen.nodes.len() >= 2 {
            let points: Vec<_> = self.pen.nodes.iter().map(|node| screen(rect, view, crate::canvas_curves::p32(node.pos))).collect();
            painter.extend(egui::Shape::dashed_line(&points, Stroke::new(1.0, accent.gamma_multiply(0.45)), 4.0, 3.0));
        }
        for (i, node) in self.pen.nodes.iter().enumerate() {
            node_box(node.pos, i == 0);
            if i + 1 == self.pen.nodes.len() {
                handles(node);
            }
        }
        if tool_kind(&session.active_tool) == ToolKind::Node {
            if let Some((start, end, _)) = self.nodes.marquee.filter(|(start, end, _)| start.distance(*end) >= 3.0) {
                let area = egui::Rect::from_two_pos(start, end);
                painter.rect_filled(area, 0.0, accent.gamma_multiply(0.15));
                painter.rect_stroke(area, 0.0, stroke, egui::StrokeKind::Inside);
            }
            for (id, contours) in self.nodes.other_curves(session) {
                let contours = self.nodes.live_other_contours(id).unwrap_or(contours);
                for (c, contour) in contours.iter().enumerate() {
                    curve(contour);
                    for node in &contour.points {
                        let selected = self.nodes.other_node_selected(id, c, node.id);
                        if selected {
                            handles(node);
                        }
                        node_box(node.pos, selected);
                    }
                }
            }
            if !self.nodes.primitive && let Some(contours) = self.nodes.contours(session) {
                let orientation = session.tool_options.flag("tool.node", "node.show_orientation").unwrap_or(false);
                for (c, contour) in contours.iter().enumerate() {
                    curve(contour);
                    if orientation && let [.., from, to] = contour.points.as_slice() {
                        let project = |point| screen(rect, view, crate::canvas_curves::p32(point));
                        let red = Stroke::new(1.5, egui::Color32::from_rgb(232, 71, 71));
                        let a = project(from.pos);
                        let b = project(to.pos);
                        if from.handle_out.is_some() || to.handle_in.is_some() {
                            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                                [a, project(from.handle_out.unwrap_or(from.pos)), project(to.handle_in.unwrap_or(to.pos)), b],
                                false, egui::Color32::TRANSPARENT, red,
                            ));
                        } else {
                            painter.line_segment([a, b], red);
                        }
                        painter.rect_stroke(egui::Rect::from_center_size(b, egui::Vec2::splat(HANDLE_PX + 4.0)),
                            0.0, red, egui::StrokeKind::Middle);
                    }
                    for (i, node) in contour.points.iter().enumerate() {
                        let selected = self.nodes.selected.contains(&(c, i));
                        if selected {
                            handles(node);
                        }
                        node_box(node.pos, selected);
                    }
                }
                let transform_box = self.nodes.transform_box(session, &contours);
                let hide_box = self.nodes.hide_transform_box(session);
                if let Some(b) = transform_box {
                    use crate::canvas_curves::{NodeEdit, NodePart};
                    let pointer = ui.input(|input| input.pointer.hover_pos()).filter(|pos| rect.contains(*pos));
                    let radius = HANDLE_PX / view.zoom.max(1e-4);
                    for (alignment, grip) in NodeEdit::alignment_handles(session, b, radius) {
                        let grip = screen(rect, view, self.nodes.transform_display_point(session, grip));
                        let active = match self.nodes.grab {
                            Some((part, _)) => part == NodePart::Align(alignment),
                            None => pointer.is_some_and(|pos| pos.distance(grip) <= HANDLE_PX),
                        };
                        let horizontal_motion = alignment.axis == 0;
                        if active {
                            let (sin, cos) = view.rotation_deg.to_radians().sin_cos();
                            let tangent = if horizontal_motion { egui::vec2(-sin, cos) } else { egui::vec2(cos, sin) };
                            let span = tangent * rect.size().length();
                            painter.line_segment([grip - span, grip + span], Stroke::new(1.0, accent.gamma_multiply(0.75)));
                        }
                        if !hide_box {
                            let size = if horizontal_motion { egui::vec2(9.0, 5.0) } else { egui::vec2(5.0, 9.0) };
                            let handle = Rect::from_center_size(grip, size);
                            painter.rect_filled(handle, 2.0, if active { accent } else { egui::Color32::WHITE });
                            painter.rect_stroke(handle, 2.0, stroke, egui::StrokeKind::Middle);
                        }
                    }
                }
                if let Some(b @ [left, top, right, bottom]) = transform_box.filter(|_| !hide_box) {
                    let project = |point| screen(rect, view, self.nodes.transform_display_point(session, point));
                    let corners = [[left, top], [right, top], [right, bottom], [left, bottom]].map(project);
                    let outline = Stroke::new(1.0, accent);
                    painter.add(egui::Shape::closed_line(corners.to_vec(), outline));
                    let handle = egui::Rect::from_center_size(corners[2], egui::Vec2::splat(HANDLE_PX));
                    painter.rect_filled(handle, 0.0, egui::Color32::WHITE);
                    painter.rect_stroke(handle, 0.0, stroke, egui::StrokeKind::Middle);
                    let rotate = project(crate::canvas_curves::node_rotate_handle(b, ROTATE_PX / view.zoom.max(1e-4)));
                    painter.line_segment([corners[0].lerp(corners[1], 0.5), rotate], outline);
                    painter.circle(rotate, HANDLE_PX * 0.5, egui::Color32::WHITE, stroke);
                    for (_, grip) in crate::canvas_curves::node_shear_handles(b, HANDLE_PX / view.zoom.max(1e-4)) {
                        let centre = project(grip);
                        let radius = HANDLE_PX * 0.62;
                        let diamond = [[-radius, 0.0], [0.0, -radius], [radius, 0.0], [0.0, radius]]
                            .into_iter().map(|[x, y]| centre + egui::vec2(x, y)).collect();
                        painter.add(egui::Shape::convex_polygon(diamond, egui::Color32::WHITE, stroke));
                    }
                    if let Some(origin) = self.nodes.transform_origin(session, b) {
                        let origin = screen(rect, view, origin);
                        painter.circle_filled(origin, 6.0, egui::Color32::from_black_alpha(220));
                        crate::context_bar::paint_transform_origin(&painter, origin, accent, 4.0);
                    }
                    for origin in self.nodes.displayed_individual_origins(session, &contours) {
                        let origin = screen(rect, view, origin);
                        painter.circle_filled(origin, 6.0, egui::Color32::from_black_alpha(220));
                        crate::context_bar::paint_transform_origin(&painter, origin, accent, 4.0);
                    }
                }
            }
        }
        if let Some(snap) = self.nodes.construction_snap.or(self.pen.construction_snap) {
            let project = |point| screen(rect, view, point);
            let color = dtool_design::color("smart_guide");
            let moving = snap.moving.map(project);
            let stroke = Stroke::new(1.0, color);
            if let Some(triangle) = snap.triangle {
                let [anchor, corner, neighbor] = triangle.map(project);
                painter.extend(egui::Shape::dashed_line(
                    &[anchor, corner, neighbor, anchor], stroke, 4.0, 3.0,
                ));
                let a = anchor - corner;
                let b = neighbor - corner;
                let mark_size = (a.length().min(b.length()) * 0.25).min(7.0);
                let a = a.normalized() * mark_size;
                let b = b.normalized() * mark_size;
                painter.add(egui::Shape::line(vec![corner + a, corner + a + b, corner + b], stroke));
            } else {
                painter.extend(egui::Shape::dashed_line(&snap.reference.map(project), stroke, 4.0, 3.0));
            }
            painter.line_segment(moving, stroke);
            painter.circle_stroke(moving[1], HANDLE_PX * 0.75, stroke);
        }
        if let Some(mark) = self.snap_mark
            && !matches!(self.move_tool.gesture, Some(GestureKind::Move | GestureKind::Scale(_)))
        {
            let c = screen(rect, view, mark);
            let r = HANDLE_PX;
            painter.line_segment([c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0)], stroke);
            painter.line_segment([c - egui::vec2(0.0, r), c + egui::vec2(0.0, r)], stroke);
        }
        if let Some(args) = &self.preview {
            let targets = dtool_core::command::transform_target_ids(&session.document, &session.selection.ids);
            let selection_map = dtool_core::command::selection_box_matrix(args);
            let constraint_maps = dtool_core::command::selection_constraint_maps(&session.document, &session.selection.ids, args);
            for &id in &targets {
                if self.live_text_resize(session) == Some(id) {
                    continue;
                }
                if session.document.get_object(id).and_then(|object| object.path_text_data())
                    .is_some_and(|path| !targets.contains(&path.path))
                {
                    continue;
                }
                let text = scene.quad(id)
                    .filter(|_| !dtool_core::scene::has_live_text_warp(&session.document, id))
                    .and_then(|_| dtool_core::text_lines::text_outlines(&session.document, id));
                let outlines = text.map_or_else(|| preview_outlines(scene, id), |contours| {
                    contours.into_iter().map(|points| (points, true)).collect()
                });
                let map = session.document.get_object(id).and_then(|object| object.path_text_data())
                    .and_then(|path| constraint_maps.get(&path.path)).or_else(|| constraint_maps.get(&id))
                    .unwrap_or(&selection_map);
                for (points, closed) in outlines {
                    let pts: Vec<Pos2> = points.into_iter()
                        .map(|point| screen(rect, view, dtool_core::geometry::apply_matrix(map, point)))
                        .collect();
                    painter.add(if closed { egui::Shape::closed_line(pts, stroke) } else { egui::Shape::line(pts, stroke) });
                }
            }
        }
        self.paint_snap_feedback(rect, session, &painter);
    }

    pub(crate) fn live_text_resize(&self, session: &Session) -> Option<ObjectId> {
        let &[id] = session.selection.ids.as_slice() else { return None };
        let applicable = matches!(self.move_tool.gesture, Some(GestureKind::Scale(_)))
            && self.preview.as_ref().is_some_and(|args| args.scale != [1.0, 1.0])
            && session.document.get_object(id).is_some_and(|object| {
                matches!(object.kind, dtool_core::ObjectKind::ArtText { .. } | dtool_core::ObjectKind::FrameText { .. })
            });
        applicable.then_some(id)
    }
}

pub fn preview_outlines(scene: &Scene, id: ObjectId) -> Vec<(Vec<[f32; 2]>, bool)> {
    let mut out = Vec::new();
    for prim in &scene.prims {
        match prim {
            dtool_core::scene::Prim::Fill { object, contours, .. } if *object == id => out.extend(contours.iter().cloned().map(|points| (points, true))),
            dtool_core::scene::Prim::Stroke { object, points, closed, .. } if *object == id => out.push((points.clone(), *closed)),
            _ => {}
        }
    }
    if out.is_empty() {
        if let Some(q) = scene.quad(id) {
            out.push((q.to_vec(), true));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtool_core::document::Units;
    use dtool_core::object::Object;
    use dtool_core::paint::{Colour, Paint};
    use dtool_core::scene::estimate_scene;
    use dtool_core::{CanvasFlags, Document};

    const NONE: Mods = Mods { shift: false, ctrl: false, alt: false };
    const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false };

    fn session() -> (Session, ObjectId, ObjectId) {
        let mut doc = Document::new([400.0, 400.0], Units::Px, 72.0, 1);
        let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
        let a = doc.create_object(Object { pos: [10.0, 10.0], size: [50.0, 50.0], fill: red.clone(), ..Object::default() });
        let b = doc.create_object(Object { pos: [200.0, 200.0], size: [40.0, 40.0], fill: red, ..Object::default() });
        let mut session = Session::new(doc);
        session.has_document = true;
        (session, a, b)
    }

    fn scene(session: &Session) -> Scene {
        estimate_scene(&session.document, &CanvasFlags::default())
    }

    #[test]
    fn command_errors_reach_status_key_and_ko_has_messages() {
        let (mut s, _, _) = session();
        let mut input = CanvasInput::default();
        assert!(!input.run(&mut s, Some(Command::with_params("path.join", json!({ "ids": [1] })))));
        assert!(input.last_error.is_some());
        let key = input.take_error_key().unwrap();
        assert!(key.starts_with("error."), "{key}");
        assert!(input.take_error_key().is_none());
        let ko: serde_json::Value = serde_json::from_str(include_str!("../../dtool-strings/ko.json")).unwrap();
        for code in ["ENDPOINT", "NOT_OPEN", "NEED_TWO_CURVES", "NOT_A_CURVE", "NOT_A_SEGMENT", "NOT_ENOUGH_POINTS", "ALREADY_CLOSED", "NOTHING_SELECTED", "LOCKED", "INVALID_PARAMS", "NOT_FOUND", "NO_DOCUMENT"] {
            let key = dtool_protocol::CommandError::new(code).message_key;
            assert!(ko.get(&key).and_then(|v| v.as_str()).is_some_and(|v| !v.is_empty()), "{key}");
        }
    }

    #[test]
    fn nudge_prefs_are_cached_and_overridable() {
        let mut input = CanvasInput::default();
        let loaded = input.nudge_distances();
        assert!(loaded.iter().all(|v| v.is_finite() && *v > 0.0));
        assert_eq!(input.nudge_prefs, Some(loaded));
        input.nudge_prefs = Some([3.0, 30.0]);
        assert_eq!(input.nudge_distances(), [3.0, 30.0]);
    }

    #[test]
    fn transform_and_modes() {
        let mut view = ViewState::default();
        view.zoom = 2.0;
        view.scroll = [10.0, 20.0];
        let doc = dtool_core::view_to_doc(&view, [30.0, 40.0]);
        assert_eq!(doc, [10.0, 10.0]);
        assert_eq!(dtool_core::doc_to_view(&view, doc), [30.0, 40.0]);
        zoom_at(&mut view, [30.0, 40.0], 2.0);
        assert_eq!(dtool_core::doc_to_view(&view, doc), [30.0, 40.0]);
        assert_eq!(nav_mode("tool.move", true, false), NavMode::Pan);
        assert_eq!(nav_mode("tool.zoom", false, false), NavMode::Zoom);
        assert_eq!(nav_mode("tool.move", false, false), NavMode::Input);
        assert_eq!(tool_kind("tool.shape.ellipse"), ToolKind::Shape(ShapeKind::Ellipse));
        assert_eq!(tool_kind("tool.text.frame"), ToolKind::FrameText);
        assert_eq!(tool_kind("tool.text.artistic"), ToolKind::ArtText);
        assert_eq!(tool_kind("tool.move"), ToolKind::Move);
    }

    #[test]
    fn click_select_toggle_and_clear() {
        let (mut s, a, b) = session();
        let sc = scene(&s);
        let mut input = CanvasInput::new();
        input.click_move(&sc, &mut s, [20.0, 20.0], NONE);
        assert_eq!(s.selection.ids, vec![a]);
        input.click_move(&sc, &mut s, [210.0, 210.0], SHIFT);
        assert_eq!(s.selection.ids.len(), 2);
        input.click_move(&sc, &mut s, [20.0, 20.0], SHIFT);
        assert_eq!(s.selection.ids, vec![b]);
        input.click_move(&sc, &mut s, [150.0, 150.0], NONE);
        assert!(s.selection.ids.is_empty());
    }

    #[test]
    fn drag_move_commits_one_transform() {
        let (mut s, a, _) = session();
        let sc = scene(&s);
        let mut input = CanvasInput::new();
        input.press_move(&sc, &mut s, [20.0, 20.0], NONE);
        assert_eq!(s.selection.ids, vec![a]);
        input.drag_move([25.0, 25.0], NONE);
        input.drag_move([30.0, 40.0], NONE);
        assert!(input.preview.is_some());
        assert_eq!(s.document.get_object(a).unwrap().pos, [10.0, 10.0]);
        let action = input.release_move(&sc, &mut s, [30.0, 40.0], NONE);
        assert!(matches!(action, MoveToolAction::TransformCommit { .. }));
        assert_eq!(s.document.get_object(a).unwrap().pos, [20.0, 30.0]);
        assert!(s.run(&Command::new("edit.undo")).is_ok());
        assert_eq!(s.document.get_object(a).unwrap().pos, [10.0, 10.0]);
    }

    #[test]
    fn handle_resize_keeps_aspect_with_shift() {
        let (mut s, a, _) = session();
        let mut input = CanvasInput::new();
        input.click_move(&scene(&s), &mut s, [20.0, 20.0], NONE);
        let sc = scene(&s);
        let action = input.press_move(&sc, &mut s, [60.0, 60.0], SHIFT);
        assert!(matches!(action, MoveToolAction::GestureStart { kind: GestureKind::Scale(Handle::BottomRight) }));
        input.drag_move([110.0, 70.0], SHIFT);
        input.release_move(&sc, &mut s, [110.0, 70.0], SHIFT);
        let o = s.document.get_object(a).unwrap();
        assert!((o.size[0] - 100.0).abs() < 0.01 && (o.size[1] - 100.0).abs() < 0.01, "{:?}", o.size);
    }

    #[test]
    fn marquee_and_escape() {
        let (mut s, a, b) = session();
        let sc = scene(&s);
        let mut input = CanvasInput::new();
        input.press_move(&sc, &mut s, [0.0, 0.0], NONE);
        input.drag_move([300.0, 300.0], NONE);
        assert!(input.marquee.is_some());
        let action = input.release_move(&sc, &mut s, [300.0, 300.0], NONE);
        assert!(matches!(action, MoveToolAction::MarqueeCommit { .. }));
        assert_eq!(s.selection.ids.len(), 2);
        assert!(s.selection.ids.contains(&a) && s.selection.ids.contains(&b));
        input.press_move(&sc, &mut s, [100.0, 300.0], NONE);
        input.drag_move([150.0, 350.0], NONE);
        input.cancel();
        assert!(!input.busy() && input.marquee.is_none());
    }

    #[test]
    fn shape_and_text_commands() {
        let cmd = shape_command(ShapeKind::Ellipse, [50.0, 50.0], [10.0, 20.0], NONE).unwrap();
        assert_eq!(cmd.name, "shape.add");
        assert_eq!(cmd.params["pos"], json!([10.0, 20.0]));
        assert_eq!(cmd.params["size"], json!([40.0, 30.0]));
        assert!(shape_command(ShapeKind::Rectangle, [5.0, 5.0], [5.0, 5.0], NONE).is_none());
        let (mut s, _, _) = session();
        let before = s.document.objects.len();
        let mut input = CanvasInput::new();
        input.drag = Some(Drag::Shape { kind: ShapeKind::Rectangle, start: [100.0, 100.0], current: [150.0, 130.0] });
        assert!(input.finish_drag(&mut s, [150.0, 130.0], NONE), "{:?}", input.last_error);
        assert_eq!(s.document.objects.len(), before + 1);
        assert_eq!(art_text_command([10.0, 10.0], [10.0, 10.0], 1.0).name, "text.art.add");
        input.drag = Some(Drag::ArtText { start: [300.0, 300.0], current: [300.0, 300.0] });
        assert!(input.finish_drag(&mut s, [300.0, 300.0], NONE), "{:?}", input.last_error);
        assert_eq!(s.document.objects.len(), before + 2);
    }

    #[test]
    fn tab_switch_mid_drag_cancels_and_leaves_both_documents() {
        let (mut a, _, _) = session();
        let (mut b, _, _) = session();
        let before = (a.document.version, a.document.objects.len(), b.document.version, b.document.objects.len());
        let mut input = CanvasInput::new();
        input.drag = Some(Drag::Shape { kind: ShapeKind::Rectangle, start: [100.0, 100.0], current: [150.0, 130.0] });
        input.claim(&a);
        assert_eq!(input.owner_check(&b), OwnerCheck::Lost);
        let sc = scene(&b);
        assert!(input.resolve_owner(&mut b, &sc));
        assert!(!input.gesture_active() && input.drag.is_none() && input.owner.is_none());
        assert!(!input.finish_drag(&mut b, [150.0, 130.0], NONE));
        assert_eq!((a.document.version, a.document.objects.len(), b.document.version, b.document.objects.len()), before);
        assert_eq!(input.owner_check(&a), OwnerCheck::Same);
        let sc = scene(&a);
        assert!(!input.resolve_owner(&mut a, &sc));
        assert_eq!(a.document.objects.len(), before.1);
    }

    #[test]
    fn bound_context_change_cancels_move_drag() {
        let (mut s, a, _) = session();
        let sc = scene(&s);
        let mut input = CanvasInput::new();
        input.bind_context(Some(1));
        input.press_move(&sc, &mut s, [20.0, 20.0], NONE);
        input.drag_move([40.0, 40.0], NONE);
        input.last_point = Some([40.0, 40.0]);
        input.claim(&s);
        input.bind_context(Some(2));
        assert_eq!(input.owner_check(&s), OwnerCheck::Lost);
        assert!(input.resolve_owner(&mut s, &sc));
        assert!(!input.busy());
        assert_eq!(s.document.get_object(a).unwrap().pos, [10.0, 10.0]);
    }

    #[test]
    fn document_change_under_gesture_cancels() {
        let (mut s, _, _) = session();
        let mut input = CanvasInput::new();
        input.drag = Some(Drag::Line { start: [0.0, 0.0], current: [50.0, 50.0] });
        input.claim(&s);
        s.document.version += 1;
        let objects = s.document.objects.len();
        let sc = scene(&s);
        assert_eq!(input.owner_check(&s), OwnerCheck::Lost);
        input.resolve_owner(&mut s, &sc);
        assert!(input.drag.is_none());
        assert_eq!(s.document.objects.len(), objects);
    }

    #[test]
    fn tool_switch_mid_drag_commits_one_step() {
        let (mut s, _, _) = session();
        s.active_tool = "tool.shape.rectangle".to_string();
        let before = s.document.objects.len();
        let mut input = CanvasInput::new();
        input.drag = Some(Drag::Shape { kind: ShapeKind::Rectangle, start: [100.0, 100.0], current: [150.0, 130.0] });
        input.claim(&s);
        assert_eq!(input.owner_check(&s), OwnerCheck::Same);
        s.active_tool = "tool.move".to_string();
        assert_eq!(input.owner_check(&s), OwnerCheck::ToolChanged);
        let sc = scene(&s);
        assert!(input.resolve_owner(&mut s, &sc));
        assert!(!input.gesture_active() && input.owner.is_none());
        assert_eq!(s.document.objects.len(), before + 1);
        assert!(s.run(&Command::new("edit.undo")).is_ok());
        assert_eq!(s.document.objects.len(), before);
    }

    #[test]
    fn tool_switch_mid_move_commits_at_last_point() {
        let (mut s, a, _) = session();
        let sc = scene(&s);
        let mut input = CanvasInput::new();
        input.press_move(&sc, &mut s, [20.0, 20.0], NONE);
        input.drag_move([30.0, 40.0], NONE);
        input.last_point = Some([30.0, 40.0]);
        input.claim(&s);
        s.active_tool = "tool.node".to_string();
        assert!(input.resolve_owner(&mut s, &sc));
        assert!(!input.busy());
        assert_eq!(s.document.get_object(a).unwrap().pos, [20.0, 30.0]);
    }
}
