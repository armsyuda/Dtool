use dtool_core::command::Session;
use dtool_core::curve::Curve;
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::{Command, CommandResult};
use dtool_text::frame::VAlign;

pub const TOOL_IDS: &[&str] = &["tool.text.frame", "tool.frame_text"];
pub const STATUS_CONVERT: &str = "tool.frame_text.convert_hint";
pub const STATUS_LINK: &str = "tool.frame_text.link_hint";

#[derive(Debug, Clone, Copy, PartialEq)]
enum DragState {
    Idle,
    Pending { start: [f32; 2] },
    Dragging { start: [f32; 2], current: [f32; 2] },
}

impl Default for DragState {
    fn default() -> Self {
        DragState::Idle
    }
}

#[derive(Debug, Clone, Default)]
pub struct FrameTextTool {
    drag: DragState,
    pub status: String,
    pub pending_edit: Option<(u64, u64)>,
    link_from: Option<ObjectId>,
    marks: Option<((usize, u64, u64), FlowMarks)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlowMarks {
    pub overset: Vec<ObjectId>,
    pub links: Vec<(ObjectId, ObjectId)>,
}

impl FrameTextTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(tool: &str) -> bool {
        TOOL_IDS.iter().any(|id| *id == tool)
    }

    pub fn begin_pointer(&mut self, doc_point: [f32; 2]) {
        self.pending_edit = None;
        self.drag = DragState::Pending { start: doc_point };
        if self.link_from.is_none() {
            self.status.clear();
        }
    }

    pub fn move_pointer(&mut self, doc_point: [f32; 2]) {
        match self.drag {
            DragState::Pending { start } => {
                if (doc_point[0] - start[0]).abs() > 2.0 || (doc_point[1] - start[1]).abs() > 2.0 {
                    self.drag = DragState::Dragging {
                        start,
                        current: doc_point,
                    };
                }
            }
            DragState::Dragging { start, .. } => {
                self.drag = DragState::Dragging {
                    start,
                    current: doc_point,
                };
            }
            DragState::Idle => {}
        }
    }

    pub fn hover(&mut self, session: &Session, doc_point: [f32; 2]) {
        if matches!(self.drag, DragState::Dragging { .. }) {
            return;
        }
        if self.link_from.is_some() {
            self.status = STATUS_LINK.to_string();
        } else if closed_shape_at(&session.document, doc_point).is_some() {
            self.status = STATUS_CONVERT.to_string();
        } else {
            self.status.clear();
        }
    }

    pub fn get_status_text(&self) -> &str {
        if self.status == STATUS_CONVERT {
            "Click to convert Curve to Text Frame"
        } else if self.status == STATUS_LINK {
            "Click to flow text into this frame. Ctrl to continue linking."
        } else {
            &self.status
        }
    }

    pub fn cancel(&mut self) {
        self.drag = DragState::Idle;
        self.status.clear();
        self.pending_edit = None;
        self.link_from = None;
    }

    pub fn flow_marks(&mut self, session: &Session) -> &FlowMarks {
        let library = dtool_text::FontLibrary::shared();
        let key = (std::ptr::from_ref(&session.document) as usize, session.generation, dtool_text::fonts::font_generation());
        if self.marks.as_ref().is_some_and(|(cached, _)| *cached != key) {
            self.marks = None;
        }
        let (_, marks) = self.marks.get_or_insert_with(|| {
            (
                key,
                FlowMarks {
                    overset: dtool_text::frame::overset_frames(&library, &session.document),
                    links: dtool_core::frame::flow_links(&session.document),
                },
            )
        });
        marks
    }

    pub fn flow_marks_key(&self) -> Option<(usize, u64, u64)> {
        self.marks.as_ref().map(|(key, _)| *key)
    }

    pub fn linking(&self) -> Option<ObjectId> {
        self.link_from
    }

    pub fn begin_link(&mut self, from: ObjectId) {
        self.drag = DragState::Idle;
        self.pending_edit = None;
        self.link_from = Some(from);
        self.status = STATUS_LINK.to_string();
    }

    pub fn cancel_link(&mut self) {
        self.link_from = None;
        self.drag = DragState::Idle;
        self.status.clear();
    }

    pub fn out_port_at(&self, doc: &dtool_core::Document, point: [f32; 2], tolerance: f32) -> Option<ObjectId> {
        doc.objects.iter().rev().find(|object| {
            !object.locked
                && object.pickable
                && matches!(object.kind, ObjectKind::FrameText { .. })
                && !has_next_frame(doc, object.id)
                && {
                    let port = flow_frame_quad(doc, object)[2];
                    (point[0] - port[0]).abs() <= tolerance && (point[1] - port[1]).abs() <= tolerance
                }
        }).map(|object| object.id)
    }

    pub fn link_target(&self, doc: &dtool_core::Document, point: [f32; 2]) -> Option<ObjectId> {
        let from = self.link_from?;
        let source = dtool_core::frame::frame_story(doc, from)?;
        doc.objects.iter().rev().find(|object| {
            !object.locked
                && object.pickable
                && object.id != from
                && matches!(object.kind, ObjectKind::FrameText { story } if story != source)
                && dtool_core::frame::chain_of(doc, object.id).is_none()
                && point_in_frame(doc, object, point)
        }).map(|object| object.id)
    }

    pub fn finish_link(&mut self, session: &mut Session, doc_point: [f32; 2], keep_linking: bool) -> CommandResult {
        let Some(from) = self.link_from else {
            return Ok(Default::default());
        };
        let start = match self.drag {
            DragState::Pending { start } | DragState::Dragging { start, .. } => start,
            DragState::Idle => doc_point,
        };
        self.drag = DragState::Idle;
        let dragged = (doc_point[0] - start[0]).abs() > 2.0 || (doc_point[1] - start[1]).abs() > 2.0;
        let target = if dragged {
            let mut cmd = Command::new("text.frame.add");
            cmd.params["rect"] = vec![
                f64::from(start[0].min(doc_point[0])),
                f64::from(start[1].min(doc_point[1])),
                f64::from((doc_point[0] - start[0]).abs().max(1.0)),
                f64::from((doc_point[1] - start[1]).abs().max(1.0)),
            ].into();
            session.run(&cmd)?.get("id").and_then(|v| v.as_u64()).map(ObjectId)
        } else {
            self.link_target(&session.document, doc_point)
        };
        let Some(to) = target else {
            self.cancel_link();
            return Ok(Default::default());
        };
        let result = link_frames(session, from, to);
        if result.is_ok() && keep_linking {
            self.begin_link(to);
        } else {
            self.cancel_link();
        }
        result
    }

    pub fn finish(&mut self, session: &mut Session, doc_point: [f32; 2], double_click: bool) -> CommandResult {
        let start = match self.drag {
            DragState::Pending { start } | DragState::Dragging { start, .. } => start,
            DragState::Idle => doc_point,
        };
        self.drag = DragState::Idle;
        if double_click {
            if let Some(object) = frame_at(&session.document, doc_point) {
                if let Some(story) = object.story_id() {
                    let caret = caret_for_object(&session.document, object, doc_point);
                    let mut cmd = Command::new("text.select");
                    cmd.params["story"]=story.0.into();cmd.params["id"]=object.id.0.into();cmd.params["range"]["start"]=(caret as u64).into();cmd.params["range"]["end"]=(caret as u64).into();
                    self.pending_edit = Some((object.id.0, story.0));
                    return session.run(&cmd);
                }
            }
        }
        let dragged = (doc_point[0] - start[0]).abs() > 2.0 || (doc_point[1] - start[1]).abs() > 2.0;
        if dragged {
            let x0 = start[0].min(doc_point[0]);
            let y0 = start[1].min(doc_point[1]);
            let w = (doc_point[0] - start[0]).abs().max(1.0);
            let h = (doc_point[1] - start[1]).abs().max(1.0);
            let mut cmd = Command::new("text.frame.add");
            cmd.params["rect"]=vec![f64::from(x0),f64::from(y0),f64::from(w),f64::from(h)].into();
            let result = session.run(&cmd)?;
            if let Some(id) = result.get("id").and_then(|v| v.as_u64()) {
                if let Some(story) = result.get("story").and_then(|v| v.as_u64()) {
                    self.pending_edit = Some((id, story));
                    let mut select = Command::new("text.select");
                    select.params["story"]=story.into();select.params["id"]=id.into();select.params["range"]["start"]=0_u64.into();select.params["range"]["end"]=0_u64.into();
                    let _ = session.run(&select);
                }
            }
            return Ok(result);
        }
        if let Some(object) = closed_shape_at(&session.document, doc_point) {
            let mut cmd = Command::new("text.shape_text.add");
            cmd.params["shape"]=object.id.0.into();
            let result = session.run(&cmd)?;
            if let Some(id) = result.get("id").and_then(|v| v.as_u64()) {
                if let Some(story) = result.get("story").and_then(|v| v.as_u64()) {
                    self.pending_edit = Some((id, story));
                    let mut select = Command::new("text.select");
                    select.params["story"]=story.into();select.params["id"]=id.into();select.params["range"]["start"]=0_u64.into();select.params["range"]["end"]=0_u64.into();
                    let _ = session.run(&select);
                }
            }
            self.status.clear();
            return Ok(result);
        }
        if let Some(object) = frame_at(&session.document, doc_point) {
            if let Some(story) = object.story_id() {
                let caret = caret_for_object(&session.document, object, doc_point);
                let mut cmd = Command::new("text.select");
                cmd.params["story"]=story.0.into();cmd.params["id"]=object.id.0.into();cmd.params["range"]["start"]=(caret as u64).into();cmd.params["range"]["end"]=(caret as u64).into();
                self.pending_edit = Some((object.id.0, story.0));
                return session.run(&cmd);
            }
        }
        Ok(Default::default())
    }

    pub fn set_valign(session: &mut Session, id: ObjectId, valign: VAlign) -> CommandResult {
        let mut cmd = Command::new("text.frame.set");
        cmd.params["id"] = id.0.into();
        cmd.params["valign"] = match valign {
            VAlign::Top => "top",
            VAlign::Center => "center",
            VAlign::Bottom => "bottom",
            VAlign::Justify => "justify",
        }.into();
        session.run(&cmd)
    }

    pub fn set_font_size(session: &mut Session, id: ObjectId, size_pt: f32) -> CommandResult {
        let mut cmd = Command::new("text.frame.set");
        cmd.params["id"] = id.0.into();
        cmd.params["size_pt"] = f64::from(size_pt).into();
        session.run(&cmd)
    }

    pub fn set_font_style(session: &mut Session, id: ObjectId, style: &str) -> CommandResult {
        let mut cmd = Command::new("text.frame.set");
        cmd.params["id"] = id.0.into();
        cmd.params["font_style"] = style.into();
        session.run(&cmd)
    }

    pub fn set_para_leading(session: &mut Session, id: ObjectId, leading: f32) -> CommandResult {
        let mut cmd = Command::new("text.frame.set");
        cmd.params["id"] = id.0.into();
        cmd.params["para_leading"] = f64::from(leading).into();
        session.run(&cmd)
    }
}

pub fn install_filler() -> bool {
    dtool_text::filler::install()
}

pub fn install_flow_ranges() -> bool {
    dtool_text::frame::install_flow_ranges()
}

pub fn install_canvas_text() -> bool {
    let canvas = dtool_text::canvas_text::install();
    let faces = dtool_text::text_faces::install();
    dtool_text::text_host::install() && canvas && faces
}

pub fn link_frames(session: &mut Session, from: ObjectId, to: ObjectId) -> CommandResult {
    let mut cmd = Command::new("text.link");
    cmd.params["from"] = from.0.into();
    cmd.params["to"] = to.0.into();
    session.run(&cmd)
}

pub fn unlink_frame(session: &mut Session, id: ObjectId) -> CommandResult {
    let mut cmd = Command::new("text.unlink");
    cmd.params["id"] = id.0.into();
    session.run(&cmd)
}

pub fn flow_frame_quad(doc: &dtool_core::Document, object: &Object) -> [[f32; 2]; 4] {
    let mut quad = dtool_core::geometry::object_world_quad(object);
    if let Some(points) = dtool_core::commands::warp::live_warp_points(doc, object.id, &quad) {
        quad.copy_from_slice(&points);
    }
    quad
}

fn has_next_frame(doc: &dtool_core::Document, id: ObjectId) -> bool {
    dtool_core::frame::chain_of(doc, id).is_some_and(|chain| chain.frames.last() != Some(&id))
}

fn point_in_rect(pos: [f32; 2], size: [f32; 2], point: [f32; 2]) -> bool {
    point[0] >= pos[0]
        && point[1] >= pos[1]
        && point[0] <= pos[0] + size[0]
        && point[1] <= pos[1] + size[1]
}

fn point_in_frame(doc: &dtool_core::Document, object: &Object, point: [f32; 2]) -> bool {
    let matrix = dtool_core::geometry::object_matrix(object);
    let Some(inverse) = dtool_core::geometry::invert_matrix(&matrix) else {
        return false;
    };
    let world = dtool_core::commands::warp::unwarp_point(doc, object.id, point);
    let position = dtool_core::geometry::apply_matrix(&inverse, world);
    point_in_rect(object.pos, object.size.map(f32::abs), position)
}

fn frame_at(doc: &dtool_core::Document, point: [f32; 2]) -> Option<&Object> {
    doc.objects.iter().rev().find(|object| {
        !object.locked && object.pickable && matches!(object.kind, ObjectKind::FrameText { .. })
            && point_in_frame(doc, object, point)
    })
}

fn closed_curve(curve: &Curve) -> bool {
    curve.contour.closed && curve.contour.points.len() >= 2
}

pub fn is_closed_shape(object: &Object) -> bool {
    match &object.kind {
        ObjectKind::Shape => object.shape.as_ref().is_none_or(|recipe|matches!(recipe.kind,dtool_core::shape::ShapeKind::Rectangle|dtool_core::shape::ShapeKind::Ellipse)),
        ObjectKind::Curve(curve) => closed_curve(curve) && curve.holes.is_empty(),
        _ => false,
    }
}

fn closed_shape_at(doc: &dtool_core::Document, point: [f32; 2]) -> Option<&Object> {
    doc.objects.iter().rev().find(|object| {
        !object.locked && object.pickable && is_closed_shape(object) && point_in_rect(object.pos, object.size, point)
    })
}

pub fn hit_frame_id(doc: &dtool_core::Document, point: [f32; 2]) -> Option<ObjectId> {
    frame_at(doc, point).map(|object| object.id)
}

impl FrameTextTool {
    pub fn drag_corners(&self) -> Option<([f32; 2], [f32; 2])> {
        match self.drag {
            DragState::Dragging { start, current } => Some((start, current)),
            _ => None,
        }
    }
}

fn size_pt_of_story(story: &dtool_core::Story) -> f32 {
    for range in &story.ranges {
        if let Some(size) = range.get("size_pt").and_then(|v| v.as_f64()) {
            if size.is_finite() && size > 0.0 {
                return size as f32;
            }
        }
    }
    12.0
}

pub fn estimate_caret(story: &dtool_core::Story, wrap_w: f32, local: [f32; 2]) -> usize {
    let size_pt = size_pt_of_story(story);
    let line_h = if size_pt > 0.0 { size_pt * 1.2 } else { 14.4 };
    let advance = if size_pt > 0.0 { size_pt * 0.5 } else { 6.0 };
    let width = if wrap_w.is_finite() && wrap_w > 0.0 { wrap_w } else { f32::MAX };
    let chars: Vec<char> = story.text.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    let mut lines: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    let mut x = 0.0f32;
    for (i, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            lines.push((start, i));
            start = i + 1;
            x = 0.0;
            continue;
        }
        let next = x + advance;
        if next > width && i > start {
            lines.push((start, i));
            start = i;
            x = advance;
        } else {
            x = next;
        }
    }
    lines.push((start, chars.len()));
    let mut line_index = 0usize;
    let mut best = f32::MAX;
    for i in 0..lines.len() {
        let baseline = (i as f32 + 0.8) * line_h;
        let d = (baseline - local[1]).abs();
        if d < best {
            best = d;
            line_index = i;
        }
    }
    let (a, b) = lines[line_index];
    if a == b {
        return a;
    }
    let col = if advance > 0.0 { (local[0] / advance).round() as i32 } else { 0 };
    let col = col.max(0) as usize;
    (a + col).min(b)
}

pub fn caret_in_frame(doc: &dtool_core::Document, id: ObjectId, point: [f32; 2]) -> Option<usize> {
    let object = doc.get_object(id)?;
    Some(caret_for_object(doc, object, point))
}

fn caret_for_object(doc: &dtool_core::Document, object: &Object, doc_point: [f32; 2]) -> usize {
    let doc_point = dtool_core::commands::warp::unwarp_point(doc, object.id, doc_point);
    let Some(story_id) = object.story_id() else { return 0; };
    let Some(story) = doc.stories.iter().find(|s| s.id == story_id) else { return 0; };
    let wrap = object
        .frame_props()
        .map(|props| props.wrap_width())
        .unwrap_or(object.size[0]);
    let Some(inverse) = dtool_core::geometry::invert_matrix(&dtool_core::geometry::text_matrix(object)) else { return 0 };
    let [x, y] = dtool_core::geometry::apply_matrix(&inverse, doc_point);
    let local = [x - object.pos[0], y - object.pos[1]];
    let library = dtool_text::FontLibrary::shared();
    if let Some(flow) = dtool_text::frame::layout_doc_flow(&library, doc, story_id) {
        if let Some(lines) = flow.frames.iter().position(|id| *id == object.id).and_then(|index| flow.layout.per_frame.get(index)) {
            if !lines.is_empty() {
                return dtool_text::frame::caret_at_point(lines, local);
            }
            if let Some(index) = flow.frames.iter().position(|id| *id == object.id).filter(|index| *index > 0) {
                return flow.layout.per_frame[..index].iter().rev().find_map(|prev| prev.last()).map_or(0, |line| line.range.end);
            }
        }
    }
    let scale = dtool_core::units::units_per_point(doc.units, doc.dpi);
    estimate_caret(story, wrap / scale, [local[0] / scale, local[1] / scale])
}
