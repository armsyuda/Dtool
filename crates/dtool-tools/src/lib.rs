pub mod move_tool;
pub use dtool_text;

pub const ZOOM_MIN: f32 = 0.01;
pub const ZOOM_MAX: f32 = 100.0;
pub const WHEEL_ZOOM_RATE: f32 = 0.0015;

pub fn crate_ready() -> bool {
    true
}

pub fn clamp_zoom(z: f32) -> f32 {
    if z.is_nan() {
        return 1.0;
    }
    z.clamp(ZOOM_MIN, ZOOM_MAX)
}

pub type ViewNav = dtool_core::ViewState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewCommand {
    Pan { delta: [f32; 2] },
    ZoomSet { zoom: f32, around: Option<[f32; 2]> },
    RotateSet { degrees: f32 },
}

impl ViewCommand {
    pub fn name(&self) -> &'static str {
        match self {
            ViewCommand::Pan { .. } => "view.pan",
            ViewCommand::ZoomSet { .. } => "view.zoom.set",
            ViewCommand::RotateSet { .. } => "view.rotate.set",
        }
    }
}

pub use dtool_core::view::{doc_to_view, view_to_doc};

pub fn apply_view_command(state: &mut ViewNav, command: ViewCommand, view_size: [f32; 2]) {
    match command {
        ViewCommand::Pan { delta } => {
            dtool_core::command::view_pan_apply(state, delta);
        }
        ViewCommand::ZoomSet { zoom, around } => {
            let around = around.unwrap_or([view_size[0] * 0.5, view_size[1] * 0.5]);
            dtool_core::view::zoom_about(state, zoom, around);
        }
        ViewCommand::RotateSet { degrees } => {
            state.rotation_deg = dtool_core::command::view_rotation_wrap(degrees);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NavInput {
    pub space_down: bool,
    pub dragging: bool,
    pub drag_delta: [f32; 2],
    pub pointer: Option<[f32; 2]>,
    pub wheel_y: f32,
    pub pinch_factor: f32,
    pub rotation_delta_deg: f32,
}

#[derive(Debug, Clone, Default)]
pub struct HandNavigator {
    pub panning: bool,
}

impl HandNavigator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn commands(&mut self, state: &ViewNav, input: &NavInput) -> Vec<ViewCommand> {
        let mut out = Vec::new();

        self.panning = input.space_down && input.dragging;
        if self.panning && (input.drag_delta[0] != 0.0 || input.drag_delta[1] != 0.0) {
            out.push(ViewCommand::Pan {
                delta: input.drag_delta,
            });
        }

        let mut factor = if input.pinch_factor > 0.0 { input.pinch_factor } else { 1.0 };
        if factor == 1.0 && input.wheel_y != 0.0 {
            factor = (input.wheel_y * WHEEL_ZOOM_RATE).exp();
        }
        if factor != 1.0 {
            let zoom = clamp_zoom(state.zoom * factor);
            if zoom != state.zoom {
                out.push(ViewCommand::ZoomSet {
                    zoom,
                    around: input.pointer,
                });
            }
        }

        if input.rotation_delta_deg != 0.0 {
            out.push(ViewCommand::RotateSet {
                degrees: state.rotation_deg + input.rotation_delta_deg,
            });
        }

        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavigationTool { #[default] View, Zoom }

pub fn zoom_tool_click(view: &mut dtool_core::ViewState, point: [f32; 2], alt: bool) {
    let factor = if alt { 0.5 } else { 2.0 };
    let anchor = dtool_core::view_to_doc(view, point);
    view.zoom = dtool_core::clamp_zoom(view.zoom * factor);
    let mapped = dtool_core::doc_to_view(view, anchor);
    view.scroll[0] += point[0] - mapped[0];
    view.scroll[1] += point[1] - mapped[1];
}

pub fn zoom_tool_marquee(view: &mut dtool_core::ViewState, start: [f32; 2], end: [f32; 2], size: [f32; 2]) {
    let width = (end[0] - start[0]).abs();
    let height = (end[1] - start[1]).abs();
    if width <= f32::EPSILON || height <= f32::EPSILON || !size.iter().all(|v| v.is_finite() && *v > 0.0) { return; }
    let midpoint = [(start[0] + end[0]) * 0.5, (start[1] + end[1]) * 0.5];
    let anchor = dtool_core::view_to_doc(view, midpoint);
    view.zoom = dtool_core::clamp_zoom(view.zoom * (size[0] / width).min(size[1] / height));
    let mapped = dtool_core::doc_to_view(view, anchor);
    view.scroll[0] += size[0] * 0.5 - mapped[0];
    view.scroll[1] += size[1] * 0.5 - mapped[1];
}

pub fn view_tool_pan(view: &mut dtool_core::ViewState, delta: [f32; 2]) {
    view.scroll[0] += delta[0];
    view.scroll[1] += delta[1];
}

pub mod shape_tool;
pub mod qr_tool;
pub mod vector_crop;
pub mod style_picker;

pub mod selection_tools;

pub mod paint_tools;

pub mod art_text;

pub mod frame_text;

pub mod perspective_tool;
