use dtool_core::command::Session;
use dtool_core::commands::curve_edit::{map_contours, world_contours, CURVE_ADD, CURVE_NODES_SET};
use dtool_core::commands::curves::{preview_contours, SHAPE_CONVERT};
use dtool_core::commands::curve_nodes::{CURVE_NODES_CONVERT, CURVE_NODES_DELETE, CURVE_NODES_INSERT};
use dtool_core::commands::path_nodes::{PATH_BREAK, PATH_CLOSE, PATH_JOIN};
use dtool_core::curve::{Contour, NodeKind, PathPoint, PointId};
use dtool_core::geometry::{IDENTITY, invert_matrix, object_matrix};
use dtool_core::object::ObjectId;
use dtool_core::transform::Anchor;
use dtool_protocol::Command;
use serde_json::json;

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

    pub fn release(&mut self) {
        self.dragging = false;
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
        self.last = None;
        self.extension = None;
    }

    pub fn finish_command(&mut self, closed: bool) -> Option<Command> {
        let mut nodes = std::mem::take(&mut self.nodes);
        self.dragging = false;
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
}

pub const NODE_TRANSFORM_ANCHOR: Anchor = Anchor::TopLeft;

fn cubic(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2], t: f64) -> [f32; 2] {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    p32([w[0] * a[0] + w[1] * b[0] + w[2] * c[0] + w[3] * d[0], w[0] * a[1] + w[1] * b[1] + w[2] * c[1] + w[3] * d[1]])
}

pub fn segment_at(contours: &[Contour], point: [f32; 2], radius: f32) -> Option<(usize, usize)> {
    let mut best: Option<(f32, usize, usize)> = None;
    for (c, contour) in contours.iter().enumerate() {
        let n = contour.points.len();
        let count = if contour.closed { n } else { n.saturating_sub(1) };
        for i in 0..count {
            let (p, q) = (&contour.points[i], &contour.points[(i + 1) % n]);
            let (b, cc) = (p.handle_out.unwrap_or(p.pos), q.handle_in.unwrap_or(q.pos));
            let samples: Vec<[f32; 2]> = (0..=24).map(|k| cubic(p.pos, b, cc, q.pos, f64::from(k) / 24.0)).collect();
            let d = samples.windows(2).map(|w| segment_distance(point, w[0], w[1])).fold(f32::INFINITY, f32::min);
            if d <= radius && best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, c, i));
            }
        }
    }
    best.map(|(_, c, i)| (c, i))
}

pub fn selection_box(contours: &[Contour], selected: &[(usize, usize)]) -> Option<[f32; 4]> {
    let points: Vec<[f32; 2]> = selected.iter().filter_map(|&(c, i)| contours.get(c)?.points.get(i).map(|n| p32(n.pos))).collect();
    let first = points.first()?;
    let mut b = [first[0], first[1], first[0], first[1]];
    for p in &points {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    Some(b)
}

pub fn scale_handle(contours: &[Contour], selected: &[(usize, usize)]) -> Option<[f32; 2]> {
    if selected.len() < 2 {
        return None;
    }
    let b = selection_box(contours, selected)?;
    Some([b[2], b[3]])
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
    pub transform_mode: bool,
    display_to_edit: Option<[f32; 6]>,
    context: Option<dtool_core::workspace::ContextId>,
    point_order: Vec<Vec<PointId>>,
    point_order_generation: Option<u64>,
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
            let origin = [f64::from(b[0]), f64::from(b[1])];
            let span = [f64::from(b[2] - b[0]), f64::from(b[3] - b[1])];
            let s = [if span[0] > 1e-6 { (span[0] + d[0]) / span[0] } else { 1.0 }, if span[1] > 1e-6 { (span[1] + d[1]) / span[1] } else { 1.0 }];
            for &(c, i) in selected {
                if let Some(node) = out.get_mut(c).and_then(|contour| contour.points.get_mut(i)) {
                    node.pos = scale_about(node.pos, origin, s);
                    node.handle_in = node.handle_in.map(|h| scale_about(h, origin, s));
                    node.handle_out = node.handle_out.map(|h| scale_about(h, origin, s));
                }
            }
            return out;
        }
        _ => {}
    }
    match part {
        NodePart::Segment(..) | NodePart::Scale => {}
        NodePart::Anchor(..) => {
            for &(c, i) in selected {
                if let Some(node) = out.get_mut(c).and_then(|contour| contour.points.get_mut(i)) {
                    node.pos = shifted(node.pos, d);
                    node.handle_in = node.handle_in.map(|h| shifted(h, d));
                    node.handle_out = node.handle_out.map(|h| shifted(h, d));
                }
            }
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
    session.active_tool == "tool.node" && match session.selection.ids.as_slice() {
        [id] => session.document.get_object(*id).is_some_and(|object| !object.locked && object.curve().is_some()),
        ids => ids.len() > 1 && join_command(session).is_some(),
    }
}

pub fn insert_command(id: ObjectId, contour: usize, point: [f32; 2]) -> Command {
    Command::with_params(CURVE_NODES_INSERT, json!({ "id": id.0, "contour": contour, "point": point }))
}

impl NodeEdit {
    pub fn sync(&mut self, session: &Session) {
        let editable = match session.selection.ids.as_slice() {
            [id] => node_contours(session, *id).map(|(contours, primitive)| (*id, contours, primitive)),
            _ => None,
        };
        let single = editable.as_ref().map(|(id, _, _)| *id);
        if single != self.target || self.context != session.context {
            *self = NodeEdit { target: single, context: session.context, alt: self.alt, shift: self.shift,
                transform_mode: self.transform_mode, ..NodeEdit::default() };
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
        let single = match session.selection.ids.as_slice() { [id] => Some(*id), _ => None };
        if self.target != single || self.context != session.context || self.point_order_generation != Some(session.generation) {
            self.sync(session);
        }
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

    pub fn select_at(&mut self, session: &Session, point: [f32; 2], radius: f32) -> bool {
        self.sync(session);
        let Some((contours, _)) = self.target.and_then(|id| node_contours(session, id)) else { return false };
        match hit_part(&contours, &[], point, radius) {
            Some(NodePart::Anchor(c, i)) => {
                if !self.selected.contains(&(c, i)) {
                    self.selected = vec![(c, i)];
                }
                true
            }
            _ => false,
        }
    }

    pub fn break_command(&self, session: &Session) -> Option<Command> {
        let id = self.target?;
        let [(0, i)] = self.selected.as_slice() else { return None };
        let contours = editable(session, id)?;
        let contour = contours.first()?;
        let node = contour.points.get(*i)?;
        if !contour.closed && (*i == 0 || *i + 1 == contour.points.len()) {
            return None;
        }
        Some(self.preserve_edit_transform(Command::with_params(PATH_BREAK, json!({ "id": id.0, "point": { "id": node.id.0 } }))))
    }

    pub(crate) fn reverse_command(&self, session: &Session) -> Option<Command> {
        let command = self.preserve_edit_transform(Command::with_params(
            dtool_core::commands::path_points::REVERSE,
            json!({"id": self.target?.0, "subpath": self.selected.first().map_or(0, |(index, _)| *index)}),
        ));
        session.can_execute(&command).then_some(command)
    }

    pub fn claims_delete(&self) -> bool {
        self.target.is_some() && !self.primitive && !self.selected.is_empty()
    }

    pub fn press(&mut self, session: &Session, point: [f32; 2], shift: bool, alt: bool, radius: f32) -> NodePress {
        self.sync(session);
        let (Some(id), Some((contours, _))) = (self.target, self.target.and_then(|id| node_contours(session, id))) else { return NodePress::Miss };
        if self.transform_mode && scale_handle(&contours, &self.selected).is_some_and(|h| distance(h, point) <= radius) {
            self.grab = Some((NodePart::Scale, point));
            self.base = Some(contours);
            self.live = None;
            return NodePress::Grab;
        }
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
                        self.selected = vec![(c, i)];
                    }
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
                if !shift {
                    self.selected.clear();
                }
                NodePress::Miss
            }
        }
    }

    pub fn select_all(&mut self, session: &Session) -> bool {
        self.sync(session);
        let Some((contours, _)) = self.target.and_then(|id| node_contours(session, id)) else { return false };
        self.selected = contours.iter().enumerate().flat_map(|(c, contour)| (0..contour.points.len()).map(move |i| (c, i))).collect();
        true
    }

    pub fn close_command(&self, session: &Session) -> Option<Command> {
        let id = self.target?;
        let contours = editable(session, id)?;
        let c = self.selected.first().map_or(0, |(c, _)| *c);
        let contour = contours.get(c)?;
        (!contour.closed && contour.points.len() >= 3).then(|| {
            self.preserve_edit_transform(Command::with_params(PATH_CLOSE, json!({ "target": format!("node:{}", id.0), "subpath": c })))
        })
    }

    pub fn moving_points(&self, point: [f32; 2]) -> Option<Vec<[f32; 2]>> {
        let (Some((NodePart::Anchor(..), start)), Some(base)) = (self.grab, self.base.as_ref()) else { return None };
        let d = [point[0] - start[0], point[1] - start[1]];
        let points: Vec<[f32; 2]> = self.selected.iter().filter_map(|&(c, i)| base.get(c)?.points.get(i).map(|n| p32(n.pos))).map(|p| [p[0] + d[0], p[1] + d[1]]).collect();
        (!points.is_empty()).then_some(points)
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

    pub(crate) fn convert_selected(&self, session: &Session, kind: NodeKind) -> Option<Command> {
        let id = self.target?;
        let contours = editable(session, id)?;
        let mut groups = std::collections::BTreeMap::<usize, Vec<u64>>::new();
        for &(c, i) in &self.selected {
            let node = contours.get(c)?.points.get(i)?;
            if node.kind != kind {
                groups.entry(c).or_default().push(node.id.0);
            }
        }
        let kind = match kind { NodeKind::Sharp => "sharp", NodeKind::Smooth => "smooth", NodeKind::Smart => "smart" };
        let mut commands: Vec<_> = groups.into_iter().map(|(contour, nodes)| {
            self.preserve_edit_transform(Command::with_params(CURVE_NODES_CONVERT,
                json!({ "id": id.0, "contour": contour, "nodes": nodes, "kind": kind })))
        }).collect();
        match commands.len() {
            0 => None,
            1 => commands.pop(),
            _ => Some(Command::with_params("edit.batch", json!({ "commands": commands }))),
        }
    }

    pub fn delete_commands(&mut self, session: &Session) -> Vec<Command> {
        let (Some(id), Some(contours)) = (self.target, self.target.and_then(|id| editable(session, id))) else { return Vec::new() };
        let mut by_contour: Vec<(usize, Vec<u64>)> = Vec::new();
        for &(c, i) in &self.selected {
            let Some(node) = contours.get(c).and_then(|contour| contour.points.get(i)) else { continue };
            match by_contour.iter_mut().find(|(k, _)| *k == c) {
                Some((_, ids)) => ids.push(node.id.0),
                None => by_contour.push((c, vec![node.id.0])),
            }
        }
        by_contour.sort_by(|a, b| b.0.cmp(&a.0));
        self.selected.clear();
        by_contour.into_iter().map(|(c, nodes)| {
            self.preserve_edit_transform(Command::with_params(CURVE_NODES_DELETE, json!({ "id": id.0, "contour": c, "nodes": nodes })))
        }).collect()
    }

    pub fn drag(&mut self, point: [f32; 2]) {
        let (Some((part, start)), Some(base)) = (self.grab, self.base.as_ref()) else { return };
        let delta = [point[0] - start[0], point[1] - start[1]];
        self.live = (delta != [0.0, 0.0]).then(|| drag_part(base, part, &self.selected, delta, self.shift, self.alt));
    }

    pub fn release(&mut self) -> Option<Command> {
        self.grab = None;
        self.base = None;
        let mut contours = self.live.take()?;
        self.unproject_contours(&mut contours);
        let id = self.target?;
        let name = if self.primitive { SHAPE_CONVERT } else { CURVE_NODES_SET };
        Some(self.preserve_edit_transform(Command::with_params(name, json!({ "id": id.0, "contours": contours }))))
    }

    pub fn cancel(&mut self) {
        self.grab = None;
        self.base = None;
        self.live = None;
    }
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
