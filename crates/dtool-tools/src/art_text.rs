use dtool_core::Command;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
pub enum HandleKind {
    Start,
    End,
    Baseline,
    Flip,
}

#[derive(Clone, Copy, PartialEq)]
pub struct DragState {
    pub handle: HandleKind,
    pub start_pos: [f32; 2],
    pub current_pos: [f32; 2],
    pub shift_held: bool,
    pub ctrl_held: bool,
    pub alt_held: bool,
}

#[derive(Clone, Debug)]
pub struct ArtTextGlyph {
    pub pos: [f32; 2],
    pub start: usize,
    pub end: usize,
}

#[derive(Clone)]
pub struct ArtTextTool {
    pub active_drag: Option<DragState>,
    pub preview_flows: HashMap<u64, Vec<f32>>,
    pub editing_story_id: Option<u64>,
    pub hovering_curve: bool,
    pub hover_pos: Option<[f32; 2]>,
}

impl ArtTextTool {
    pub fn new() -> Self {
        Self {
            active_drag: None,
            preview_flows: HashMap::new(),
            editing_story_id: None,
            hovering_curve: false,
            hover_pos: None,
        }
    }

    pub fn click(&self, pos: [f32; 2]) -> Command {
        let mut command = Command::new("text.art.add");
        command.params["pos"] = pos.to_vec().into();
        command
    }

    pub fn drag(&self, start: [f32; 2], end: [f32; 2]) -> Command {
        self.drag_in_document(start, end, 1.0)
    }

    pub fn drag_in_document(&self, start: [f32; 2], end: [f32; 2], units_per_point: f32) -> Command {
        let pos = [start[0].min(end[0]), start[1].min(end[1])];
        let mut size_pt = (end[1] - start[1]).abs() / units_per_point;
        if !size_pt.is_finite() || size_pt <= 0.0 {
            size_pt = 12.0;
        }
        let mut command = Command::new("text.art.add");
        command.params["pos"] = pos.to_vec().into();
        command.params["size_pt"] = f64::from(size_pt).into();
        command
    }

    pub fn begin_drag(&mut self, handle: HandleKind, pos: [f32; 2], shift: bool, ctrl: bool, alt: bool) {
        self.active_drag = Some(DragState {
            handle,
            start_pos: pos,
            current_pos: pos,
            shift_held: shift,
            ctrl_held: ctrl,
            alt_held: alt,
        });
    }

    pub fn update_drag(&mut self, pos: [f32; 2]) {
        if let Some(ref mut drag) = self.active_drag {
            drag.current_pos = pos;
        }
    }

    pub fn cancel_drag(&mut self) {
        self.active_drag = None;
        self.preview_flows.clear();
    }

    pub fn commit_drag(&mut self, story_id: u64, path_id: u64) -> Option<Command> {
        if let Some(drag) = &self.active_drag {
            let dx = drag.current_pos[0] - drag.start_pos[0];
            let dy = drag.current_pos[1] - drag.start_pos[1];
            let flows = match drag.handle {
                HandleKind::Start => vec![dx],
                HandleKind::End => vec![dy],
                HandleKind::Baseline => vec![dx, dy],
                HandleKind::Flip => vec![0.0],
            };

            self.preview_flows.insert(story_id, flows.clone());

            let mut command = Command::new("text.path.set");
            command.params["id"] = path_id.into();
            command.params["flows"] = flows.into_iter().map(f64::from).collect::<Vec<f64>>().into();
            self.active_drag = None;
            Some(command)
        } else {
            None
        }
    }

    pub fn enter_editing(&mut self, story_id: u64) {
        self.editing_story_id = Some(story_id);
    }

    pub fn exit_editing(&mut self) -> Option<Command> {
        if self.editing_story_id.is_some() {
            self.editing_story_id = None;
            let mut command = Command::new("text.edit.end");
            Some(command)
        } else {
            None
        }
    }

    pub fn is_editing(&self) -> bool {
        self.editing_story_id.is_some()
    }

    pub fn set_hovering_curve(&mut self, hovering: bool, pos: Option<[f32; 2]>) {
        self.hovering_curve = hovering;
        self.hover_pos = pos;
    }

    pub fn is_hovering_curve(&self) -> bool {
        self.hovering_curve
    }

    pub fn get_status_text(&self) -> String {
        if self.hovering_curve {
            "Click to convert the Curve to a Text Path".to_string()
        } else {
            "Click to add artistic text, or drag to set the point size from the drag height.".to_string()
        }
    }

    pub fn create_path_text(&mut self, path_id: &str, pos: [f32; 2]) -> Option<Command> {
        if !self.hovering_curve {
            return None;
        }

        let mut command = Command::new("text.path_text.add");
        command.params["path"] = path_id.into();
        command.params["pos"] = pos.to_vec().into();
        self.hovering_curve = false;
        self.hover_pos = None;
        Some(command)
    }

    pub fn hit_test_glyph(&self, glyphs: &[ArtTextGlyph], click_pos: [f32; 2]) -> Option<usize> {
        let mut nearest_idx = None;
        let mut min_dist_sq = f32::MAX;

        for (i, glyph) in glyphs.iter().enumerate() {
            let dx = glyph.pos[0] - click_pos[0];
            let dy = glyph.pos[1] - click_pos[1];
            let dist_sq = dx * dx + dy * dy;
            if dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                nearest_idx = Some(i);
            }
        }

        nearest_idx
    }

    pub fn caret_position_from_hit(&self, glyphs: &[ArtTextGlyph], glyph_idx: usize, click_pos: [f32; 2]) -> usize {
        if glyph_idx >= glyphs.len() {
            return 0;
        }

        let glyph = &glyphs[glyph_idx];
        let dx = glyph.pos[0] - click_pos[0];
        let dy = glyph.pos[1] - click_pos[1];
        let dist_to_start_sq = dx * dx + dy * dy;

        let dist_to_end_sq = if glyph_idx + 1 < glyphs.len() {
            let next_glyph = &glyphs[glyph_idx + 1];
            let ndx = next_glyph.pos[0] - click_pos[0];
            let ndy = next_glyph.pos[1] - click_pos[1];
            ndx * ndx + ndy * ndy
        } else {
            dist_to_start_sq + 1.0
        };

        if dist_to_start_sq <= dist_to_end_sq {
            glyph.start
        } else {
            glyph.end
        }
    }

    pub fn select_word(&self, story_id: u64, text: &str, at: usize) -> Command {
        select_range(story_id, dtool_text::story::word_range(text, at))
    }

    pub fn select_paragraph(&self, story_id: u64, text: &str, at: usize) -> Command {
        select_range(story_id, dtool_text::story::paragraph_range(text, at))
    }

    pub fn insert_text(&self, story_id: u64, at: usize, text: &str) -> Command {
        let mut command = Command::new("text.insert");
        command.params["story"] = story_id.into();
        command.params["at"] = at.into();
        command.params["text"] = text.into();
        command
    }

    pub fn delete_text(&self, story_id: u64, start: usize, end: usize) -> Command {
        let mut command = Command::new("text.delete");
        command.params["story"] = story_id.into();
        command.params["range"]["start"] = start.into();
        command.params["range"]["end"] = end.into();
        command
    }
}

impl Default for ArtTextTool {
    fn default() -> Self {
        Self::new()
    }
}

fn select_range(story_id: u64, range: dtool_text::TextRange) -> Command {
    let mut command = Command::new("text.select");
    command.params["story"] = story_id.into();
    command.params["range"]["start"] = range.start.into();
    command.params["range"]["end"] = range.end.into();
    command
}
