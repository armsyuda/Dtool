use dtool_core::slice::{create_manual_slice, SliceId};
use dtool_core::CommandError;

pub struct SliceToolState {
    pub start: Option<[f32; 2]>,
    pub current: Option<[f32; 2]>,
    pub shift_held: bool,
    pub alt_held: bool,
}

impl SliceToolState {
    pub fn new() -> Self {
        Self {
            start: None,
            current: None,
            shift_held: false,
            alt_held: false,
        }
    }

    pub fn begin_drag(&mut self, x: f32, y: f32) {
        self.start = Some([x, y]);
        self.current = Some([x, y]);
    }

    pub fn update_drag(&mut self, x: f32, y: f32, shift: bool, alt: bool) {
        if let Some(start) = self.start {
            self.shift_held = shift;
            self.alt_held = alt;
            
            let mut dx = x - start[0];
            let mut dy = y - start[1];

            if self.shift_held {
                let abs_dx = dx.abs();
                let abs_dy = dy.abs();
                let max_abs = abs_dx.max(abs_dy);
                
                if abs_dx > abs_dy {
                    dy = max_abs * dy.signum();
                } else {
                    dx = max_abs * dx.signum();
                }
            }

            if self.alt_held {
                let center_x = start[0];
                let center_y = start[1];
                self.current = Some([center_x + dx, center_y + dy]);
            } else {
                self.current = Some([x, y]);
            }
        }
    }

    pub fn finish_drag(&mut self) -> Option<[f32; 4]> {
        if let (Some(start), Some(current)) = (self.start, self.current) {
            let bounds = [start[0], start[1], current[0], current[1]];
            self.start = None;
            self.current = None;
            Some(bounds)
        } else {
            None
        }
    }
}

pub fn create_slice_from_drag(bounds: [f32; 4], name: String, id: SliceId) -> dtool_core::slice::Slice {
    create_manual_slice(bounds, name, id)
}

pub fn validate_slice_options(params: &serde_json::Value) -> Result<(), CommandError> {
    if let Some(width) = params.get("width").and_then(|v| v.as_f64()) {
        if width < 0.0 {
            return Err(CommandError::invalid_params("ui.tool.options.set", "width must be non-negative"));
        }
    }
    if let Some(height) = params.get("height").and_then(|v| v.as_f64()) {
        if height < 0.0 {
            return Err(CommandError::invalid_params("ui.tool.options.set", "height must be non-negative"));
        }
    }
    Ok(())
}
