use dtool_core::command::{SelectionNudgeArgs, TransformSelectionBoxArgs, EDIT_DUPLICATE_DRAG, NUDGE_SHIFT_MULTIPLIER, NUDGE_STEP_PX};
use dtool_core::scene_hit::SelectionFrame;
use dtool_core::selection::{MarqueeMode, SelectOp};
use dtool_core::transform::Anchor;

use crate::{view_to_doc, ViewNav};

pub const POSITION_FIELD: &str = "Object.pos";
pub const ALT_DRAG_DUPLICATE_COMMAND: &str = EDIT_DUPLICATE_DRAG;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    FrameTextScale,
    Rotate,
    ShearTop,
    ShearBottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureKind {
    Marquee,
    Move,
    Scale(Handle),
    Rotate,
    Shear(Handle),
}

#[derive(Debug, Clone)]
pub struct MoveTool {
    pub active: bool,
    pub dragging: bool,
    pub marquee_start: Option<[f32; 2]>,
    pub marquee_current: Option<[f32; 2]>,
    pub modifier_shift: bool,
    pub modifier_ctrl: bool,
    pub modifier_alt: bool,
    pub gesture: Option<GestureKind>,
    pub selection_box: Option<[f32; 4]>,
    pub selection_frame: SelectionFrame,
    pub anchor: Option<Anchor>,
    pub transform_origin: Option<[f32; 2]>,
    pub start_point: Option<[f32; 2]>,
    pub last_point: Option<[f32; 2]>,
    pub handle_radius: f32,
    pub rotate_offset: f32,
    pub duplicate_on_drag: bool,
    pub proportional_text_corners: bool,
    pub frame_text_scale_handle: bool,
    pub show_handles: bool,
    pub transform_separately: bool,
}

impl Default for MoveTool {
    fn default() -> Self {
        MoveTool {
            active: false,
            dragging: false,
            marquee_start: None,
            marquee_current: None,
            modifier_shift: false,
            modifier_ctrl: false,
            modifier_alt: false,
            gesture: None,
            selection_box: None,
            selection_frame: SelectionFrame::default(),
            anchor: None,
            transform_origin: None,
            start_point: None,
            last_point: None,
            handle_radius: 6.0,
            rotate_offset: 24.0,
            duplicate_on_drag: false,
            proportional_text_corners: false,
            frame_text_scale_handle: false,
            show_handles: true,
            transform_separately: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PointerEvent {
    pub x: f32,
    pub y: f32,
    pub pressed: bool,
    pub released: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Debug, Clone)]
pub enum MoveToolAction {
    HitSelect { point: [f32; 2], cycle: bool },
    MarqueeStart { x: f32, y: f32 },
    MarqueeUpdate { rect: [f32; 4] },
    MarqueeCommit { rect: [f32; 4], mode: MarqueeMode, op: SelectOp },
    MarqueeCancel,
    MoveStart { dx: f32, dy: f32 },
    MoveUpdate { dx: f32, dy: f32 },
    MoveCommit,
    GestureStart { kind: GestureKind },
    TransformPreview { args: TransformSelectionBoxArgs },
    TransformCommit { args: TransformSelectionBoxArgs },
    TransformCancel,
    EnterGroup { point: [f32; 2] },
    ExitGroup,
    DuplicatePreview { offset: [f32; 2] },
    DuplicateCommit { offset: [f32; 2] },
    None,
}

impl MoveTool {
    pub fn set_selection_box(&mut self, selection_box: Option<[f32; 4]>) {
        self.set_selection_frame(selection_box.map(|bounds| SelectionFrame { bounds, ..SelectionFrame::default() }));
    }

    pub fn set_selection_frame(&mut self, frame: Option<SelectionFrame>) {
        if !self.dragging {
            self.selection_box = frame.map(|frame| frame.bounds);
            self.selection_frame = frame.unwrap_or_default();
        }
    }

    pub fn set_anchor(&mut self, anchor: Option<Anchor>) {
        self.anchor = anchor;
    }

    pub fn on_pointer(&mut self, event: PointerEvent) -> MoveToolAction {
        self.modifier_shift = event.shift;
        self.modifier_ctrl = event.ctrl;
        self.modifier_alt = event.alt;
        let point = [event.x, event.y];
        if self.dragging && event.alt && self.gesture == Some(GestureKind::Move) {
            self.duplicate_on_drag = true;
        }

        if event.pressed && !self.dragging {
            self.dragging = true;
            self.start_point = Some(point);
            self.last_point = Some(point);

            if let Some(bounds) = self.selection_box {
                if let Some(handle) = self.hit_handle(bounds, point) {
                    let kind = match handle {
                        Handle::Rotate => GestureKind::Rotate,
                        Handle::ShearTop | Handle::ShearBottom => GestureKind::Shear(handle),
                        other => GestureKind::Scale(other),
                    };
                    self.gesture = Some(kind);
                    return MoveToolAction::GestureStart { kind };
                }
                if contains(bounds, self.selection_frame.to_local(point)) {
                    self.gesture = Some(GestureKind::Move);
                    self.duplicate_on_drag = event.alt;
                    return MoveToolAction::GestureStart { kind: GestureKind::Move };
                }
            }

            self.gesture = Some(GestureKind::Marquee);
            self.marquee_start = Some(point);
            self.marquee_current = Some(point);
            return MoveToolAction::MarqueeStart { x: event.x, y: event.y };
        }

        if self.dragging && !event.released {
            self.last_point = Some(point);
            return self.preview();
        }

        if self.dragging && event.released {
            self.last_point = Some(point);
            let kind = self.gesture.unwrap_or(GestureKind::Marquee);

            if kind == GestureKind::Marquee {
                let mode = self.resolve_marquee_mode();
                let op = self.resolve_select_op();
                let start = self.marquee_start.unwrap_or(point);
                let rect = normalize_rect(start[0], start[1], event.x, event.y);
                self.reset();

                if rect[2] <= 0.0 && rect[3] <= 0.0 {
                    return MoveToolAction::HitSelect { point, cycle: event.alt };
                }

                return MoveToolAction::MarqueeCommit { rect, mode, op };
            }

            let moved = match self.start_point {
                Some(start) => start[0] != point[0] || start[1] != point[1],
                None => false,
            };
            let duplicate = kind == GestureKind::Move && (self.duplicate_on_drag || event.alt);
            let args = self.current_args();
            self.reset();

            if duplicate && moved {
                return match args {
                    Some(args) if args.translate[0] != 0.0 || args.translate[1] != 0.0 => {
                        MoveToolAction::DuplicateCommit { offset: args.translate }
                    }
                    _ => MoveToolAction::TransformCancel,
                };
            }

            if !moved {
                if kind == GestureKind::Move {
                    return MoveToolAction::HitSelect { point, cycle: event.alt };
                }
                return MoveToolAction::TransformCancel;
            }

            return match args {
                Some(args) if !is_identity(&args) => MoveToolAction::TransformCommit { args },
                _ => MoveToolAction::TransformCancel,
            };
        }

        MoveToolAction::None
    }

    pub fn on_modifiers(&mut self, shift: bool, ctrl: bool, alt: bool) -> MoveToolAction {
        let changed = self.modifier_shift != shift || self.modifier_ctrl != ctrl || self.modifier_alt != alt;
        self.modifier_shift = shift;
        self.modifier_ctrl = ctrl;
        self.modifier_alt = alt;
        if self.dragging && alt && self.gesture == Some(GestureKind::Move) {
            self.duplicate_on_drag = true;
        }
        if !self.dragging || !changed {
            return MoveToolAction::None;
        }
        self.preview()
    }

    pub fn escape(&mut self) -> MoveToolAction {
        if !self.dragging {
            return MoveToolAction::None;
        }
        let kind = self.gesture.unwrap_or(GestureKind::Marquee);
        self.reset();
        if kind == GestureKind::Marquee {
            MoveToolAction::MarqueeCancel
        } else {
            MoveToolAction::TransformCancel
        }
    }

    pub fn on_pointer_view(&mut self, nav: &ViewNav, event: PointerEvent) -> MoveToolAction {
        let point = view_to_doc(nav, [event.x, event.y]);
        self.on_pointer(PointerEvent { x: point[0], y: point[1], ..event })
    }

    pub fn on_double_click(&mut self, nav: &ViewNav, view_point: [f32; 2]) -> MoveToolAction {
        self.reset();
        MoveToolAction::EnterGroup { point: view_to_doc(nav, view_point) }
    }

    pub fn exit_group(&mut self) -> MoveToolAction {
        self.reset();
        MoveToolAction::ExitGroup
    }

    pub fn on_tab(&mut self) -> MoveToolAction {
        MoveToolAction::None
    }

    pub fn on_focus_lost(&mut self) -> MoveToolAction {
        self.escape()
    }

    pub fn cancel(&mut self) {
        self.reset();
    }

    pub fn current_args(&self) -> Option<TransformSelectionBoxArgs> {
        let bounds = self.selection_box?;
        let start = self.start_point?;
        let point = self.last_point?;
        let kind = self.gesture?;
        let centre = [bounds[0] + bounds[2] * 0.5, bounds[1] + bounds[3] * 0.5];

        let mut args = TransformSelectionBoxArgs { origin: centre, ..Default::default() };

        match kind {
            GestureKind::Marquee => return None,
            GestureKind::Move => {
                let mut delta = [point[0] - start[0], point[1] - start[1]];
                if self.modifier_shift {
                    delta = constrain_delta(delta);
                }
                args.translate = delta;
            }
            GestureKind::Scale(handle) => {
                let start = self.selection_frame.to_local(start);
                let point = self.selection_frame.to_local(point);
                let frac = handle_frac(handle);
                let origin = if let Some(origin) = self.transform_origin {
                    self.selection_frame.to_local(origin)
                } else if self.modifier_alt {
                    centre
                } else {
                    box_point(bounds, [1.0 - frac[0], 1.0 - frac[1]])
                };
                let grip = box_point(bounds, frac);
                let point = [grip[0] + (point[0] - start[0]), grip[1] + (point[1] - start[1])];
                let moves_x = frac[0] != 0.5;
                let moves_y = frac[1] != 0.5;
                let mut sx = if moves_x { ratio(point[0] - origin[0], grip[0] - origin[0]) } else { 1.0 };
                let mut sy = if moves_y { ratio(point[1] - origin[1], grip[1] - origin[1]) } else { 1.0 };
                let scale_frame_text = self.frame_text_scale_handle && (self.modifier_ctrl || handle == Handle::FrameTextScale);
                if self.modifier_shift ^ (moves_x && moves_y && (self.proportional_text_corners || scale_frame_text)) {
                    if moves_x && moves_y {
                        let s = if sx.abs() > sy.abs() { sx.abs() } else { sy.abs() };
                        sx = s * sign(sx);
                        sy = s * sign(sy);
                    } else if moves_x {
                        sy = sx.abs();
                    } else {
                        sx = sy.abs();
                    }
                }
                args.origin = self.selection_frame.to_world(origin);
                args.scale = [non_zero(sx), non_zero(sy)];
                args.reflow_frame_text = !self.modifier_ctrl && handle != Handle::FrameTextScale;
                args.axes = (self.selection_frame.axes != [0.0, 0.0]).then_some(self.selection_frame.axes);
            }
            GestureKind::Rotate => {
                let origin = self.rotation_origin(self.modifier_alt);
                let a0 = (start[1] - origin[1]).atan2(start[0] - origin[0]);
                let a1 = (point[1] - origin[1]).atan2(point[0] - origin[0]);
                let mut deg = normalize_deg((a1 - a0).to_degrees());
                if self.modifier_shift {
                    deg = (deg / 15.0).round() * 15.0;
                }
                args.origin = origin;
                args.rotation_deg = deg;
            }
            GestureKind::Shear(handle) => {
                let start = self.selection_frame.to_local(start);
                let point = self.selection_frame.to_local(point);
                let grip_y = if handle == Handle::ShearBottom { bounds[1] + bounds[3] } else { bounds[1] };
                let origin = if let Some(origin) = self.transform_origin {
                    self.selection_frame.to_local(origin)
                } else if self.modifier_alt {
                    centre
                } else if handle == Handle::ShearBottom {
                    [centre[0], bounds[1]]
                } else {
                    [centre[0], bounds[1] + bounds[3]]
                };
                let height = grip_y - origin[1];
                let mut deg = if height == 0.0 {
                    0.0
                } else {
                    ((point[0] - start[0]) / height).atan().to_degrees()
                };
                if self.modifier_shift {
                    deg = (deg / 15.0).round() * 15.0;
                }
                args.origin = self.selection_frame.to_world(origin);
                args.axes = (self.selection_frame.axes != [0.0, 0.0]).then_some(self.selection_frame.axes);
                args.shear_deg = deg.clamp(-89.0, 89.0);
            }
        }

        args.separate_origins = self.transform_separately && kind != GestureKind::Move;
        Some(args)
    }

    pub fn rotation_origin(&self, centre: bool) -> [f32; 2] {
        self.transform_origin.unwrap_or_else(|| {
            let anchor = if centre { Anchor::Centre } else { self.anchor.unwrap_or(Anchor::Centre) };
            self.selection_frame.grip(anchor_frac(anchor))
        })
    }

    pub fn hit_handle(&self, bounds: [f32; 4], point: [f32; 2]) -> Option<Handle> {
        if !self.show_handles { return None; }
        let r = self.handle_radius;
        if self.frame_text_scale_handle && near(point, self.frame_text_scale_position(), r) {
            return Some(Handle::FrameTextScale);
        }
        let centre_x = bounds[0] + bounds[2] * 0.5;
        let rotate = self.selection_frame.to_world([centre_x, bounds[1] - self.rotate_offset]);
        if near(point, rotate, r) {
            return Some(Handle::Rotate);
        }
        let scale_handles = [
            Handle::TopLeft,
            Handle::Top,
            Handle::TopRight,
            Handle::Left,
            Handle::Right,
            Handle::BottomLeft,
            Handle::Bottom,
            Handle::BottomRight,
        ];
        let [top, bottom] = shear_handle_points(bounds, r);
        scale_handles.into_iter().map(|handle| (handle, box_point(bounds, handle_frac(handle))))
            .chain([(Handle::ShearTop, top), (Handle::ShearBottom, bottom)])
            .map(|(handle, centre)| (handle, self.selection_frame.to_world(centre)))
            .filter(|(_, centre)| near(point, *centre, r))
            .min_by(|(_, a), (_, b)| {
                let distance = |centre: &[f32; 2]| (point[0] - centre[0]).powi(2) + (point[1] - centre[1]).powi(2);
                distance(a).total_cmp(&distance(b))
            })
            .map(|(handle, _)| handle)
    }

    pub fn frame_text_scale_position(&self) -> [f32; 2] {
        let [x, y, w, h] = self.selection_frame.bounds;
        let gap = self.rotate_offset * 0.58;
        self.selection_frame.to_world([x + w + gap, y + h + gap])
    }

    fn preview(&mut self) -> MoveToolAction {
        match self.gesture {
            Some(GestureKind::Marquee) | None => {
                let start = match self.marquee_start {
                    Some(start) => start,
                    None => return MoveToolAction::None,
                };
                let point = self.last_point.unwrap_or(start);
                self.marquee_current = Some(point);
                MoveToolAction::MarqueeUpdate { rect: normalize_rect(start[0], start[1], point[0], point[1]) }
            }
            Some(GestureKind::Move) if self.duplicate_on_drag => match self.current_args() {
                Some(args) => MoveToolAction::DuplicatePreview { offset: args.translate },
                None => MoveToolAction::None,
            },
            Some(_) => match self.current_args() {
                Some(args) => MoveToolAction::TransformPreview { args },
                None => MoveToolAction::None,
            },
        }
    }

    fn reset(&mut self) {
        self.dragging = false;
        self.marquee_start = None;
        self.marquee_current = None;
        self.gesture = None;
        self.start_point = None;
        self.last_point = None;
        self.duplicate_on_drag = false;
    }

    fn resolve_marquee_mode(&self) -> MarqueeMode {
        if self.modifier_ctrl {
            MarqueeMode::Enclosed
        } else {
            MarqueeMode::Touch
        }
    }

    fn resolve_select_op(&self) -> SelectOp {
        if self.modifier_shift {
            SelectOp::Add
        } else {
            SelectOp::Replace
        }
    }
}

pub fn shear_handle_points([x, y, width, height]: [f32; 4], hit_radius: f32) -> [[f32; 2]; 2] {
    let preferred = x + width * 0.5 + hit_radius * 3.0;
    let right = x + width;
    // Keep the diamond clear of the corner handle on compact selections.
    let grip_x = if right - preferred >= hit_radius * 2.0 { preferred } else { right + hit_radius * 1.5 };
    [[grip_x, y], [grip_x, y + height]]
}

fn handle_frac(handle: Handle) -> [f32; 2] {
    match handle {
        Handle::TopLeft => [0.0, 0.0],
        Handle::Top => [0.5, 0.0],
        Handle::TopRight => [1.0, 0.0],
        Handle::Left => [0.0, 0.5],
        Handle::Right => [1.0, 0.5],
        Handle::BottomLeft => [0.0, 1.0],
        Handle::Bottom => [0.5, 1.0],
        Handle::BottomRight | Handle::FrameTextScale => [1.0, 1.0],
        Handle::Rotate => [0.5, 0.0],
        Handle::ShearTop => [0.5, 0.0],
        Handle::ShearBottom => [0.5, 1.0],
    }
}

fn anchor_frac(anchor: Anchor) -> [f32; 2] {
    dtool_core::command::anchor_fraction(anchor)
}

fn box_point(bounds: [f32; 4], frac: [f32; 2]) -> [f32; 2] {
    [bounds[0] + bounds[2] * frac[0], bounds[1] + bounds[3] * frac[1]]
}

fn contains(bounds: [f32; 4], point: [f32; 2]) -> bool {
    point[0] >= bounds[0] && point[0] <= bounds[0] + bounds[2] && point[1] >= bounds[1] && point[1] <= bounds[1] + bounds[3]
}

fn near(a: [f32; 2], b: [f32; 2], r: f32) -> bool {
    (a[0] - b[0]).abs() <= r && (a[1] - b[1]).abs() <= r
}

fn ratio(num: f32, den: f32) -> f32 {
    if den == 0.0 { 1.0 } else { num / den }
}

fn sign(v: f32) -> f32 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

fn non_zero(v: f32) -> f32 {
    if v.abs() < 0.0001 { 0.0001 * sign(v) } else { v }
}

fn normalize_deg(mut deg: f32) -> f32 {
    while deg > 180.0 {
        deg -= 360.0;
    }
    while deg <= -180.0 {
        deg += 360.0;
    }
    deg
}

fn constrain_delta(delta: [f32; 2]) -> [f32; 2] {
    let length = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    if length == 0.0 {
        return delta;
    }
    let angle = delta[1].atan2(delta[0]);
    let step = std::f32::consts::FRAC_PI_4;
    let snapped = (angle / step).round() * step;
    let projected = delta[0] * snapped.cos() + delta[1] * snapped.sin();
    [projected * snapped.cos(), projected * snapped.sin()]
}

fn is_identity(args: &TransformSelectionBoxArgs) -> bool {
    args.translate == [0.0, 0.0] && args.scale == [1.0, 1.0] && args.rotation_deg == 0.0 && args.shear_deg == 0.0
}

fn normalize_rect(x1: f32, y1: f32, x2: f32, y2: f32) -> [f32; 4] {
    let x = if x1 < x2 { x1 } else { x2 };
    let y = if y1 < y2 { y1 } else { y2 };
    let w = (x2 - x1).abs();
    let h = (y2 - y1).abs();
    [x, y, w, h]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeDirection {
    Left,
    Right,
    Up,
    Down,
}

pub fn nudge_delta(direction: NudgeDirection, shift: bool) -> [f32; 2] {
    let step = if shift { NUDGE_STEP_PX * NUDGE_SHIFT_MULTIPLIER } else { NUDGE_STEP_PX };
    match direction {
        NudgeDirection::Left => [-step, 0.0],
        NudgeDirection::Right => [step, 0.0],
        NudgeDirection::Up => [0.0, -step],
        NudgeDirection::Down => [0.0, step],
    }
}

pub fn nudge_args(direction: NudgeDirection, shift: bool) -> SelectionNudgeArgs {
    let delta = nudge_delta(direction, shift);
    SelectionNudgeArgs { dx: delta[0], dy: delta[1], ids: None }
}

impl MoveTool {
    pub fn on_arrow(&self, direction: NudgeDirection, shift: bool) -> Option<SelectionNudgeArgs> {
        if self.dragging || self.gesture.is_some() {
            return None;
        }
        Some(nudge_args(direction, shift))
    }
}

pub fn selection_command_name(action: &MoveToolAction) -> Option<&'static str> {
    match action {
        MoveToolAction::HitSelect { .. } => Some("selection.hit"),
        MoveToolAction::EnterGroup { .. } => Some("selection.enter"),
        MoveToolAction::ExitGroup => Some("selection.exit"),
        _ => None,
    }
}

pub fn alt_drag_duplicate_offset(action: &MoveToolAction) -> Option<[f32; 2]> {
    match action {
        MoveToolAction::DuplicateCommit { offset } => Some(*offset),
        _ => None,
    }
}

