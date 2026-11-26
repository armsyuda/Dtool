use dtool_core::command::Session;
use dtool_core::commands::curve_edit::{map_contours, world_contours, CURVE_ADD, CURVE_NODES_SET};
use dtool_core::commands::curves::{preview_contours, SHAPE_CONVERT};
use dtool_core::commands::curve_nodes::{CURVE_NODES_CONVERT, CURVE_NODES_DELETE, CURVE_NODES_INSERT, segment_count, segment_point_within};
use dtool_core::commands::path_nodes::{PATH_BREAK, PATH_CLOSE, PATH_JOIN, PATH_POINT_ADD};
use dtool_core::curve::{Contour, NodeKind, PathPoint, PointId};
use dtool_core::geometry::{IDENTITY, invert_matrix, object_matrix};
use dtool_core::object::ObjectId;
use dtool_core::transform::Anchor;
use dtool_protocol::Command;
use serde_json::json;
use std::collections::{HashMap, HashSet};

#[path = "pen_spline.rs"]
mod pen_spline;

pub const ANGLE_STEP_DEG: f32 = 45.0;

pub fn p32(p: [f64; 2]) -> [f32; 2] {
    [p[0] as f32, p[1] as f32]
}

pub fn p64(p: [f32; 2]) -> [f64; 2] {
    [f64::from(p[0]), f64::from(p[1])]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

pub fn constrain_angle(start: [f32; 2], end: [f32; 2]) -> [f32; 2] {
    let len = distance(start, end);
    if len <= 0.0 {
        return end;
    }
    let step = ANGLE_STEP_DEG.to_radians();
    let angle = ((end[1] - start[1]).atan2(end[0] - start[0]) / step).round() * step;
    [start[0] + len * angle.cos(), start[1] + len * angle.sin()]
}

pub fn drawing_curve_stroke(session: &Session) -> dtool_core::paint::StrokeStyle {
    let mut stroke = dtool_core::commands::curve_edit::default_stroke();
    stroke.width *= dtool_core::units::units_per_point(session.document.units, session.document.dpi);
    dtool_core::commands::context_toolbar::drawing_stroke(session, &session.active_tool, stroke)
}

pub fn line_command(start: [f32; 2], end: [f32; 2]) -> Option<Command> {
    if distance(start, end) < crate::canvas_input::MIN_DRAG_DOC {
        return None;
    }
    Some(Command::with_params(CURVE_ADD, json!({ "points": [start, end] })))
}

pub fn flatten(contour: &Contour, tolerance: f64) -> Vec<(Vec<[f32; 2]>, bool)> {
    dtool_core::outline::flatten_subpaths(&contour.to_kurbo(), tolerance)
        .into_iter()
        .map(|(points, closed)| (points.into_iter().map(p32).collect(), closed))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenStep {
    Added,
    Close,
    Handle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PenMode {
    #[default]
    Pen,
    Smart,
    Polygon,
    Line,
    Spline,
    BSpline,
}

impl PenMode {
    pub fn from_key(key: &str) -> Self {
        match key {
            "smart" => PenMode::Smart,
            "polygon" => PenMode::Polygon,
            "line" => PenMode::Line,
            "spline" => PenMode::Spline,
            "b_spline" => PenMode::BSpline,
            _ => PenMode::Pen,
        }
    }

    pub fn for_tool(active_tool: &str, option: Option<&str>) -> Self {
        match active_tool {
            "tool.pen.smart" => PenMode::Smart,
            "tool.pen.polyline" => PenMode::Polygon,
            "tool.pen.spline" => PenMode::Spline,
            "tool.pen.b_spline" => PenMode::BSpline,
            _ => option.map(PenMode::from_key).unwrap_or_default(),
        }
    }

    pub fn is_spline(self) -> bool {
        matches!(self, Self::Spline | Self::BSpline)
    }

    fn fit(self, points: Vec<PathPoint>, closed: bool) -> Vec<PathPoint> {
        match self {
            Self::Smart => smart_nodes(&points, closed),
            Self::Spline => pen_spline::interpolating(&points, closed),
            Self::BSpline => pen_spline::basis_spline(&points, closed),
            _ => points,
        }
    }
}

pub fn smart_nodes(points: &[PathPoint], closed: bool) -> Vec<PathPoint> {
    if closed {
        let positions: Vec<[f64; 2]> = points.iter().map(|p| p.pos).collect();
        return Contour::smart_handles(&positions).into_iter().zip(points).map(|(node, src)| PathPoint { id: src.id, ..node }).collect();
    }
    let n = points.len();
    points
        .iter()
        .enumerate()
        .map(|(i, src)| {
            if n < 3 || i == 0 || i + 1 == n {
                return PathPoint::sharp(src.id, src.pos);
            }
            let (prev, next) = (points[i - 1].pos, points[i + 1].pos);
            let h = [(next[0] - prev[0]) / 6.0, (next[1] - prev[1]) / 6.0];
            PathPoint { id: src.id, pos: src.pos, handle_in: Some([src.pos[0] - h[0], src.pos[1] - h[1]]), handle_out: Some([src.pos[0] + h[0], src.pos[1] + h[1]]), kind: NodeKind::Smart, corner: None }
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pen {
    pub nodes: Vec<PathPoint>,
    pub dragging: bool,
    pub polyline: bool,
    pub alt: bool,
    pub mode: PenMode,
    pub shift: bool,
    pub space: bool,
    pub rubber_band: bool,
    pub last: Option<[f32; 2]>,
    pub stroke: Option<dtool_core::paint::StrokeStyle>,
    pub extension: Option<(PathExtension, f32)>,
    pub construction_snap: Option<ConstructionSnap>,
}

impl Pen {
    pub fn active(&self) -> bool {
        !self.nodes.is_empty()
    }

    pub fn can_close(&self) -> bool {
        match &self.extension {
            Some((extension, _)) => self.nodes.len() >= 2 && extension.contour.points.len() + self.nodes.len() >= 4,
            None => self.nodes.len() >= 3,
        }
    }

    pub fn start_at(
        &mut self, session: &Session, scene: &dtool_core::scene::Scene, point: [f32; 2], snapped: [f32; 2], near: f32,
    ) -> [f32; 2] {
        if self.active() {
            return self.closing_target(point, near).unwrap_or(snapped);
        }
        if let Some((extension, anchor)) = PathExtension::at(session, scene, point, near) {
            self.extension = Some((extension, near));
            return anchor;
        }
        snapped
    }

    fn closing_target(&self, point: [f32; 2], near: f32) -> Option<[f32; 2]> {
        let target = match &self.extension {
            Some((extension, _)) if !self.nodes.is_empty() && extension.contour.points.len() + self.nodes.len() >= 4 => {
                extension.closing_point()?
            }
            None if self.can_close() => p32(self.nodes.first()?.pos),
            _ => return None,
        };
        (distance(point, target) <= near).then_some(target)
    }

    fn handles(&self) -> bool {
        !self.polyline && self.mode == PenMode::Pen
    }

    pub fn line_done(&self) -> bool {
        self.mode == PenMode::Line && self.nodes.len() >= 2
    }

    pub fn press(&mut self, point: [f32; 2], near: f32) -> PenStep {
        let close = self.closing_target(point, near);
        let point = match self.nodes.last() {
            Some(prev) if self.shift => constrain_angle(p32(prev.pos), point),
            _ => point,
        };
        self.last = Some(point);
        if self.alt && self.handles() && self.nodes.last().is_some_and(|last| distance(p32(last.pos), point) <= near) {
            self.dragging = true;
            return PenStep::Handle;
        }
        if let Some(target) = close {
            if self.extension.is_some() {
                let id = PointId(self.nodes.len() as u64 + 1);
                self.nodes.push(PathPoint::sharp(id, p64(target)));
            }
            self.dragging = false;
            return PenStep::Close;
        }
        let id = PointId(self.nodes.len() as u64 + 1);
        self.nodes.push(PathPoint::sharp(id, p64(point)));
        self.dragging = self.handles() || self.space;
        PenStep::Added
    }

    fn move_last(&mut self, point: [f32; 2]) {
        let Some(from) = self.last else { return };
        let d = [f64::from(point[0] - from[0]), f64::from(point[1] - from[1])];
        if let Some(node) = self.nodes.last_mut() {
            node.pos = shifted(node.pos, d);
            node.handle_in = node.handle_in.map(|h| shifted(h, d));
            node.handle_out = node.handle_out.map(|h| shifted(h, d));
        }
    }

    pub fn drag(&mut self, point: [f32; 2]) {
        if !self.dragging {
            return;
        }
        if self.space {
            self.move_last(point);
            self.last = Some(point);
            return;
        }
        self.last = Some(point);
        if !self.handles() {
            return;
        }
        let Some(last) = self.nodes.last_mut() else { return };
        let pos = last.pos;
        let point = if self.shift { constrain_angle(p32(pos), point) } else { point };
        let out = p64(point);
        if (out[0] - pos[0]).hypot(out[1] - pos[1]) < 1e-3 {
            return;
        }
        last.handle_out = Some(out);
        if self.alt {
            last.kind = NodeKind::Sharp;
            return;
        }
        last.handle_in = Some([2.0 * pos[0] - out[0], 2.0 * pos[1] - out[1]]);
        last.kind = NodeKind::Smooth;
    }

    pub(crate) fn drag_with_construction(&mut self, point: [f32; 2], enabled: bool, tolerance: f32) {
        self.construction_snap = None;
        if enabled && self.dragging && self.handles() && !self.alt && !self.shift && !self.space
            && let [.., previous, node] = self.nodes.as_slice()
        {
            let anchor = p32(node.pos);
            let incoming = [2.0 * anchor[0] - point[0], 2.0 * anchor[1] - point[1]];
            self.construction_snap = construction_handle_point(
                anchor, p32(previous.pos), previous.handle_out.map(p32), incoming, tolerance,
            );
            if let Some(snap) = self.construction_snap {
                let target = snap.moving[1];
                self.drag([2.0 * anchor[0] - target[0], 2.0 * anchor[1] - target[1]]);
                self.last = Some(point);
                return;
            }
        }
        self.drag(point);
    }

    pub fn release(&mut self) {
        self.dragging = false;
        self.construction_snap = None;
    }

    pub fn drop_duplicate_tail(&mut self, near: f32) {
        let n = self.nodes.len();
        if n >= 2 && distance(p32(self.nodes[n - 1].pos), p32(self.nodes[n - 2].pos)) <= near {
            self.nodes.pop();
        }
    }

    pub fn cancel(&mut self) {
        self.nodes.clear();
        self.dragging = false;
        self.construction_snap = None;
        self.last = None;
        self.extension = None;
    }

    pub fn finish_command(&mut self, closed: bool) -> Option<Command> {
        let mut nodes = std::mem::take(&mut self.nodes);
        self.release();
        self.last = None;
        if let Some((extension, near)) = self.extension.take() {
            if closed && nodes.len() >= 2 {
                let target = extension.closing_point()?;
                if distance(p32(nodes.last()?.pos), target) > near {
                    let id = PointId(nodes.len() as u64 + 1);
                    nodes.push(PathPoint::sharp(id, p64(target)));
                }
            }
            return extension.finish(nodes, true, near, self.mode.is_spline(), |nodes| self.mode.fit(nodes, false));
        }
        if nodes.len() < if closed { 3 } else { 2 } {
            return None;
        }
        let nodes = self.mode.fit(nodes, closed);
        let mut command = Command::with_params(CURVE_ADD, json!({ "points": nodes, "closed": closed }));
        if let Some(stroke) = &self.stroke {
            command.params["stroke"] = json!(stroke);
        }
        Some(command)
    }

    pub fn preview(&self, hover: Option<[f32; 2]>) -> Option<Contour> {
        let mut points = self.nodes.clone();
        if let Some(h) = hover.filter(|_| !self.dragging && !points.is_empty()) {
            let h = match points.last() {
                Some(prev) if self.shift => constrain_angle(p32(prev.pos), h),
                _ => h,
            };
            points.push(PathPoint::sharp(PointId(u64::MAX), p64(h)));
        }
        if points.len() < 2 {
            return None;
        }
        let mut points = self.mode.fit(points, false);
        if let Some((extension, _)) = &self.extension {
            if self.mode.is_spline() {
                extension.align_tangents(&mut points, false);
            }
            if let Some(handle) = extension.start_handle() {
                points[0].handle_out = Some(handle);
            }
        }
        Some(Contour { points, closed: false })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pencil {
    pub points: Vec<[f32; 2]>,
    extension: Option<PathExtension>,
    keep_open: bool,
    stroke: Option<dtool_core::paint::StrokeStyle>,
    brush: Option<(dtool_core::paint::StrokeStyle, f32, Option<dtool_core::path_brush::Texture>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathExtension {
    id: ObjectId,
    index: usize,
    prepend: bool,
    contour: Contour,
    inverse: [f32; 6],
}

pub fn simplify(points: &[[f32; 2]], tolerance: f32) -> Vec<[f32; 2]> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let far = (a + 1..b)
            .map(|i| (i, line_distance(points[i], points[a], points[b])))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((i, _)) = far.filter(|(_, d)| *d > tolerance) {
            keep[i] = true;
            stack.push((a, i));
            stack.push((i, b));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

fn line_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    if l <= f32::EPSILON {
        return distance(p, a);
    }
    ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / l
}

pub fn smooth_nodes(points: &[[f32; 2]], closed: bool) -> Vec<PathPoint> {
    let n = points.len();
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let id = PointId(i as u64 + 1);
            let pos = p64(*p);
            if n < 3 {
                return PathPoint::sharp(id, pos);
            }
            let (prev, next) = if closed {
                (points[(i + n - 1) % n], points[(i + 1) % n])
            } else {
                (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)])
            };
            let t = [f64::from(next[0] - prev[0]) / 6.0, f64::from(next[1] - prev[1]) / 6.0];
            let mut node = PathPoint::sharp(id, pos);
            let first = !closed && i == 0;
            let last = !closed && i == n - 1;
            if !last {
                node.handle_out = Some([pos[0] + t[0], pos[1] + t[1]]);
            }
            if !first {
                node.handle_in = Some([pos[0] - t[0], pos[1] - t[1]]);
            }
            if !first && !last {
                node.kind = NodeKind::Smooth;
            }
            node
        })
        .collect()
}

impl Pencil {
    pub fn active(&self) -> bool {
        !self.points.is_empty()
    }

    pub fn press(&mut self, point: [f32; 2]) {
        self.cancel();
        self.points.push(point);
    }

    pub fn begin_brush(&mut self, session: &Session, point: [f32; 2]) {
        self.press(point);
        self.brush = Some(dtool_core::commands::brush::vector_brush_settings(session));
    }

    pub fn brush(&self) -> Option<&(dtool_core::paint::StrokeStyle, f32, Option<dtool_core::path_brush::Texture>)> {
        self.brush.as_ref()
    }

    pub fn begin(
        &mut self, session: &Session, scene: &dtool_core::scene::Scene,
        point: [f32; 2], snapped: [f32; 2], near: f32,
    ) {
        let options = &session.tool_options;
        self.press(snapped);
        self.stroke = Some(drawing_curve_stroke(session));
        self.keep_open = !options.flag("tool.pen.pencil", "pencil.auto_close").unwrap_or(true);
        if !options.flag("tool.pen.pencil", "pencil.extend").unwrap_or(true) {
            return;
        }
        if let Some((extension, anchor)) = PathExtension::at(session, scene, point, near) {
            self.points[0] = anchor;
            self.extension = Some(extension);
        }
    }

    pub fn drag(&mut self, point: [f32; 2], step: f32) {
        if self.points.last().is_some_and(|last| distance(*last, point) >= step) {
            self.points.push(point);
        }
    }

    pub fn cancel(&mut self) {
        self.points.clear();
        self.extension = None;
        self.keep_open = false;
        self.stroke = None;
        self.brush = None;
    }

    pub fn finish_command(&mut self, tolerance: f32, near: f32) -> Option<Command> {
        let points = std::mem::take(&mut self.points);
        let auto_close = !std::mem::take(&mut self.keep_open);
        let mut kept = simplify(&points, tolerance);
        if let Some((stroke, opacity, texture)) = self.brush.take() {
            if kept.len() < 2 || (kept.len() == 2 && distance(kept[0], kept[1]) <= tolerance) {
                return None;
            }
            return Some(Command::with_params("brush.vector.stroke", json!({
                "points": kept, "stroke": stroke, "opacity": opacity, "texture": texture, "select": true
            })));
        }
        if let Some(extension) = self.extension.take() {
            if kept.len() < 2 || (kept.len() == 2 && distance(kept[0], kept[1]) <= near) {
                return None;
            }
            let anchors = kept.into_iter().enumerate()
                .map(|(i, point)| PathPoint::sharp(PointId(i as u64 + 1), p64(point))).collect();
            return extension.finish(anchors, auto_close, near, false, |nodes| {
                let points: Vec<_> = nodes.iter().map(|node| p32(node.pos)).collect();
                smooth_nodes(&points, false)
            });
        }
        let closed = auto_close && kept.len() >= 4 && distance(kept[0], kept[kept.len() - 1]) <= near;
        if closed {
            kept.pop();
        }
        if kept.len() < 2 || (kept.len() == 2 && distance(kept[0], kept[1]) <= near) {
            return None;
        }
        let mut command = Command::with_params(CURVE_ADD, json!({ "points": smooth_nodes(&kept, closed), "closed": closed }));
        if let Some(stroke) = self.stroke.take() {
            command.params["stroke"] = json!(stroke);
        }
        Some(command)
    }

    pub fn preview(&self) -> Option<Contour> {
        (self.points.len() >= 2).then(|| Contour { points: self.points.iter().enumerate().map(|(i, p)| PathPoint::sharp(PointId(i as u64 + 1), p64(*p))).collect(), closed: false })
    }
}

impl PathExtension {
    fn end_indices(&self) -> (usize, usize) {
        let last = self.contour.points.len() - 1;
        if self.prepend { (0, last) } else { (last, 0) }
    }

    fn closing_point(&self) -> Option<[f32; 2]> {
        let (_, opposite) = self.end_indices();
        let matrix = dtool_core::geometry::invert_matrix(&self.inverse)?;
        Some(dtool_core::geometry::apply_matrix(&matrix, p32(self.contour.points[opposite].pos)))
    }

    fn start_handle(&self) -> Option<[f64; 2]> {
        let (start, _) = self.end_indices();
        let endpoint = &self.contour.points[start];
        let handle = if self.prepend { endpoint.handle_in } else { endpoint.handle_out }?;
        let matrix = dtool_core::geometry::invert_matrix(&self.inverse)?;
        Some(p64(dtool_core::geometry::apply_matrix(&matrix, p32(handle))))
    }

    fn at(
        session: &Session, scene: &dtool_core::scene::Scene, point: [f32; 2], near: f32,
    ) -> Option<(Self, [f32; 2])> {
        use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};
        let mut best = None;
        let mut closest = near;
        for &id in &session.selection.ids {
            let Some(object) = session.document.get_object(id)
                .filter(|o| !o.locked && o.vector_crop.is_none() && scene.quad(id).is_some()) else { continue };
            let matrix = object_matrix(object);
            let Some(inverse) = invert_matrix(&matrix) else { continue };
            for (index, contour) in world_contours(object).into_iter().enumerate() {
                if contour.closed || contour.points.len() < 2 {
                    continue;
                }
                let ends = [(true, &contour.points[0]), (false, &contour.points[contour.points.len() - 1])];
                let (gap, prepend, anchor) = ends.into_iter().map(|(prepend, node)| {
                    let anchor = apply_matrix(&matrix, p32(node.pos));
                    (distance(point, anchor), prepend, anchor)
                }).min_by(|a, b| a.0.total_cmp(&b.0)).expect("open contour has two ends");
                if gap <= closest {
                    closest = gap;
                    best = Some((Self { id, index, prepend, contour, inverse }, anchor));
                }
            }
        }
        best
    }

    fn align_tangents(&self, nodes: &mut [PathPoint], closes: bool) {
        use dtool_core::geometry::{apply_matrix, invert_matrix};
        let Some(matrix) = invert_matrix(&self.inverse) else { return };
        let last = self.contour.points.len() - 1;
        let align = |i: usize, neighbor: usize, incoming: bool, point: &mut PathPoint, next: [f64; 2], entering: bool| {
            let endpoint = &self.contour.points[i];
            let previous = if incoming { endpoint.handle_in } else { endpoint.handle_out }
                .unwrap_or(self.contour.points[neighbor].pos);
            let end = apply_matrix(&matrix, p32(endpoint.pos));
            let prev = apply_matrix(&matrix, p32(previous));
            let direction = [f64::from(end[0] - prev[0]), f64::from(end[1] - prev[1])];
            let distance = direction[0].hypot(direction[1]);
            let handle = if entering { &mut point.handle_in } else { &mut point.handle_out };
            if distance > 1e-6 {
                let reference = handle.unwrap_or(next);
                let length = (reference[0] - point.pos[0]).hypot(reference[1] - point.pos[1]);
                let length = if handle.is_some() { length } else { length / 3.0 };
                *handle = Some(std::array::from_fn(|axis| point.pos[axis] + length * direction[axis] / distance));
            }
        };
        if let Some(next) = nodes.get(1).map(|node| node.pos) {
            if self.prepend {
                align(0, 1, false, &mut nodes[0], next, false);
            } else {
                align(last, last - 1, true, &mut nodes[0], next, false);
            }
        }
        if closes && nodes.len() >= 2 {
            let end = nodes.len() - 1;
            let previous = nodes[end - 1].pos;
            if self.prepend {
                align(last, last - 1, true, &mut nodes[end], previous, true);
            } else {
                align(0, 1, false, &mut nodes[end], previous, true);
            }
        }
    }

    fn finish(
        mut self, mut kept: Vec<PathPoint>, auto_close: bool, near: f32, smooth_join: bool,
        fit: impl FnOnce(Vec<PathPoint>) -> Vec<PathPoint>,
    ) -> Option<Command> {
        if kept.len() < 2 {
            return None;
        }
        use dtool_core::geometry::apply_matrix;
        let (start, opposite) = self.end_indices();
        let end = p32(kept.last()?.pos);
        let target = if auto_close { self.closing_point().filter(|&point| distance(end, point) <= near) } else { None };
        let closes = target.is_some();
        if let Some(target) = target {
            if kept.len() + self.contour.points.len() < 5 {
                return None;
            }
            let tail = kept.last_mut()?;
            let at = p64(target);
            let delta = [at[0] - tail.pos[0], at[1] - tail.pos[1]];
            tail.pos = at;
            tail.handle_in = tail.handle_in.map(|point| shifted(point, delta));
            tail.handle_out = tail.handle_out.map(|point| shifted(point, delta));
        }
        let mut nodes = fit(kept);
        if nodes.len() < 2 || (closes && nodes.len() + self.contour.points.len() < 5) {
            return None;
        }
        if smooth_join {
            self.align_tangents(&mut nodes, closes);
        }
        for node in &mut nodes {
            let map = |point| p64(apply_matrix(&self.inverse, p32(point)));
            node.pos = map(node.pos);
            node.handle_in = node.handle_in.map(map);
            node.handle_out = node.handle_out.map(map);
        }
        let stroke_start = nodes.remove(0);
        let endpoint = &mut self.contour.points[start];
        let handle = if self.prepend { &mut endpoint.handle_in } else { &mut endpoint.handle_out };
        *handle = handle.or(stroke_start.handle_out);
        if closes {
            let stroke_end = nodes.pop()?;
            let endpoint = &mut self.contour.points[opposite];
            let handle = if self.prepend { &mut endpoint.handle_out } else { &mut endpoint.handle_in };
            *handle = handle.or(stroke_end.handle_in);
            self.contour.closed = true;
        }
        let mut next_id = self.contour.points.iter().map(|node| node.id.0).max()?;
        for node in &mut nodes {
            next_id = next_id.checked_add(1)?;
            node.id = PointId(next_id);
        }
        if self.prepend {
            nodes.reverse();
            for node in &mut nodes {
                std::mem::swap(&mut node.handle_in, &mut node.handle_out);
            }
            self.contour.points.splice(0..0, nodes);
        } else {
            self.contour.points.extend(nodes);
        }
        Some(Command::with_params(CURVE_NODES_SET, json!({
            "id": self.id, "index": self.index, "contour": self.contour, "preserve_transform": true
        })))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodePart {
    Anchor(usize, usize),
    In(usize, usize),
    Out(usize, usize),
    Segment(usize, usize),
    Scale,
    Rotate,
    Shear(NodeShear),
    Align(NodeAlignHandle),
    Origin,
}

pub(crate) enum NodeAnchorSnap {
    Point { offset: [f32; 2], target: [f32; 2] },
    /// Each axis has its own offset and target coordinate.
    Axes([Option<(f32, f32)>; 2]),
}

impl NodePart {
    fn is_shear(self) -> bool { matches!(self, Self::Shear(_)) }

    fn allows_separate_transform(self) -> bool {
        matches!(self, Self::Scale | Self::Rotate) || self.is_shear()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeAlignHandle {
    pub(crate) axis: usize,
    edge: usize,
}

impl NodeAlignHandle {
    fn coordinate(self, bounds: [f32; 4]) -> f32 {
        let low = bounds[self.axis];
        let high = bounds[self.axis + 2];
        match self.edge {
            0 => low,
            1 => low.midpoint(high),
            _ => high,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeShear { Top, Bottom, Left, Right }

impl NodeShear {
    // The coordinate displaced by the shear. The other coordinate is fixed.
    pub(crate) fn axis(self) -> usize {
        match self {
            Self::Top | Self::Bottom => 0,
            Self::Left | Self::Right => 1,
        }
    }

    fn edges(self, bounds: [f32; 4]) -> (f32, f32) {
        let axis = 1 - self.axis();
        if matches!(self, Self::Top | Self::Left) {
            (bounds[axis], bounds[axis + 2])
        } else {
            (bounds[axis + 2], bounds[axis])
        }
    }
}

pub const NODE_TRANSFORM_ANCHOR: Anchor = Anchor::TopLeft;
pub(crate) const NODE_CONVERSION_CHOICES: [(NodeKind, &str); 3] = [
    (NodeKind::Sharp, "context.node.convert.sharp"),
    (NodeKind::Smooth, "context.node.convert.smooth"),
    (NodeKind::Smart, "context.node.convert.smart"),
];

pub fn segment_at(contours: &[Contour], point: [f32; 2], radius: f32) -> Option<(usize, usize)> {
    let point = p64(point);
    let mut distance = f64::from(radius);
    let mut best = None;
    for (c, contour) in contours.iter().enumerate() {
        for i in 0..segment_count(contour) {
            if let Some(hit) = segment_point_within(contour, i, point, distance) {
                let next_distance = (hit[0] - point[0]).hypot(hit[1] - point[1]);
                if best.is_none() || next_distance < distance {
                    distance = next_distance;
                    best = Some((c, i));
                }
            }
        }
    }
    best
}

pub fn selection_box(contours: &[Contour], selected: &[(usize, usize)]) -> Option<[f32; 4]> {
    selected.iter()
        .filter_map(|&(c, i)| Some(p32(contours.get(c)?.points.get(i)?.pos)))
        .map(|[x, y]| [x, y, x, y])
        .reduce(union_node_bounds)
}

fn union_node_bounds(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

fn curve_outline_bounds(contours: &[Contour]) -> Option<[f32; 4]> {
    contours.iter().filter(|contour| !contour.points.is_empty())
        .map(Contour::bounds).reduce(union_node_bounds)
}

fn node_box_center([left, top, right, bottom]: [f32; 4]) -> [f32; 2] {
    [left.midpoint(right), top.midpoint(bottom)]
}

pub(crate) fn node_rotate_handle([left, top, right, _]: [f32; 4], offset: f32) -> [f32; 2] {
    [left.midpoint(right), top - offset]
}

pub(crate) fn node_shear_handles([left, top, right, bottom]: [f32; 4], radius: f32) -> impl Iterator<Item = (NodeShear, [f32; 2])> {
    use dtool_tools::move_tool::shear_handle_points;
    let width = right - left;
    let height = bottom - top;
    let [top_grip, bottom_grip] = shear_handle_points([left, top, width, height], radius);
    let [left_grip, right_grip] = shear_handle_points([top, left, height, width], radius);
    [
        (height, NodeShear::Top, top_grip),
        (height, NodeShear::Bottom, bottom_grip),
        (width, NodeShear::Left, [left_grip[1], left_grip[0]]),
        (width, NodeShear::Right, [right_grip[1], right_grip[0]]),
    ].into_iter().filter_map(move |(lever, side, point)| (lever > radius * 2.0).then_some((side, point)))
}

#[derive(Debug, Clone)]
pub enum NodePress {
    Grab,
    Run(Command),
    Miss,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeEdit {
    pub target: Option<ObjectId>,
    pub selected: Vec<(usize, usize)>,
    pub grab: Option<(NodePart, [f32; 2])>,
    pub base: Option<Vec<Contour>>,
    pub live: Option<Vec<Contour>>,
    pub primitive: bool,
    pub alt: bool,
    pub shift: bool,
    pub(crate) ctrl: bool,
    pub transform_mode: bool,
    pub(crate) construction_snap: Option<ConstructionSnap>,
    pub(crate) marquee: Option<(egui::Pos2, egui::Pos2, bool)>,
    display_to_edit: Option<[f32; 6]>,
    context: Option<dtool_core::workspace::ContextId>,
    point_order: Vec<Vec<PointId>>,
    point_order_generation: Option<u64>,
    other_selected: HashMap<ObjectId, HashSet<(usize, PointId)>>,
    other_drags: Vec<CurveNodeDrag>,
    group_transform: Option<NodeGroupTransform>,
    individual_origins: HashMap<ObjectId, [f64; 2]>,
    transform_bounds: Option<[f32; 4]>,
    gesture_origin: Option<[f32; 2]>,
    origin_preview: Option<[f32; 2]>,
    custom_origin: Option<NodeOrigin>,
}

#[derive(Debug, Clone, PartialEq)]
struct NodeOrigin {
    point: [f32; 2],
    selection: Vec<(ObjectId, usize, PointId)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum NodeGroupTransform {
    Translate([f64; 2]),
    Scale { origin: [f64; 2], factors: [f64; 2] },
    Rotate { origin: [f64; 2], sin: f64, cos: f64 },
    Shear { axis: usize, origin: f64, factor: f64 },
}

impl NodeGroupTransform {
    fn scale(bounds: [f32; 4], delta: [f64; 2], proportional: bool) -> Self {
        let origin = [f64::from(bounds[0]), f64::from(bounds[1])];
        let span = [f64::from(bounds[2]) - origin[0], f64::from(bounds[3]) - origin[1]];
        let mut factors = std::array::from_fn(|axis| {
            if span[axis] > 1e-6 { 1.0 + delta[axis] / span[axis] } else { 1.0 }
        });
        if proportional {
            let length_squared = span[0] * span[0] + span[1] * span[1];
            if length_squared > 1e-12 {
                let factor = 1.0 + (delta[0] * span[0] + delta[1] * span[1]) / length_squared;
                factors = [factor; 2];
            }
        }
        Self::Scale { origin, factors }
    }

    fn map(self, point: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Translate(delta) => shifted(point, delta),
            Self::Scale { origin, factors } => scale_about(point, origin, factors),
            Self::Rotate { origin, sin, cos } => {
                let x = point[0] - origin[0];
                let y = point[1] - origin[1];
                [origin[0] + cos * x - sin * y, origin[1] + sin * x + cos * y]
            }
            Self::Shear { axis, origin, factor } => {
                let mut moved = point;
                moved[axis] += (point[1 - axis] - origin) * factor;
                moved
            }
        }
    }

    fn with_origin(self, point: Option<[f64; 2]>) -> Self {
        let Some(origin) = point else { return self };
        match self {
            Self::Scale { factors, .. } => Self::Scale { origin, factors },
            Self::Rotate { sin, cos, .. } => Self::Rotate { origin, sin, cos },
            Self::Shear { axis, factor, .. } => Self::Shear { axis, origin: origin[1 - axis], factor },
            Self::Translate(_) => self,
        }
    }

    fn apply(self, contours: &mut [Contour], indices: &[(usize, usize)]) {
        for &(c, i) in indices {
            if let Some(node) = contours.get_mut(c).and_then(|contour| contour.points.get_mut(i)) {
                node.pos = self.map(node.pos);
                node.handle_in = node.handle_in.map(|point| self.map(point));
                node.handle_out = node.handle_out.map(|point| self.map(point));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CurveNodeDrag {
    id: ObjectId,
    contours: Vec<Contour>,
    indices: Vec<(usize, usize)>,
    matrix: [f32; 6],
}

impl CurveNodeDrag {
    fn transformed(&self, transform: NodeGroupTransform) -> Vec<Contour> {
        let mut contours = self.contours.clone();
        transform.apply(&mut contours, &self.indices);
        contours
    }

    fn command(mut self) -> Command {
        if self.matrix != IDENTITY {
            map_contours(&mut self.contours, self.matrix);
        }
        Command::with_params(CURVE_NODES_SET, json!({"id": self.id.0, "contours": self.contours, "preserve_transform": true}))
    }
}

fn batch_curves(mut commands: Vec<Command>) -> Option<Command> {
    match commands.len() {
        0 => None,
        1 => commands.pop(),
        _ => Some(Command::with_params("edit.batch", json!({"commands": commands}))),
    }
}

pub fn editable(session: &Session, id: ObjectId) -> Option<Vec<Contour>> {
    let object = session.document.get_object(id)?;
    if object.locked || object.curve().is_none() {
        return None;
    }
    let mut contours = world_contours(object);
    let matrix = object_matrix(object);
    if matrix != IDENTITY {
        invert_matrix(&matrix)?;
        map_contours(&mut contours, matrix);
    }
    Some(contours)
}

pub fn node_contours(session: &Session, id: ObjectId) -> Option<(Vec<Contour>, bool)> {
    if let Some(contours) = editable(session, id) {
        return Some((contours, false));
    }
    let object = session.document.get_object(id)?;
    if object.locked || object.rotation_deg != 0.0 || object.shear_deg != 0.0 {
        return None;
    }
    preview_contours(object).map(|contours| (contours, true))
}

pub fn hit_part(contours: &[Contour], selected: &[(usize, usize)], point: [f32; 2], radius: f32) -> Option<NodePart> {
    let near = |p: [f64; 2]| distance(p32(p), point) <= radius;
    for &(c, i) in selected {
        let Some(node) = contours.get(c).and_then(|contour| contour.points.get(i)) else { continue };
        if node.handle_in.is_some_and(near) {
            return Some(NodePart::In(c, i));
        }
        if node.handle_out.is_some_and(near) {
            return Some(NodePart::Out(c, i));
        }
    }
    contours
        .iter()
        .enumerate()
        .flat_map(|(c, contour)| contour.points.iter().enumerate().map(move |(i, node)| (c, i, node)))
        .filter(|(_, _, node)| near(node.pos))
        .min_by(|a, b| distance(p32(a.2.pos), point).total_cmp(&distance(p32(b.2.pos), point)))
        .map(|(c, i, _)| NodePart::Anchor(c, i))
}

fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0] * d[0] + d[1] * d[1];
    let t = if l > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l).clamp(0.0, 1.0) } else { 0.0 };
    distance(p, [a[0] + d[0] * t, a[1] + d[1] * t])
}

pub fn segment_hit(contours: &[Contour], point: [f32; 2], radius: f32, tolerance: f64) -> Option<usize> {
    contours.iter().position(|contour| {
        flatten(contour, tolerance).iter().any(|(points, closed)| {
            let mut pairs: Vec<([f32; 2], [f32; 2])> = points.windows(2).map(|w| (w[0], w[1])).collect();
            if *closed {
                if let (Some(first), Some(last)) = (points.first(), points.last()) {
                    pairs.push((*last, *first));
                }
            }
            pairs.iter().any(|(a, b)| segment_distance(point, *a, *b) <= radius)
        })
    })
}

fn shifted(p: [f64; 2], d: [f64; 2]) -> [f64; 2] {
    [p[0] + d[0], p[1] + d[1]]
}

pub fn drag_contours(base: &[Contour], part: NodePart, selected: &[(usize, usize)], delta: [f32; 2]) -> Vec<Contour> {
    drag_part(base, part, selected, delta, false, false)
}

fn mirrored(pos: [f64; 2], h: [f64; 2]) -> [f64; 2] {
    [2.0 * pos[0] - h[0], 2.0 * pos[1] - h[1]]
}

fn scale_about(p: [f64; 2], origin: [f64; 2], s: [f64; 2]) -> [f64; 2] {
    [origin[0] + (p[0] - origin[0]) * s[0], origin[1] + (p[1] - origin[1]) * s[1]]
}

pub fn drag_part(base: &[Contour], part: NodePart, selected: &[(usize, usize)], delta: [f32; 2], shift: bool, alt: bool) -> Vec<Contour> {
    let d = p64(delta);
    let mut out = base.to_vec();
    match part {
        NodePart::Segment(c, i) => {
            if let Some(contour) = out.get_mut(c) {
                let n = contour.points.len();
                let j = (i + 1) % n.max(1);
                if n >= 2 && (contour.closed || j > i) {
                    let p = &mut contour.points[i];
                    let h = shifted(p.handle_out.unwrap_or(p.pos), d);
                    p.handle_out = Some(h);
                    if p.kind != NodeKind::Sharp && p.handle_in.is_some() {
                        p.handle_in = Some(mirrored(p.pos, h));
                    }
                    let q = &mut contour.points[j];
                    let h = shifted(q.handle_in.unwrap_or(q.pos), d);
                    q.handle_in = Some(h);
                    if q.kind != NodeKind::Sharp && q.handle_out.is_some() {
                        q.handle_out = Some(mirrored(q.pos, h));
                    }
                }
            }
            return out;
        }
        NodePart::Scale => {
            let Some(b) = selection_box(base, selected) else { return out };
            NodeGroupTransform::scale(b, d, shift).apply(&mut out, selected);
            return out;
        }
        _ => {}
    }
    match part {
        NodePart::Segment(..) | NodePart::Scale | NodePart::Rotate | NodePart::Shear(_) | NodePart::Align(_) | NodePart::Origin => {}
        NodePart::Anchor(..) => {
            NodeGroupTransform::Translate(d).apply(&mut out, selected);
        }
        NodePart::In(c, i) | NodePart::Out(c, i) => {
            if let Some(node) = out.get_mut(c).and_then(|contour| contour.points.get_mut(i)) {
                let is_in = matches!(part, NodePart::In(..));
                let pos = node.pos;
                let moved = if is_in { node.handle_in } else { node.handle_out }.map(|h| shifted(h, d)).map(|h| if shift { p64(constrain_angle(p32(pos), p32(h))) } else { h });
                if alt {
                    node.kind = NodeKind::Sharp;
                }
                let mirror = moved.filter(|_| node.kind != NodeKind::Sharp).map(|h| mirrored(pos, h));
                if is_in {
                    node.handle_in = moved;
                    if mirror.is_some() {
                        node.handle_out = mirror;
                    }
                } else {
                    node.handle_out = moved;
                    if mirror.is_some() {
                        node.handle_in = mirror;
                    }
                }
            }
        }
    }
    out
}

pub fn convert_command(id: ObjectId, contour: usize, node: &PathPoint) -> Command {
    let kind = if node.kind == NodeKind::Sharp { "smooth" } else { "sharp" };
    Command::with_params(CURVE_NODES_CONVERT, json!({ "id": id.0, "contour": contour, "nodes": [node.id.0], "kind": kind }))
}

pub fn join_command(session: &Session) -> Option<Command> {
    if session.selection.ids.len() < 2 {
        return None;
    }
    let command = Command::with_params(PATH_JOIN, json!({ "ids": session.selection.ids }));
    session.can_execute(&command).then_some(command)
}

pub(crate) fn node_context_menu(session: &Session) -> bool {
    session.active_tool == "tool.node" && selected_curves(session).next().is_some()
}

fn selected_curves(session: &Session) -> impl Iterator<Item = ObjectId> + '_ {
    session.selection.ids.iter().copied().filter(|&id| {
        session.document.get_object(id).is_some_and(|object| {
            !object.locked && object.pickable && object.curve().is_some()
                && invert_matrix(&object_matrix(object)).is_some()
        })
    })
}

pub fn insert_command(id: ObjectId, contour: usize, point: [f32; 2]) -> Command {
    Command::with_params(CURVE_NODES_INSERT, json!({ "id": id.0, "contour": contour, "point": point }))
}

impl NodeEdit {
    pub fn sync(&mut self, session: &Session) {
        let geometry_changed = self.point_order_generation != Some(session.generation);
        let target = match session.selection.ids.as_slice() {
            [id] => Some(*id),
            _ => selected_curves(session).find(|id| self.context == session.context && Some(*id) == self.target)
                .or_else(|| selected_curves(session).next()),
        };
        self.sync_target(session, target);
        if !self.other_selected.is_empty() {
            let eligible: HashSet<_> = selected_curves(session).collect();
            self.other_selected.retain(|id, selection| {
                if !eligible.contains(id) { return false; }
                if geometry_changed
                    && let Some(curve) = session.document.get_object(*id).and_then(|object| object.curve())
                {
                    let live: HashSet<_> = curve.contours().enumerate().flat_map(|(c, contour)| {
                        contour.points.iter().map(move |node| (c, node.id))
                    }).collect();
                    selection.retain(|point| live.contains(point));
                }
                !selection.is_empty()
            });
        }
        if !self.other_drags.is_empty() && (
            self.other_drags.iter().any(|curve| !self.other_selected.contains_key(&curve.id))
                || (geometry_changed && self.other_drags != self.selected_other_curves(session))
        ) {
            self.cancel();
        }
        if self.custom_origin.as_ref().is_some_and(|origin| origin.selection != self.node_signature()) {
            self.custom_origin = None;
        }
    }

    fn sync_target(&mut self, session: &Session, target: Option<ObjectId>) {
        let editable = target.and_then(|id| {
            node_contours(session, id).map(|(contours, primitive)| (id, contours, primitive))
        });
        let single = editable.as_ref().map(|(id, _, _)| *id);
        if single != self.target || self.context != session.context {
            let mut other_selected = HashMap::new();
            if self.context == session.context && single.is_some() && !self.primitive {
                other_selected = std::mem::take(&mut self.other_selected);
                if let Some(prior) = self.target.filter(|id| session.selection.ids.contains(id)) {
                    let points: HashSet<_> = self.selected.iter().filter_map(|&(c, i)| {
                        self.point_order.get(c)?.get(i).copied().map(|id| (c, id))
                    }).collect();
                    if !points.is_empty() {
                        other_selected.insert(prior, points);
                    }
                }
            }
            let custom_origin = (self.context == session.context).then(|| self.custom_origin.take()).flatten();
            *self = NodeEdit { target: single, context: session.context, alt: self.alt, shift: self.shift,
                transform_mode: self.transform_mode, other_selected, custom_origin, ..NodeEdit::default() };
            if let Some((id, contours, false)) = &editable {
                self.selected = self.other_indices(*id, contours);
                self.other_selected.remove(id);
            }
        }
        self.primitive = false;
        if let Some((_, contours, primitive)) = editable {
            let display_to_edit = self.target.and_then(|id| session.document.get_object(id))
                .filter(|_| !primitive).map(object_matrix)
                .filter(|matrix| *matrix != IDENTITY).and_then(|matrix| invert_matrix(&matrix));
            if self.base.as_ref().is_some_and(|base| *base != contours || self.display_to_edit != display_to_edit) {
                self.cancel();
            }
            self.display_to_edit = display_to_edit;
            self.primitive = primitive;
            if self.point_order_generation != Some(session.generation) {
                self.reconcile_point_order(&contours);
                self.point_order_generation = Some(session.generation);
            }
            self.selected.retain(|(c, i)| contours.get(*c).is_some_and(|contour| *i < contour.points.len()));
        }
    }

    pub(crate) fn sync_toolbar(&mut self, session: &Session) {
        if self.context != session.context || self.point_order_generation != Some(session.generation)
            || self.target.is_none_or(|id| !session.selection.ids.contains(&id))
            || (self.primitive && session.selection.ids.len() != 1)
        {
            self.sync(session);
        }
    }

    pub(crate) fn other_curves<'a>(&self, session: &'a Session) -> impl Iterator<Item = (ObjectId, Vec<Contour>)> + 'a {
        let target = self.target;
        selected_curves(session).filter(move |id| Some(*id) != target)
            .filter_map(|id| editable(session, id).map(|contours| (id, contours)))
    }

    pub(crate) fn other_node_selected(&self, id: ObjectId, contour: usize, node: PointId) -> bool {
        self.other_selected.get(&id).is_some_and(|points| points.contains(&(contour, node)))
    }

    fn other_indices(&self, id: ObjectId, contours: &[Contour]) -> Vec<(usize, usize)> {
        let Some(points) = self.other_selected.get(&id) else { return Vec::new(); };
        contours.iter().enumerate().flat_map(|(c, contour)| {
            contour.points.iter().enumerate().filter_map(move |(i, node)| points.contains(&(c, node.id)).then_some((c, i)))
        }).collect()
    }

    pub(crate) fn selected_count(&self) -> usize {
        self.selected.len() + self.other_selected.values().map(HashSet::len).sum::<usize>()
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected.clear();
        self.other_selected.clear();
        self.custom_origin = None;
    }

    pub(crate) fn has_cross_selection(&self) -> bool {
        !self.other_selected.is_empty()
    }

    fn set_other_selection(&mut self, session: &Session, contains: impl Fn([f32; 2]) -> bool) {
        let hits: Vec<_> = self.other_curves(session).filter_map(|(id, contours)| {
            let points: Vec<_> = contours.iter().enumerate().flat_map(|(c, contour)| {
                contour.points.iter().filter(|node| contains(p32(node.pos))).map(move |node| (c, node.id))
            }).collect();
            (!points.is_empty()).then_some((id, points))
        }).collect();
        for (id, points) in hits {
            self.other_selected.entry(id).or_default().extend(points);
        }
    }

    fn selected_other_curves(&self, session: &Session) -> Vec<CurveNodeDrag> {
        if self.other_selected.is_empty() { return Vec::new(); }
        self.other_curves(session).filter_map(|(id, contours)| {
            let indices = self.other_indices(id, &contours);
            let matrix = invert_matrix(&object_matrix(session.document.get_object(id)?))?;
            (!indices.is_empty()).then_some(CurveNodeDrag { id, contours, indices, matrix })
        }).collect()
    }

    pub(crate) fn separate_transforms(&self, session: &Session) -> bool {
        if self.grab.is_some_and(|(part, _)| part.allows_separate_transform()) {
            return !self.individual_origins.is_empty();
        }
        usize::from(!self.selected.is_empty()) + self.other_selected.len() > 1
            && session.tool_options.flag(
                "tool.node", dtool_core::context_toolbar::NODE_TRANSFORM_SEPARATELY,
            ).unwrap_or(false)
    }

    fn curve_origins(&self, active: &[Contour], others: &[CurveNodeDrag]) -> HashMap<ObjectId, [f64; 2]> {
        let primary = self.target.filter(|_| !self.selected.is_empty()).map(|id| (id, active));
        primary.into_iter().chain(others.iter().map(|curve| (curve.id, curve.contours.as_slice())))
            .filter_map(|(id, contours)| Some((id, p64(node_box_center(curve_outline_bounds(contours)?)))))
            .collect()
    }

    pub(crate) fn displayed_individual_origins(&self, session: &Session, active: &[Contour]) -> Vec<[f32; 2]> {
        if !self.separate_transforms(session)
            || !session.tool_options.flag("tool.node", "node.show_transform_origin").unwrap_or(false)
        {
            return Vec::new();
        }
        if !self.individual_origins.is_empty() {
            return self.individual_origins.values().copied().map(p32).collect();
        }
        self.curve_origins(active, &self.selected_other_curves(session)).into_values().map(p32).collect()
    }

    pub(crate) fn transform_box(&self, session: &Session, contours: &[Contour]) -> Option<[f32; 4]> {
        if !self.transform_mode || self.primitive {
            return None;
        }
        let from_curves = session.tool_options.flag("tool.node", dtool_core::context_toolbar::NODE_BOX_FROM_CURVES).unwrap_or(false);
        let minimum_nodes = if from_curves { 1 } else { 2 };
        if self.selected_count() < minimum_nodes {
            return None;
        }
        // Keep the drag handle attached to the preview. Refit the box when the
        // edit commits, since nodes outside the selection stay where they are.
        if self.grab.is_some_and(|(part, _)| part == NodePart::Rotate || part.is_shear())
            || ((from_curves || !self.individual_origins.is_empty()) && matches!(self.grab, Some((NodePart::Scale, _))))
            || matches!(self.grab, Some((NodePart::Align(_), _)))
        {
            return self.transform_bounds;
        }
        let bounds = |contours: &[Contour], selected: &[(usize, usize)]| {
            if from_curves && !selected.is_empty() {
                curve_outline_bounds(contours)
            } else {
                selection_box(contours, selected)
            }
        };
        let active = bounds(contours, &self.selected);
        let others = self.selected_other_curves(session);
        active.into_iter().chain(others.iter().filter_map(|curve| {
            let live = self.live_other_contours(curve.id);
            bounds(live.as_deref().unwrap_or(&curve.contours), &curve.indices)
        })).reduce(union_node_bounds)
    }

    pub(crate) fn hide_transform_box(&self, session: &Session) -> bool {
        self.live.is_some() && session.tool_options.flag(
            "tool.node", dtool_core::context_toolbar::NODE_HIDE_TRANSFORM_BOX,
        ).unwrap_or(false)
    }

    fn node_signature(&self) -> Vec<(ObjectId, usize, PointId)> {
        let mut points: Vec<_> = self.other_selected.iter().flat_map(|(&id, points)| {
            points.iter().map(move |&(c, point)| (id, c, point))
        }).collect();
        if let Some(id) = self.target {
            points.extend(self.selected.iter().filter_map(|&(c, i)| {
                Some((id, c, *self.point_order.get(c)?.get(i)?))
            }));
        }
        points.sort_unstable_by_key(|(id, c, point)| (id.0, *c, point.0));
        points
    }

    pub(crate) fn has_custom_origin(&self) -> bool { self.custom_origin.is_some() }

    pub(crate) fn reset_origin(&mut self) { self.custom_origin = None; }

    pub(crate) fn transform_origin(&self, session: &Session, bounds: [f32; 4]) -> Option<[f32; 2]> {
        (!self.separate_transforms(session) && session.tool_options.flag("tool.node", "node.show_transform_origin").unwrap_or(false))
            .then(|| self.origin_preview.or(self.gesture_origin)
                .or_else(|| self.custom_origin.as_ref().map(|origin| origin.point))
                .unwrap_or_else(|| self.transform_display_point(session, node_box_center(bounds))))
    }

    pub(crate) fn transform_display_point(&self, session: &Session, point: [f32; 2]) -> [f32; 2] {
        match self.group_transform {
            Some(transform @ (NodeGroupTransform::Rotate { .. } | NodeGroupTransform::Shear { .. })) => p32(transform.map(p64(point))),
            Some(translation @ NodeGroupTransform::Translate(_))
                if matches!(self.grab, Some((NodePart::Align(_), _))) =>
                p32(translation.map(p64(point))),
            Some(scale @ NodeGroupTransform::Scale { .. })
                if !self.individual_origins.is_empty()
                    || session.tool_options.flag("tool.node", dtool_core::context_toolbar::NODE_BOX_FROM_CURVES).unwrap_or(false) =>
                p32(scale.map(p64(point))),
            _ => point,
        }
    }

    pub(crate) fn alignment_handles(
        session: &Session, bounds: [f32; 4], radius: f32,
    ) -> impl Iterator<Item = (NodeAlignHandle, [f32; 2])> {
        let enabled = session.tool_options.flag(
            "tool.node", dtool_core::context_toolbar::NODE_SHOW_ALIGNMENT_HANDLES,
        ).unwrap_or(false);
        (0..2).flat_map(|axis| (0..3).map(move |edge| NodeAlignHandle { axis, edge }))
            .filter_map(move |handle| {
                let axis = handle.axis;
                if !enabled || (handle.edge != 1 && bounds[axis + 2] - bounds[axis] <= radius * 4.0) {
                    return None;
                }
                let offset = radius * 2.0;
                let coordinate = handle.coordinate(bounds);
                let point = if axis == 0 { [coordinate, bounds[3] + offset] } else { [bounds[0] - offset, coordinate] };
                Some((handle, point))
            })
    }

    fn hit_transform_handle(
        &self, session: &Session, contours: &[Contour], point: [f32; 2], radius: f32,
    ) -> Option<(NodePart, [f32; 4])> {
        let bounds = self.transform_box(session, contours)?;
        let offset = radius * (crate::canvas_input::ROTATE_PX / crate::canvas_input::HANDLE_PX);
        [
            (NodePart::Origin, self.transform_origin(session, bounds)),
            (NodePart::Rotate, Some(node_rotate_handle(bounds, offset))),
            (NodePart::Scale, Some([bounds[2], bounds[3]])),
        ].into_iter()
            .chain(node_shear_handles(bounds, radius).map(|(side, point)| (NodePart::Shear(side), Some(point))))
            .chain(Self::alignment_handles(session, bounds, radius).map(|(handle, point)| (NodePart::Align(handle), Some(point))))
            .find_map(|(part, handle)| {
            handle.filter(|&p| distance(p, point) <= radius).map(|_| (part, bounds))
        })
    }

    pub(crate) fn transform_handle(&self, session: &Session, point: [f32; 2], radius: f32) -> Option<NodePart> {
        if let Some((part, _)) = self.grab {
            return Some(part);
        }
        if !self.transform_mode { return None; }
        self.hit_transform_handle(session, &self.contours(session)?, point, radius).map(|(part, _)| part)
    }

    pub(crate) fn claims_alignment_handle(&self, session: &Session, point: [f32; 2], radius: f32) -> bool {
        self.context == session.context && matches!(self.transform_handle(session, point, radius), Some(NodePart::Align(_)))
    }

    pub(crate) fn pointer_part(&self, session: &Session, point: [f32; 2], radius: f32) -> Option<NodePart> {
        if self.primitive { return None; }
        if let Some((part, _)) = self.grab { return Some(part); }
        let contours = self.contours(session);
        if let Some(contours) = &contours {
            if self.transform_mode
                && let Some((part, _)) = self.hit_transform_handle(session, contours, point, radius)
            {
                return Some(part);
            }
            if let Some(part) = hit_part(contours, &self.selected, point, radius) {
                return Some(part);
            }
        }
        if let Some((_, part)) = self.hit_other_node(session, point, radius) {
            return Some(part);
        }
        segment_at(contours.as_deref()?, point, radius).map(|(c, i)| NodePart::Segment(c, i))
    }

    pub(crate) fn dragging_transform_handle(&self) -> bool { self.transform_bounds.is_some() }

    pub(crate) fn snap_transform_handle(&self, session: &Session, pointer: [f32; 2]) -> Option<([f32; 2], Option<[f32; 2]>)> {
        let bounds = self.transform_bounds?;
        let (part, start) = self.grab?;
        if pointer == start || part == NodePart::Rotate
            || ((part == NodePart::Scale || part.is_shear()) && (self.shift || !self.individual_origins.is_empty()))
            || (part.is_shear() && self.alt)
        {
            return Some((pointer, None));
        }
        let exclude: Vec<_> = self.drag_targets().collect();
        if let NodePart::Align(handle) = part {
            let axis = handle.axis;
            if pointer[axis] == start[axis] {
                return Some((pointer, None));
            }
            let mut anchor = node_box_center(bounds);
            anchor[axis] = handle.coordinate(bounds) + pointer[axis] - start[axis];
            let snap = crate::canvas_tools::axis_snaps(session, &[anchor], &exclude)[axis];
            let mut snapped = pointer;
            if let Some(snap) = snap {
                snapped[axis] += snap.delta;
            }
            return Some((snapped, snap.map(|snap| snap.point)));
        }
        if let NodePart::Shear(side) = part {
            let radius = crate::canvas_input::HANDLE_PX / session.view.zoom.max(1e-4);
            let axis = side.axis();
            let (_, mut grip) = node_shear_handles(bounds, radius).find(|(edge, _)| *edge == side)?;
            grip[axis] += pointer[axis] - start[axis];
            let (target, mark) = crate::canvas_tools::snap_point(session, grip, &exclude);
            let mut snapped = pointer;
            snapped[axis] += target[axis] - grip[axis];
            return Some((snapped, mark.filter(|_| target[axis] != grip[axis])));
        }
        let origin = if part == NodePart::Origin { self.gesture_origin? } else { [bounds[2], bounds[3]] };
        let corner = [origin[0] + (pointer[0] - start[0]), origin[1] + (pointer[1] - start[1])];
        let (target, mark) = crate::canvas_tools::snap_point(session, corner, &exclude);
        Some(([pointer[0] + (target[0] - corner[0]), pointer[1] + (target[1] - corner[1])], mark))
    }

    fn focus_node_curve(&mut self, session: &Session, point: [f32; 2], radius: f32) {
        if session.selection.ids.len() < 2 || self.grab.is_some() {
            return;
        }
        if self.contours(session).is_some_and(|contours| {
            hit_part(&contours, &self.selected, point, radius).is_some()
        }) {
            return;
        }
        if let Some((id, _)) = self.hit_other_node(session, point, radius) {
            self.sync_target(session, Some(id));
        }
    }

    fn hit_other_node(&self, session: &Session, point: [f32; 2], radius: f32) -> Option<(ObjectId, NodePart)> {
        if session.selection.ids.len() < 2 { return None; }
        self.other_curves(session).find_map(|(id, contours)| {
            hit_part(&contours, &self.other_indices(id, &contours), point, radius).map(|part| (id, part))
        })
    }

    fn reconcile_point_order(&mut self, contours: &[Contour]) {
        let order: Vec<Vec<_>> = contours.iter().map(|contour| contour.points.iter().map(|node| node.id).collect()).collect();
        if !self.point_order.is_empty() && !self.selected.is_empty() && order != self.point_order {
            let positions: Vec<std::collections::HashMap<_, _>> = order.iter().map(|ids| {
                ids.iter().enumerate().map(|(index, &id)| (id, index)).collect()
            }).collect();
            self.selected.retain_mut(|(contour, index)| {
                let Some(id) = self.point_order.get(*contour).and_then(|ids| ids.get(*index)) else { return false };
                let Some(&next) = positions.get(*contour).and_then(|positions| positions.get(id)) else { return false };
                *index = next;
                true
            });
        }
        self.point_order = order;
    }

    pub fn contours(&self, session: &Session) -> Option<Vec<Contour>> {
        self.live.clone().or_else(|| node_contours(session, self.target?).map(|(contours, _)| contours))
    }

    pub(crate) fn unproject_contours(&self, contours: &mut [Contour]) {
        if let Some(matrix) = self.display_to_edit {
            map_contours(contours, matrix);
        }
    }

    fn preserve_edit_transform(&self, mut command: Command) -> Command {
        if self.display_to_edit.is_some() {
            command.params["preserve_transform"] = json!(true);
        }
        command
    }

    pub(crate) fn live_edit_contours(&self) -> Option<std::borrow::Cow<'_, [Contour]>> {
        let live = self.live.as_deref()?;
        Some(if let Some(matrix) = self.display_to_edit {
            let mut contours = live.to_vec();
            map_contours(&mut contours, matrix);
            std::borrow::Cow::Owned(contours)
        } else {
            std::borrow::Cow::Borrowed(live)
        })
    }

    pub(crate) fn live_other_contours(&self, id: ObjectId) -> Option<Vec<Contour>> {
        let transform = self.group_transform?.with_origin(self.individual_origins.get(&id).copied());
        self.other_drags.iter().find(|curve| curve.id == id).map(|curve| curve.transformed(transform))
    }

    pub(crate) fn live_curve_overrides(&self) -> Option<Vec<(ObjectId, Vec<Contour>)>> {
        let mut curves = vec![(self.target?, self.live_edit_contours()?.into_owned())];
        if let Some(transform) = self.group_transform {
            curves.extend(self.other_drags.iter().map(|curve| {
                let mut contours = curve.transformed(transform.with_origin(self.individual_origins.get(&curve.id).copied()));
                if curve.matrix != IDENTITY {
                    map_contours(&mut contours, curve.matrix);
                }
                (curve.id, contours)
            }));
        }
        Some(curves)
    }

    pub(crate) fn drag_targets(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.target.into_iter().chain(self.other_drags.iter().map(|curve| curve.id))
    }

    pub fn select_at(&mut self, session: &Session, point: [f32; 2], radius: f32) -> bool {
        self.sync(session);
        self.focus_node_curve(session, point, radius);
        let Some((contours, _)) = self.target.and_then(|id| node_contours(session, id)) else { return false };
        match hit_part(&contours, &[], point, radius) {
            Some(NodePart::Anchor(c, i)) => {
                if !self.selected.contains(&(c, i)) {
                    self.other_selected.clear();
                    self.selected = vec![(c, i)];
                }
                true
            }
            _ => false,
        }
    }

    pub(crate) fn split_after_node_command(&self, session: &Session) -> Option<Command> {
        if self.has_cross_selection() { return None; }
        let id = self.target?;
        let [(subpath, index)] = self.selected.as_slice() else { return None };
        let object = session.document.get_object(id)?;
        if object.locked || invert_matrix(&object_matrix(object)).is_none() { return None; }
        let contour = object.curve()?.contours().nth(*subpath)?;
        let from = contour.points.get(*index)?;
        let to = contour.points.get(index + 1).or_else(|| {
            (contour.closed && contour.points.len() > 1).then(|| &contour.points[0])
        })?;
        Some(Command::with_params(PATH_POINT_ADD, json!({
            "id": id.0,
            "subpath": subpath,
            "segment": { "from_id": from.id.0, "to_id": to.id.0 },
            "preserve_transform": true,
        })))
    }

    pub fn break_command(&self, session: &Session) -> Option<Command> {
        if self.has_cross_selection() { return None; }
        let id = self.target?;
        let [(c, i)] = self.selected.as_slice() else { return None };
        let contours = editable(session, id)?;
        let contour = contours.get(*c)?;
        let node = contour.points.get(*i)?;
        if !contour.closed && (*i == 0 || *i + 1 == contour.points.len()) {
            return None;
        }
        Some(self.preserve_edit_transform(Command::with_params(PATH_BREAK, json!({ "id": id.0, "point": { "id": node.id.0 } }))))
    }

    pub(crate) fn reverse_command(&self, session: &Session) -> Option<Command> {
        if self.has_cross_selection() { return None; }
        let command = self.preserve_edit_transform(Command::with_params(
            dtool_core::commands::path_points::REVERSE,
            json!({"id": self.target?.0, "subpath": self.selected.first().map_or(0, |(index, _)| *index)}),
        ));
        session.can_execute(&command).then_some(command)
    }

    pub(crate) fn smooth_command(&self, session: &Session) -> Option<Command> {
        if self.has_cross_selection() { return None; }
        let id = self.target?;
        let object = session.document.get_object(id)?;
        if object.locked || self.primitive || invert_matrix(&object_matrix(object)).is_none() { return None; }
        let curve = object.curve()?;
        let mut subpaths: Vec<_> = self.selected.iter().map(|&(subpath, _)| subpath).collect();
        if subpaths.is_empty() { subpaths.push(0); }
        subpaths.sort_unstable();
        subpaths.dedup();
        let mut commands: Vec<_> = subpaths.into_iter().filter(|&subpath| {
            curve.contours().nth(subpath).is_some_and(|contour| contour.points.len() >= 3)
        }).map(|subpath| Command::with_params(dtool_core::commands::path_points::SMOOTH, json!({
            "id": id.0, "subpath": subpath, "preserve_transform": true,
        }))).collect();
        match commands.len() {
            0 => None,
            1 => commands.pop(),
            _ => Some(Command::with_params("edit.batch", json!({ "commands": commands }))),
        }
    }

    pub fn claims_delete(&self) -> bool {
        self.target.is_some() && !self.primitive && self.selected_count() != 0
    }

    pub fn press(&mut self, session: &Session, point: [f32; 2], shift: bool, alt: bool, radius: f32) -> NodePress {
        self.sync(session);
        if self.transform_mode
            && let Some(contours) = self.contours(session)
            && let Some((part, bounds)) = self.hit_transform_handle(session, &contours, point, radius)
        {
            let separate = part.allows_separate_transform() && self.separate_transforms(session);
            self.gesture_origin = if part.is_shear() {
                (!separate).then(|| {
                    self.transform_origin(session, bounds).filter(|_| self.has_custom_origin())
                }).flatten()
            } else if matches!(part, NodePart::Origin | NodePart::Rotate) {
                Some(self.transform_origin(session, bounds).unwrap_or_else(|| node_box_center(bounds)))
            } else {
                None
            };
            self.grab = Some((part, point));
            self.base = Some(contours);
            self.live = None;
            self.transform_bounds = Some(bounds);
            if part != NodePart::Origin {
                self.other_drags = self.selected_other_curves(session);
            }
            if separate {
                self.individual_origins = self.curve_origins(self.base.as_deref().unwrap_or_default(), &self.other_drags);
            }
            return NodePress::Grab;
        }
        self.focus_node_curve(session, point, radius);
        let (Some(id), Some((contours, _))) = (self.target, self.target.and_then(|id| node_contours(session, id))) else { return NodePress::Miss };
        match hit_part(&contours, &self.selected, point, radius) {
            Some(NodePart::Anchor(..)) if alt && self.primitive => NodePress::Grab,
            Some(NodePart::Anchor(c, i)) if alt => NodePress::Run(self.preserve_edit_transform(convert_command(id, c, &contours[c].points[i]))),
            Some(part) => {
                if let NodePart::Anchor(c, i) = part {
                    if shift {
                        if let Some(at) = self.selected.iter().position(|s| *s == (c, i)) {
                            self.selected.remove(at);
                            return NodePress::Grab;
                        }
                        self.selected.push((c, i));
                    } else if !self.selected.contains(&(c, i)) {
                        self.other_selected.clear();
                        self.selected = vec![(c, i)];
                    }
                    self.other_drags = self.selected_other_curves(session);
                }
                self.grab = Some((part, point));
                self.base = Some(contours);
                self.live = None;
                NodePress::Grab
            }
            None => {
                if let Some((c, i)) = segment_at(&contours, point, radius) {
                    self.grab = Some((NodePart::Segment(c, i), point));
                    self.base = Some(contours);
                    self.live = None;
                    return NodePress::Grab;
                }
                NodePress::Miss
            }
        }
    }

    pub fn select_all(&mut self, session: &Session) -> bool {
        self.sync(session);
        let Some((contours, _)) = self.target.and_then(|id| node_contours(session, id)) else { return false };
        self.other_selected.clear();
        self.selected = contours.iter().enumerate().flat_map(|(c, contour)| (0..contour.points.len()).map(move |i| (c, i))).collect();
        self.set_other_selection(session, |_| true);
        true
    }

    pub(crate) fn select_rect(
        &mut self, session: &Session, rect: egui::Rect, add: bool, project: impl Fn([f32; 2]) -> egui::Pos2,
    ) {
        let Some(contours) = self.target.and_then(|id| editable(session, id)) else { return; };
        if !add {
            self.clear_selection();
        }
        for (c, contour) in contours.iter().enumerate() {
            for (i, node) in contour.points.iter().enumerate() {
                if rect.contains(project(p32(node.pos))) && !self.selected.contains(&(c, i)) {
                    self.selected.push((c, i));
                }
            }
        }
        self.set_other_selection(session, |point| rect.contains(project(point)));
    }

    pub(crate) fn align_command(&self, session: &Session, axis: usize, fraction: f64) -> Option<Command> {
        let id = self.target?;
        let primary = CurveNodeDrag {
            id,
            contours: editable(session, id)?,
            indices: self.selected.clone(),
            matrix: self.display_to_edit.unwrap_or(IDENTITY),
        };
        let curves = std::iter::once(primary).chain(self.selected_other_curves(session)).collect::<Vec<_>>();
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for curve in &curves {
            for &(c, i) in &curve.indices {
                let coordinate = curve.contours.get(c)?.points.get(i)?.pos[axis];
                min = min.min(coordinate);
                max = max.max(coordinate);
            }
        }
        if min >= max { return None; }
        let coordinate = min + (max - min) * fraction;
        batch_curves(curves.into_iter().filter_map(|mut curve| {
            let mut changed = false;
            for &(c, i) in &curve.indices {
                let node = &mut curve.contours[c].points[i];
                let offset = coordinate - node.pos[axis];
                changed |= offset != 0.0;
                node.pos[axis] = coordinate;
                for handle in [&mut node.handle_in, &mut node.handle_out].into_iter().flatten() {
                    handle[axis] += offset;
                }
            }
            changed.then(|| curve.command())
        }).collect())
    }

    pub fn close_command(&self, session: &Session) -> Option<Command> {
        if self.has_cross_selection() { return None; }
        let id = self.target?;
        let contours = editable(session, id)?;
        let c = self.selected.first().map_or(0, |(c, _)| *c);
        let contour = contours.get(c)?;
        (!contour.closed && contour.points.len() >= 2).then(|| {
            self.preserve_edit_transform(Command::with_params(PATH_CLOSE, json!({ "target": format!("node:{}", id.0), "subpath": c })))
        })
    }

    pub fn moving_points(&self, point: [f32; 2]) -> Option<Vec<[f32; 2]>> {
        let (Some((NodePart::Anchor(..), start)), Some(base)) = (self.grab, self.base.as_ref()) else { return None };
        let d = [point[0] - start[0], point[1] - start[1]];
        let points: Vec<[f32; 2]> = std::iter::once((base.as_slice(), self.selected.as_slice()))
            .chain(self.other_drags.iter().map(|curve| (curve.contours.as_slice(), curve.indices.as_slice())))
            .flat_map(|(contours, indices)| indices.iter().filter_map(|&(c, i)| contours.get(c)?.points.get(i).map(|n| p32(n.pos))))
            .map(|p| [p[0] + d[0], p[1] + d[1]]).collect();
        (!points.is_empty()).then_some(points)
    }

    pub(crate) fn snap_handle(
        &mut self, session: &Session, pointer: [f32; 2], tolerance: f32,
    ) -> Option<([f32; 2], Option<[f32; 2]>)> {
        self.construction_snap = None;
        let (part, start) = self.grab?;
        let (c, i, incoming) = match part {
            NodePart::In(c, i) => (c, i, true),
            NodePart::Out(c, i) => (c, i, false),
            _ => return None,
        };
        if pointer == start || self.shift || self.alt {
            return Some((pointer, None));
        }
        let contour = self.base.as_ref()?.get(c)?;
        let anchor = contour.points.get(i)?;
        let origin = if incoming { anchor.handle_in? } else { anchor.handle_out? };
        let origin = p32(origin);
        let tip = [origin[0] + (pointer[0] - start[0]), origin[1] + (pointer[1] - start[1])];
        let construction = session.tool_options.flag("tool.node", "node.snap_handle_construction").unwrap_or(false);
        if construction
            && let Some(neighbor) = handle_neighbor(contour, i, incoming)
        {
            let reference = if incoming { neighbor.handle_out } else { neighbor.handle_in };
            self.construction_snap = construction_handle_point(
                p32(anchor.pos), p32(neighbor.pos), reference.map(p32), tip, tolerance,
            );
        }
        let global = session.tool_options.flag("tool.node", "node.snap_handle_global").unwrap_or(true);
        let (position, mark) = if let Some(snap) = self.construction_snap {
            (snap.moving[1], None)
        } else if global {
            crate::canvas_tools::snap_point(session, tip, self.target.as_slice())
        } else {
            (tip, None)
        };
        Some(([pointer[0] + (position[0] - tip[0]), pointer[1] + (position[1] - tip[1])], mark))
    }

    pub(crate) fn snap_anchors(
        &self, session: &Session, pointer: [f32; 2], tolerance: f32,
        align: bool, geometry: bool, all_selected: bool,
    ) -> Option<NodeAnchorSnap> {
        let (NodePart::Anchor(c, i), start) = self.grab? else { return None; };
        let base = self.base.as_ref()?;
        let delta = [pointer[0] - start[0], pointer[1] - start[1]];
        if delta == [0.0, 0.0] { return None; }
        let grabbed = (c, i);
        let selected: std::collections::HashSet<_> = self.selected.iter().copied().collect();
        if !selected.contains(&grabbed) { return None; }
        let moving = if all_selected { self.selected.as_slice() } else { std::slice::from_ref(&grabbed) };
        let moving: Vec<_> = if all_selected {
            self.moving_points(pointer)?
        } else {
            moving.iter().filter_map(|&(c, i)| base.get(c)?.points.get(i)).map(|node| {
                let pos = p32(node.pos);
                [pos[0] + delta[0], pos[1] + delta[1]]
            }).collect()
        };
        let others: Vec<_> = self.other_curves(session).collect();
        let mut stationary = Vec::new();
        let mut segments = Vec::new();
        for (c, contour) in base.iter().enumerate() {
            for (i, node) in contour.points.iter().enumerate() {
                if !selected.contains(&(c, i)) {
                    stationary.push(p32(node.pos));
                }
            }
            if geometry {
                // Segments attached to a dragged anchor change during the gesture.
                segments.extend((0..segment_count(contour)).filter(|&i| {
                    !selected.contains(&(c, i)) && !selected.contains(&(c, (i + 1) % contour.points.len()))
                }).map(|i| (contour, i)));
            }
        }
        for (id, contours) in &others {
            for (c, contour) in contours.iter().enumerate() {
                let is_moving = |i: usize| self.other_node_selected(*id, c, contour.points[i].id);
                stationary.extend(contour.points.iter().enumerate().filter_map(|(i, node)| {
                    (!is_moving(i)).then_some(p32(node.pos))
                }));
                if geometry {
                    segments.extend((0..segment_count(contour)).filter(|&i| {
                        !is_moving(i) && !is_moving((i + 1) % contour.points.len())
                    }).map(|i| (contour, i)));
                }
            }
        }
        if align && !geometry {
            // Geometry mode compares nearby nodes and segments together below.
            // With alignment alone, keep a nearby node as a single two-axis target.
            stationary.sort_unstable_by(|a, b| a[0].total_cmp(&b[0]));
            let hit = moving.iter().flat_map(|&source| {
                let start = stationary.partition_point(|p| p[0] < source[0] - tolerance);
                stationary[start..].iter().copied().take_while(move |p| p[0] <= source[0] + tolerance)
                    .map(move |target| (distance(source, target), source, target))
            }).filter(|(d, _, _)| *d <= tolerance).min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, source, target)) = hit {
                return Some(NodeAnchorSnap::Point {
                    offset: [target[0] - source[0], target[1] - source[1]], target,
                });
            }
        }
        if geometry {
            let mut radius = tolerance;
            let mut best = None;
            for &source in &moving {
                for &target in &stationary {
                    let d = distance(source, target);
                    if d <= radius && (best.is_none() || d < radius) {
                        radius = d;
                        best = Some((source, target));
                    }
                }
                for &(contour, index) in &segments {
                    if let Some(target) = segment_point_within(contour, index, p64(source), f64::from(radius)) {
                        let target = p32(target);
                        let d = distance(source, target);
                        if best.is_none() || d < radius {
                            radius = d;
                            best = Some((source, target));
                        }
                    }
                }
            }
            if let Some((source, target)) = best {
                // Keep both coordinates together so axis snapping cannot pull the node off the path.
                return Some(NodeAnchorSnap::Point {
                    offset: [target[0] - source[0], target[1] - source[1]], target,
                });
            }
        }
        if !align {
            return None;
        }
        let mut best: [Option<(f32, [f32; 2])>; 2] = [None; 2];
        for axis in 0..2 {
            if axis == 1 {
                stationary.sort_unstable_by(|a, b| a[axis].total_cmp(&b[axis]));
            }
            for source in &moving {
                let next = stationary.partition_point(|p| p[axis] < source[axis]);
                for target in stationary[next.saturating_sub(1)..next.saturating_add(1).min(stationary.len())].iter().copied() {
                    let offset = target[axis] - source[axis];
                    if offset.abs() <= tolerance && best[axis].is_none_or(|(previous, _)| offset.abs() < previous.abs()) {
                        best[axis] = Some((offset, target));
                    }
                }
            }
        }
        best[0].or(best[1])?;
        Some(NodeAnchorSnap::Axes(std::array::from_fn(|axis| {
            best[axis].map(|(offset, target)| (offset, target[axis]))
        })))
    }

    pub fn insert_at(&self, session: &Session, point: [f32; 2], radius: f32, tolerance: f64) -> Option<Command> {
        let id = self.target?;
        let contours = editable(session, id)?;
        if hit_part(&contours, &[], point, radius).is_some() {
            return None;
        }
        segment_hit(&contours, point, radius, tolerance).map(|c| {
            let point = self.display_to_edit.map_or(point, |matrix| dtool_core::geometry::apply_matrix(&matrix, point));
            self.preserve_edit_transform(insert_command(id, c, point))
        })
    }

    pub(crate) fn double_click(&mut self, session: &Session, point: [f32; 2], radius: f32, tolerance: f64) -> Option<Command> {
        self.focus_node_curve(session, point, radius);
        let id = self.target?;
        let contours = editable(session, id)?;
        if let Some(NodePart::Anchor(c, i)) = hit_part(&contours, &[], point, radius) {
            self.grab = None;
            self.base = None;
            self.live = None;
            self.selected = vec![(c, i)];
            return Some(self.preserve_edit_transform(convert_command(id, c, &contours[c].points[i])));
        }
        self.insert_at(session, point, radius, tolerance)
    }

    fn selected_nodes<'a>(&self, session: &'a Session) -> Vec<(ObjectId, usize, &'a PathPoint)> {
        let mut nodes = Vec::new();
        for id in selected_curves(session) {
            let Some(curve) = session.document.get_object(id).and_then(|object| object.curve()) else { continue; };
            if Some(id) == self.target {
                nodes.extend(self.selected.iter().filter_map(|&(c, i)| {
                    curve.contours().nth(c)?.points.get(i).map(|node| (id, c, node))
                }));
            } else if let Some(selected) = self.other_selected.get(&id) {
                nodes.extend(curve.contours().enumerate().flat_map(|(c, contour)| {
                    contour.points.iter().filter(move |node| selected.contains(&(c, node.id)))
                        .map(move |node| (id, c, node))
                }));
            }
        }
        nodes
    }

    pub(crate) fn selected_node_kinds(&self, session: &Session) -> Vec<NodeKind> {
        self.selected_nodes(session).into_iter().map(|(_, _, node)| node.kind).collect()
    }

    pub(crate) fn convert_selected(&self, session: &Session, kind: NodeKind) -> Option<Command> {
        let mut groups = std::collections::BTreeMap::<(u64, usize), Vec<u64>>::new();
        for (id, c, node) in self.selected_nodes(session) {
            if node.kind != kind {
                groups.entry((id.0, c)).or_default().push(node.id.0);
            }
        }
        let kind = match kind { NodeKind::Sharp => "sharp", NodeKind::Smooth => "smooth", NodeKind::Smart => "smart" };
        batch_curves(groups.into_iter().map(|((id, contour), nodes)| {
            Command::with_params(CURVE_NODES_CONVERT, json!({
                "id": id, "contour": contour, "nodes": nodes, "kind": kind, "preserve_transform": true,
            }))
        }).collect())
    }

    pub fn delete_commands(&mut self, session: &Session) -> Vec<Command> {
        let (Some(id), Some(contours)) = (self.target, self.target.and_then(|id| editable(session, id))) else { return Vec::new() };
        let primary = CurveNodeDrag { id, contours, indices: self.selected.clone(), matrix: IDENTITY };
        let others = self.selected_other_curves(session);
        let across_curves = !others.is_empty();
        let commands = std::iter::once(primary).chain(others).flat_map(|curve| {
            let mut by_contour = std::collections::BTreeMap::<usize, Vec<u64>>::new();
            for (c, i) in curve.indices {
                if let Some(node) = curve.contours.get(c).and_then(|contour| contour.points.get(i)) {
                    by_contour.entry(c).or_default().push(node.id.0);
                }
            }
            by_contour.into_iter().rev().map(move |(c, nodes)| {
                Command::with_params(CURVE_NODES_DELETE, json!({
                    "id": curve.id.0, "contour": c, "nodes": nodes, "preserve_transform": true,
                }))
            })
        }).collect();
        self.clear_selection();
        if across_curves { batch_curves(commands).into_iter().collect() } else { commands }
    }

    pub fn drag(&mut self, point: [f32; 2]) {
        let (Some((part, start)), Some(base)) = (self.grab, self.base.as_ref()) else { return };
        let delta = [point[0] - start[0], point[1] - start[1]];
        self.group_transform = None;
        if part == NodePart::Origin {
            self.origin_preview = self.gesture_origin.map(|origin| {
                [origin[0] + delta[0], origin[1] + delta[1]]
            });
            return;
        }
        if delta == [0.0, 0.0] {
            self.live = None;
        } else if let Some(bounds) = self.transform_bounds {
            let transform = if part == NodePart::Rotate {
                let origin = p64(self.gesture_origin.unwrap_or_else(|| node_box_center(bounds)));
                let a = [f64::from(start[0]) - origin[0], f64::from(start[1]) - origin[1]];
                let b = [f64::from(point[0]) - origin[0], f64::from(point[1]) - origin[1]];
                let cross = a[0] * b[1] - a[1] * b[0];
                let dot = a[0] * b[0] + a[1] * b[1];
                let mut angle = cross.atan2(dot);
                if self.shift {
                    let step = 15.0_f64.to_radians();
                    angle = (angle / step).round() * step;
                }
                let (sin, cos) = angle.sin_cos();
                NodeGroupTransform::Rotate { origin, sin, cos }
            } else if let NodePart::Align(handle) = part {
                let mut translation = [0.0; 2];
                translation[handle.axis] = f64::from(delta[handle.axis]);
                NodeGroupTransform::Translate(translation)
            } else if let NodePart::Shear(side) = part {
                let axis = side.axis();
                let (grip, opposite) = side.edges(bounds);
                let origin = self.gesture_origin.map(|origin| origin[1 - axis])
                    .filter(|origin| (grip - origin).abs() > 1e-6)
                    .unwrap_or_else(|| if self.ctrl { grip.midpoint(opposite) } else { opposite });
                let mut angle = (f64::from(delta[axis]) / f64::from(grip - origin)).atan().to_degrees();
                if self.shift {
                    let step = f64::from(crate::canvas_tools::ROTATE_STEP_DEG);
                    angle = (angle / step).round() * step;
                }
                NodeGroupTransform::Shear { axis, origin: f64::from(origin), factor: angle.clamp(-89.0, 89.0).to_radians().tan() }
            } else {
                NodeGroupTransform::scale(bounds, p64(delta), self.shift)
            };
            let mut contours = base.clone();
            let origin = self.target.and_then(|id| self.individual_origins.get(&id).copied());
            transform.with_origin(origin).apply(&mut contours, &self.selected);
            self.live = Some(contours);
            self.group_transform = Some(transform);
        } else {
            self.live = Some(drag_part(base, part, &self.selected, delta, self.shift, self.alt));
            if matches!(part, NodePart::Anchor(..)) && !self.other_drags.is_empty() {
                self.group_transform = Some(NodeGroupTransform::Translate(p64(delta)));
            }
        }
    }

    pub fn release(&mut self) -> Option<Command> {
        self.grab = None;
        self.construction_snap = None;
        let base = self.base.take();
        self.transform_bounds = None;
        let origin = self.gesture_origin.take();
        if let Some(point) = self.origin_preview.take().filter(|point| Some(*point) != origin) {
            self.custom_origin = Some(NodeOrigin { point, selection: self.node_signature() });
        }
        let others = std::mem::take(&mut self.other_drags);
        let individual_origins = std::mem::take(&mut self.individual_origins);
        let transform = self.group_transform.take();
        let mut contours = self.live.take()?;
        let id = self.target?;
        let mut commands = Vec::new();
        if base.as_ref() != Some(&contours) {
            self.unproject_contours(&mut contours);
            let name = if self.primitive { SHAPE_CONVERT } else { CURVE_NODES_SET };
            let mut command = self.preserve_edit_transform(Command::with_params(name, json!({ "id": id.0, "contours": contours })));
            if !others.is_empty() {
                command.params["preserve_transform"] = json!(true);
            }
            commands.push(command);
        }
        if let Some(transform) = transform {
            commands.extend(others.into_iter().filter_map(|mut curve| {
                let contours = curve.transformed(transform.with_origin(individual_origins.get(&curve.id).copied()));
                if contours == curve.contours { return None; }
                curve.contours = contours;
                Some(curve.command())
            }));
        }
        batch_curves(commands)
    }

    pub fn cancel(&mut self) {
        self.grab = None;
        self.construction_snap = None;
        self.base = None;
        self.live = None;
        self.marquee = None;
        self.other_drags.clear();
        self.individual_origins.clear();
        self.group_transform = None;
        self.transform_bounds = None;
        self.gesture_origin = None;
        self.origin_preview = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstructionSnap {
    pub(crate) moving: [[f32; 2]; 2],
    pub(crate) reference: [[f32; 2]; 2],
    pub(crate) triangle: Option<[[f32; 2]; 3]>,
}

fn handle_neighbor(contour: &Contour, index: usize, incoming: bool) -> Option<&PathPoint> {
    let len = contour.points.len();
    if len < 2 { return None; }
    let neighbor_index = match (incoming, index, contour.closed) {
        (true, 0, true) => len - 1,
        (true, 0, false) => return None,
        (true, i, _) => i - 1,
        (false, i, true) => (i + 1) % len,
        (false, i, false) => i + 1,
    };
    contour.points.get(neighbor_index)
}

fn construction_handle_point(
    anchor: [f32; 2], neighbor: [f32; 2], neighbor_tip: Option<[f32; 2]>, tip: [f32; 2], tolerance: f32,
) -> Option<ConstructionSnap> {
    let chord_reference = [anchor, neighbor];
    let tangent_reference = [neighbor, neighbor_tip.unwrap_or(neighbor)];
    let anchor = egui::Vec2::from(anchor);
    let neighbor_pos = egui::Vec2::from(neighbor);
    let chord = (neighbor_pos - anchor).normalized();
    let tangent = neighbor_tip.map(|tip| (egui::Vec2::from(tip) - neighbor_pos).normalized())
        .unwrap_or_default();
    let reflected = 2.0 * tangent.dot(chord) * chord - tangent;
    let tip = egui::Vec2::from(tip);
    let delta = tip - anchor;
    let nearest = [(chord, chord_reference), (chord.rot90(), chord_reference),
        (tangent, tangent_reference), (tangent.rot90(), tangent_reference), (reflected, tangent_reference)].into_iter()
        .filter(|(direction, _)| direction.length_sq() > 0.0)
        .map(|(direction, reference)| {
            let position = anchor + delta.dot(direction) * direction;
            ((position - tip).length_sq(), ConstructionSnap {
                moving: [anchor.into(), position.into()], reference, triangle: None,
            })
        })
        .filter(|(distance, _)| *distance <= tolerance * tolerance)
        .min_by(|a, b| a.0.total_cmp(&b.0));

    // The two nodes are the ends of the hypotenuse. Choose the right-angle
    // vertex on the side towards the adjacent node's facing control handle.
    let normal = chord.rot90();
    let side = tangent.dot(normal).signum();
    let half_chord = (neighbor_pos - anchor) * 0.5;
    let corner = anchor + half_chord + normal * side * half_chord.length();
    let adjacent_leg = corner - neighbor_pos;
    let moving_leg = (corner - anchor).normalized();
    let length = delta.dot(moving_leg);
    let snapped = anchor + moving_leg * length;
    let distance = (snapped - tip).length();
    let line_error = adjacent_leg.dot(tangent.rot90()).abs();
    if adjacent_leg.dot(tangent) > 0.0 && length > 0.0
        && line_error <= (tolerance * 0.25).min(adjacent_leg.length() * 0.01)
        && distance <= tolerance
        && nearest.is_none_or(|(other, _)| distance <= other.sqrt() + tolerance * 0.1)
    {
        return Some(ConstructionSnap {
            moving: [anchor.into(), snapped.into()],
            reference: chord_reference,
            triangle: Some([anchor.into(), corner.into(), neighbor]),
        });
    }
    nearest.map(|(_, position)| position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_input::{tool_kind, CanvasInput, Drag, Mods, ToolKind};
    use dtool_core::document::Units;
    use dtool_core::Document;

    fn session() -> Session {
        let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
        session.has_document = true;
        session
    }

    fn last_curve(s: &Session) -> Contour {
        world_contours(s.document.objects.last().unwrap()).remove(0)
    }

    #[test]
    fn tool_kinds() {
        assert_eq!(tool_kind("tool.shape.line"), ToolKind::Line);
        assert_eq!(tool_kind("tool.pen"), ToolKind::Pen { polyline: false });
        assert_eq!(tool_kind("tool.pen.polyline"), ToolKind::Pen { polyline: true });
        assert_eq!(tool_kind("tool.node"), ToolKind::Node);
        assert_eq!(tool_kind("tool.pen.pencil"), ToolKind::Pencil);
    }

    #[test]
    fn line_drag_constrains_and_adds_curve() {
        let c = constrain_angle([0.0, 0.0], [10.0, 9.0]);
        assert!((c[0] - c[1]).abs() < 1e-4 && (c[0].hypot(c[1]) - 10.0_f32.hypot(9.0)).abs() < 1e-3);
        assert!(line_command([1.0, 1.0], [1.2, 1.0]).is_none());
        let mut s = session();
        let mut input = CanvasInput::new();
        input.drag = Some(Drag::Line { start: [10.0, 10.0], current: [110.0, 15.0] });
        let shift = Mods { shift: true, ctrl: false, alt: false };
        assert!(input.finish_drag(&mut s, [110.0, 15.0], shift), "{:?}", input.last_error);
        let contour = last_curve(&s);
        assert_eq!(contour.points.len(), 2);
        assert!((contour.points[1].pos[1] - 10.0).abs() < 1e-4, "{:?}", contour.points[1].pos);
        assert!(s.run(&Command::new("edit.undo")).is_ok());
        assert!(s.document.objects.is_empty());
    }

    #[test]
    fn pen_builds_smooth_closed_and_open_paths() {
        let mut s = session();
        let mut pen = Pen::default();
        assert_eq!(pen.press([0.0, 0.0], 3.0), PenStep::Added);
        pen.release();
        pen.press([100.0, 0.0], 3.0);
        pen.drag([150.0, 50.0]);
        pen.release();
        pen.press([100.0, 100.0], 3.0);
        pen.release();
        assert!(pen.preview(Some([50.0, 120.0])).is_some_and(|c| c.points.len() == 4));
        assert_eq!(pen.press([1.0, 1.0], 3.0), PenStep::Close);
        s.run(&pen.finish_command(true).unwrap()).unwrap();
        assert!(!pen.active());
        let contour = last_curve(&s);
        assert!(contour.closed);
        assert_eq!(contour.points.len(), 3);
        assert_eq!(contour.points[1].kind, NodeKind::Smooth);
        assert_eq!(contour.points[1].handle_out, Some([150.0, 50.0]));
        assert_eq!(contour.points[1].handle_in, Some([50.0, -50.0]));
        let mut line = Pen { polyline: true, ..Pen::default() };
        line.press([0.0, 0.0], 3.0);
        line.drag([10.0, 10.0]);
        line.press([50.0, 0.0], 3.0);
        line.press([50.5, 0.0], 3.0);
        line.drop_duplicate_tail(3.0);
        let command = line.finish_command(false).unwrap();
        s.run(&command).unwrap();
        let contour = last_curve(&s);
        assert!(!contour.closed && contour.points.len() == 2 && contour.points[0].handle_out.is_none());
        let mut short = Pen::default();
        short.press([0.0, 0.0], 3.0);
        assert!(short.finish_command(false).is_none());
    }

    #[test]
    fn node_drag_moves_selected_nodes_with_one_undo() {
        let mut s = session();
        s.run(&Command::with_params(CURVE_ADD, json!({ "points": [[10.0, 10.0], { "pos": [60.0, 10.0], "handle_in": [50.0, 10.0], "handle_out": [70.0, 10.0] }, [60.0, 60.0]] }))).unwrap();
        let id = s.document.objects.last().unwrap().id;
        let mut edit = NodeEdit::default();
        edit.sync(&s);
        assert_eq!(edit.target, Some(id));
        assert!(matches!(edit.press(&s, [200.0, 200.0], false, false, 4.0), NodePress::Miss));
        assert!(matches!(edit.press(&s, [60.0, 11.0], false, false, 4.0), NodePress::Grab));
        assert_eq!(edit.selected, vec![(0, 1)]);
        edit.release();
        edit.press(&s, [10.0, 10.0], true, false, 4.0);
        assert_eq!(edit.selected, vec![(0, 1), (0, 0)]);
        edit.drag([20.0, 15.0]);
        assert!(edit.live.is_some());
        let steps = s.undo.applied_steps();
        s.run(&edit.release().unwrap()).unwrap();
        assert_eq!(s.undo.applied_steps(), steps + 1);
        let contour = last_curve(&s);
        assert_eq!(contour.points[0].pos, [20.0, 15.0]);
        assert_eq!(contour.points[1].pos, [70.0, 15.0]);
        assert_eq!(contour.points[1].handle_out, Some([80.0, 15.0]));
        assert_eq!(contour.points[2].pos, [60.0, 60.0]);
        edit.sync(&s);
        assert!(matches!(edit.press(&s, [80.0, 15.0], false, false, 4.0), NodePress::Grab));
        edit.drag([80.0, 25.0]);
        s.run(&edit.release().unwrap()).unwrap();
        let contour = last_curve(&s);
        assert_eq!(contour.points[1].handle_out, Some([80.0, 25.0]));
        assert_eq!(contour.points[1].handle_in, Some([60.0, 5.0]));
        s.run(&Command::new("edit.undo")).unwrap();
        s.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(last_curve(&s).points[0].pos, [10.0, 10.0]);
        edit.press(&s, [10.0, 10.0], false, false, 4.0);
        assert!(edit.release().is_none());
    }

    #[test]
    fn node_insert_delete_convert_and_holes() {
        let mut s = session();
        s.run(&Command::with_params(CURVE_ADD, json!({ "points": [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], "closed": true }))).unwrap();
        let id = s.document.objects.last().unwrap().id;
        let outer = last_curve(&s);
        let hole = json!({ "points": [[20.0, 20.0], [40.0, 20.0], [40.0, 40.0], [20.0, 40.0]], "closed": true });
        s.run(&Command::with_params(CURVE_NODES_SET, json!({ "id": id.0, "contours": [outer, hole] }))).unwrap();
        let mut edit = NodeEdit::default();
        edit.sync(&s);
        assert!(edit.insert_at(&s, [0.0, 0.0], 4.0, 0.5).is_none());
        assert!(edit.insert_at(&s, [70.0, 70.0], 4.0, 0.5).is_none());
        let insert = edit.insert_at(&s, [30.0, 21.0], 4.0, 0.5).unwrap();
        assert_eq!(insert.params["contour"], json!(1));
        s.run(&insert).unwrap();
        let contours = world_contours(s.document.get_object(id).unwrap());
        assert_eq!(contours[1].points.len(), 5);
        assert_eq!(contours[1].points[1].pos, [30.0, 20.0]);
        assert!(matches!(edit.press(&s, [40.0, 40.0], false, false, 4.0), NodePress::Grab));
        edit.release();
        edit.press(&s, [100.0, 100.0], true, false, 4.0);
        edit.release();
        assert!(edit.claims_delete());
        let deletes = edit.delete_commands(&s);
        assert_eq!(deletes.len(), 2);
        assert_eq!(deletes[0].params["contour"], json!(1));
        for command in &deletes {
            s.run(command).unwrap();
        }
        assert!(!edit.claims_delete());
        let contours = world_contours(s.document.get_object(id).unwrap());
        assert_eq!((contours[0].points.len(), contours[1].points.len()), (3, 4));
        assert_eq!(s.document.objects.len(), 1);
        match edit.press(&s, [100.0, 0.0], false, true, 4.0) {
            NodePress::Run(command) => {
                assert_eq!(command.params["kind"], json!("smooth"));
                s.run(&command).unwrap();
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(last_curve(&s).points[1].kind, NodeKind::Smooth);
    }

    #[test]
    fn pen_alt_moves_only_out_handle() {
        let mut pen = Pen::default();
        pen.press([0.0, 0.0], 3.0);
        pen.drag([0.0, 20.0]);
        pen.release();
        pen.alt = true;
        assert_eq!(pen.press([1.0, 0.0], 3.0), PenStep::Handle);
        pen.drag([20.0, 0.0]);
        pen.release();
        assert_eq!(pen.nodes.len(), 1);
        let node = &pen.nodes[0];
        assert_eq!((node.handle_in, node.handle_out, node.kind), (Some([0.0, -20.0]), Some([20.0, 0.0]), NodeKind::Sharp));
        pen.alt = false;
        assert_eq!(pen.press([1.0, 0.0], 3.0), PenStep::Added);
    }

    #[test]
    fn pencil_simplifies_to_curve() {
        let mut s = session();
        let mut pencil = Pencil::default();
        pencil.press([0.0, 0.0]);
        for i in 1..=100 {
            let x = i as f32;
            pencil.drag([x, if i <= 50 { 0.0 } else { x - 50.0 }], 0.5);
        }
        assert!(pencil.preview().is_some());
        let command = pencil.finish_command(1.5, 3.0).unwrap();
        assert!(!pencil.active());
        let steps = s.undo.applied_steps();
        s.run(&command).unwrap();
        assert_eq!(s.undo.applied_steps(), steps + 1);
        let contour = last_curve(&s);
        assert_eq!(contour.points.len(), 3);
        assert!(!contour.closed);
        assert_eq!(contour.points[1].pos, [50.0, 0.0]);
        assert_eq!(contour.points[1].kind, NodeKind::Smooth);
        let mut ring = Pencil::default();
        ring.press([0.0, 0.0]);
        for p in [[50.0, 0.0], [50.0, 50.0], [0.0, 50.0], [0.0, 1.0]] {
            ring.drag(p, 0.5);
        }
        s.run(&ring.finish_command(1.5, 3.0).unwrap()).unwrap();
        let contour = last_curve(&s);
        assert!(contour.closed);
        assert_eq!(contour.points.len(), 4);
        let mut dot = Pencil::default();
        dot.press([0.0, 0.0]);
        dot.drag([1.0, 0.0], 0.5);
        assert!(dot.finish_command(1.5, 3.0).is_none());
    }

    #[test]
    fn node_break_and_join_commands() {
        let mut s = session();
        s.run(&Command::with_params(CURVE_ADD, json!({ "points": [[0.0, 0.0], [50.0, 0.0], [100.0, 0.0], [100.0, 50.0]] }))).unwrap();
        let mut edit = NodeEdit::default();
        edit.sync(&s);
        assert!(edit.break_command(&s).is_none());
        assert!(edit.select_at(&s, [0.0, 0.0], 4.0));
        assert!(edit.break_command(&s).is_none());
        assert!(!edit.select_at(&s, [70.0, 30.0], 4.0));
        assert!(edit.select_at(&s, [100.0, 0.0], 4.0));
        let command = edit.break_command(&s).unwrap();
        assert_eq!(command.name, "path.break");
        let before = s.document.objects.len();
        s.run(&command).unwrap();
        assert_eq!(s.document.objects.len(), before + 1);
        let curves: Vec<u64> = s.document.objects.iter().filter(|o| o.curve().is_some()).map(|o| o.id.0).collect();
        s.run(&Command::with_params("selection.set", json!({ "ids": curves, "op": "Replace" }))).unwrap();
        let join = join_command(&s).unwrap();
        assert_eq!(join.name, "path.join");
        s.run(&join).unwrap();
        assert_eq!(s.document.objects.len(), before);
        assert_eq!(last_curve(&s).points.len(), 4);
        s.run(&Command::with_params("selection.set", json!({ "ids": [s.document.objects.last().unwrap().id.0], "op": "Replace" }))).unwrap();
        assert!(join_command(&s).is_none());
    }
}
