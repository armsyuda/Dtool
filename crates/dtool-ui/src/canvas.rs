pub mod frame_text_input;
pub mod longdoc_marks;

use dtool_core::view_state::ViewState;
use dtool_core::workspace::Workspace;

pub use dtool_core::view::{clamp_zoom, doc_to_view, view_to_doc};
pub use dtool_core::view_state::{ZOOM_MAX, ZOOM_MIN};
pub const WHEEL_ZOOM_RATE: f32 = 0.0015;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanvasViewCommand {
    Pan { delta: [f32; 2] },
    ZoomSet { zoom: f32, around: Option<[f32; 2]> },
    RotateSet { degrees: f32 },
}

impl CanvasViewCommand {
    pub fn name(&self) -> &'static str {
        match self {
            CanvasViewCommand::Pan { .. } => "view.pan",
            CanvasViewCommand::ZoomSet { .. } => "view.zoom.set",
            CanvasViewCommand::RotateSet { .. } => "view.rotate.set",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CanvasView {
    pub panning: bool,
    pub last_commands: Vec<CanvasViewCommand>,
}

impl CanvasView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, state: &mut ViewState, command: CanvasViewCommand, view_size: [f32; 2]) {
        match command {
            CanvasViewCommand::Pan { delta } => {
                dtool_core::command::view_pan_apply(state, delta);
            }
            CanvasViewCommand::ZoomSet { zoom, around } => {
                let around = around.unwrap_or([view_size[0] * 0.5, view_size[1] * 0.5]);
                dtool_core::view::zoom_about(state, zoom, around);
            }
            CanvasViewCommand::RotateSet { degrees } => {
                state.rotation_deg = dtool_core::command::view_rotation_wrap(degrees);
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut ViewState) -> egui::Response {
        let response = ui.allocate_response(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        let view_size = [rect.width(), rect.height()];

        let (space_down, wheel_y, pinch, rotation_delta) = ui.input(|i| {
            (
                i.key_down(egui::Key::Space),
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.rotation_delta(),
            )
        });
        let pointer = response
            .hover_pos()
            .map(|p| [p.x - rect.min.x, p.y - rect.min.y]);

        let mut commands = Vec::new();

        self.panning = space_down && response.dragged();
        if self.panning {
            let d = response.drag_delta();
            if d.x != 0.0 || d.y != 0.0 {
                commands.push(CanvasViewCommand::Pan { delta: [d.x, d.y] });
            }
        }

        if response.hovered() {
            let mut factor = pinch;
            if factor == 1.0 && wheel_y != 0.0 {
                factor = (wheel_y * WHEEL_ZOOM_RATE).exp();
            }
            if factor != 1.0 {
                let zoom = clamp_zoom(state.zoom * factor);
                if zoom != state.zoom {
                    commands.push(CanvasViewCommand::ZoomSet { zoom, around: pointer });
                }
            }

            if rotation_delta != 0.0 {
                commands.push(CanvasViewCommand::RotateSet {
                    degrees: state.rotation_deg + rotation_delta.to_degrees(),
                });
            }
        }

        for command in &commands {
            self.apply(state, *command, view_size);
        }
        self.last_commands = commands;

        if self.panning {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if space_down && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        response
    }

    pub fn show_active(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) -> Option<egui::Response> {
        let id = workspace.active()?;
        let (response, view) = {
            let state = workspace.view_mut(id)?;
            let response = self.show(ui, state);
            (response, state.clone())
        };
        if !self.panning {
            if let Some(session) = workspace.session_mut() {
                frame_text_input::handle(ui, session, &response, &view);
            }
        }
        Some(response)
    }
}
