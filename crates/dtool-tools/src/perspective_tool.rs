use dtool_core::pixel_perspective::pixel_perspective_revert;
use dtool_core::{
    pixel_perspective_apply, pixel_perspective_cancel, warp_pixel_layer, CommandError, CommandResult,
    Document, ObjectId, PerspectiveMode, PixelLayer, PixelPerspective, Session,
};

pub const KEY_SHOW_GRID: &str = "perspective.show_grid";
pub const KEY_PRESERVE_ALPHA: &str = "perspective.preserve_alpha";
pub const KEY_MODE_SINGLE: &str = "perspective.mode.single";
pub const KEY_MODE_TWO: &str = "perspective.mode.two";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CornerHandle {
    pub plane: usize,
    pub corner: usize,
}

#[derive(Clone)]
pub struct PerspectiveTool {
    pub state: Option<PixelPerspective>,
    pub preview: Option<PixelLayer>,
    pub drag: Option<CornerHandle>,
    pub layer_size: [u32; 2],
    pub handle_radius: f32,
    original: Option<PixelLayer>,
}

impl Default for PerspectiveTool {
    fn default() -> Self {
        Self::new()
    }
}

impl PerspectiveTool {
    pub fn new() -> Self {
        Self {
            state: None,
            preview: None,
            drag: None,
            layer_size: [0, 0],
            handle_radius: 6.0,
            original: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.state.is_some()
    }

    pub fn from_layer(layer_id: ObjectId, layer: &PixelLayer, mode: PerspectiveMode) -> Self {
        let mut tool = Self::new();
        tool.original = Some(layer.clone());
        tool.begin(layer_id, layer.width, layer.height, mode);
        tool
    }

    pub fn start(&mut self, doc: &Document, layer_id: ObjectId, mode: PerspectiveMode) -> Result<(), CommandError> {
        let layer = doc.pixel_layers.get(layer_id).ok_or_else(CommandError::not_found)?;
        if layer.width == 0 || layer.height == 0 {
            return Err(CommandError::invalid_params());
        }
        self.original = Some(layer.clone());
        self.begin(layer_id, layer.width, layer.height, mode);
        Ok(())
    }

    fn begin(&mut self, layer_id: ObjectId, width: u32, height: u32, mode: PerspectiveMode) {
        let mut state = PixelPerspective::for_layer(layer_id, width, height, mode);
        if let Some(old) = self.state.as_ref() {
            state.preserve_alpha = old.preserve_alpha;
            state.show_grid = old.show_grid;
        }
        self.state = Some(state);
        self.preview = None;
        self.drag = None;
        self.layer_size = [width, height];
    }

    pub fn mode(&self) -> Option<PerspectiveMode> {
        self.state.as_ref().map(|s| s.mode)
    }

    pub fn set_mode(&mut self, mode: PerspectiveMode) {
        let (layer, keep) = match self.state.as_ref() {
            Some(s) if s.mode != mode => (s.layer, true),
            _ => return,
        };
        if keep {
            let [w, h] = self.layer_size;
            self.begin(layer, w, h, mode);
        }
    }

    pub fn mode_key(&self) -> &'static str {
        match self.mode() {
            Some(PerspectiveMode::TwoPlanes) => KEY_MODE_TWO,
            _ => KEY_MODE_SINGLE,
        }
    }

    pub fn show_grid(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.show_grid)
    }

    pub fn set_show_grid(&mut self, on: bool) {
        if let Some(s) = self.state.as_mut() {
            s.show_grid = on;
        }
    }

    pub fn toggle_show_grid(&mut self) {
        let on = !self.show_grid();
        self.set_show_grid(on);
    }

    pub fn preserve_alpha(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.preserve_alpha)
    }

    pub fn set_preserve_alpha(&mut self, on: bool) {
        if let Some(s) = self.state.as_mut() {
            if s.preserve_alpha != on {
                s.preserve_alpha = on;
                self.preview = None;
            }
        }
    }

    pub fn toggle_preserve_alpha(&mut self) {
        let on = !self.preserve_alpha();
        self.set_preserve_alpha(on);
    }

    pub fn plane_count(&self) -> usize {
        self.state.as_ref().map_or(0, |s| s.plane_count())
    }

    pub fn plane_corners(&self, plane: usize) -> Option<[[f32; 2]; 4]> {
        let s = self.state.as_ref()?;
        match plane {
            0 => Some(s.dst),
            1 if s.mode == PerspectiveMode::TwoPlanes => Some(s.dst2),
            _ => None,
        }
    }

    pub fn handles(&self) -> Vec<(CornerHandle, [f32; 2])> {
        let mut out = Vec::new();
        for plane in 0..self.plane_count() {
            if let Some(q) = self.plane_corners(plane) {
                for (corner, p) in q.iter().enumerate() {
                    out.push((CornerHandle { plane, corner }, *p));
                }
            }
        }
        out
    }

    pub fn hit_handle(&self, point: [f32; 2]) -> Option<CornerHandle> {
        let r2 = self.handle_radius * self.handle_radius;
        let mut best: Option<(CornerHandle, f32)> = None;
        for (h, p) in self.handles() {
            let dx = p[0] - point[0];
            let dy = p[1] - point[1];
            let d2 = dx * dx + dy * dy;
            if d2 <= r2 && best.map_or(true, |(_, b)| d2 < b) {
                best = Some((h, d2));
            }
        }
        best.map(|(h, _)| h)
    }

    fn corner_mut(state: &mut PixelPerspective, handle: CornerHandle) -> Option<&mut [f32; 2]> {
        if handle.corner >= 4 {
            return None;
        }
        match handle.plane {
            0 => Some(&mut state.dst[handle.corner]),
            1 if state.mode == PerspectiveMode::TwoPlanes => Some(&mut state.dst2[handle.corner]),
            _ => None,
        }
    }

    pub fn move_corner(&mut self, handle: CornerHandle, point: [f32; 2]) -> bool {
        if !point[0].is_finite() || !point[1].is_finite() {
            return false;
        }
        let state = match self.state.as_mut() {
            Some(s) => s,
            None => return false,
        };
        let old = match Self::corner_mut(state, handle) {
            Some(c) => *c,
            None => return false,
        };
        if let Some(c) = Self::corner_mut(state, handle) {
            *c = point;
        }
        if state.mode == PerspectiveMode::TwoPlanes {
            let other_plane = 1 - handle.plane.min(1);
            for corner in 0..4 {
                let other = CornerHandle { plane: other_plane, corner };
                if let Some(c) = Self::corner_mut(state, other) {
                    if c[0] == old[0] && c[1] == old[1] {
                        *c = point;
                    }
                }
            }
        }
        self.preview = None;
        true
    }

    pub fn press(&mut self, point: [f32; 2]) -> Option<CornerHandle> {
        self.drag = self.hit_handle(point);
        self.drag
    }

    pub fn drag_to(&mut self, point: [f32; 2]) -> bool {
        match self.drag {
            Some(h) => self.move_corner(h, point),
            None => false,
        }
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    pub fn reset(&mut self) {
        if let Some(s) = self.state.as_mut() {
            s.dst = s.src;
            s.dst2 = s.src2;
        }
        self.preview = None;
        self.drag = None;
    }

    fn rotate(&mut self, clockwise: bool) {
        let state = match self.state.as_mut() {
            Some(s) => s,
            None => return,
        };
        let two = state.mode == PerspectiveMode::TwoPlanes;
        let mut cx = 0.0f32;
        let mut cy = 0.0f32;
        let mut n = 0.0f32;
        let planes: &[[[f32; 2]; 4]] = if two { &[state.dst, state.dst2] } else { &[state.dst] };
        for q in planes.iter() {
            for p in q.iter() {
                cx += p[0];
                cy += p[1];
                n += 1.0;
            }
        }
        cx /= n;
        cy /= n;
        let turn = |p: &mut [f32; 2]| {
            let dx = p[0] - cx;
            let dy = p[1] - cy;
            *p = if clockwise { [cx - dy, cy + dx] } else { [cx + dy, cy - dx] };
        };
        state.dst.iter_mut().for_each(turn);
        if two {
            state.dst2.iter_mut().for_each(turn);
        }
        self.preview = None;
    }

    pub fn rotate_clockwise(&mut self) {
        self.rotate(true);
    }

    pub fn rotate_anticlockwise(&mut self) {
        self.rotate(false);
    }

    pub fn refresh_preview(&mut self, doc: &Document) -> Result<Option<&PixelLayer>, CommandError> {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Ok(None),
        };
        let layer = doc
            .pixel_layers
            .get(state.layer)
            .ok_or_else(CommandError::not_found)?;
        let source = self.original.as_ref().unwrap_or(layer);
        self.preview = Some(warp_pixel_layer(source, state)?);
        Ok(self.preview.as_ref())
    }

    pub fn grid_lines(&self, divisions: u32) -> Vec<[[f32; 2]; 2]> {
        let mut out = Vec::new();
        if !self.show_grid() || divisions == 0 {
            return out;
        }
        let lerp = |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        for plane in 0..self.plane_count() {
            if let Some(q) = self.plane_corners(plane) {
                for i in 0..=divisions {
                    let t = i as f32 / divisions as f32;
                    out.push([lerp(q[0], q[1], t), lerp(q[3], q[2], t)]);
                    out.push([lerp(q[0], q[3], t), lerp(q[1], q[2], t)]);
                }
            }
        }
        out
    }

    pub fn apply(&mut self, session: &mut Session, doc: &mut Document) -> Result<CommandResult, CommandError> {
        let state = self.state.clone().ok_or_else(|| CommandError::validation("NO_PERSPECTIVE"))?;
        if let Some(original) = self.original.as_ref() {
            doc.pixel_layers.insert(state.layer, original.clone());
        }
        let result = pixel_perspective_apply(session, doc, &state)?;
        if result.is_ok() {
            self.state = None;
            self.preview = None;
            self.drag = None;
            self.original = None;
        }
        Ok(result)
    }

    pub fn cancel(&mut self, session: &mut Session, doc: &mut Document) -> Result<CommandResult, CommandError> {
        if let (Some(orig), Some(state)) = (self.original.take(), self.state.as_ref()) {
            pixel_perspective_revert(doc, state.layer, orig);
        }
        self.state = None;
        self.preview = None;
        self.drag = None;
        pixel_perspective_cancel(session, doc)
    }
}
