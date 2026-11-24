use dtool_design::{color, icon_button, space};
use dtool_tools::art_text::{ArtTextTool, HandleKind};
use egui::{Context, PointerButton, Response, Sense, Ui, Vec2};

pub struct PathTextHandles {
    pub tool: ArtTextTool,
}

impl PathTextHandles {
    pub fn new() -> Self {
        Self {
            tool: ArtTextTool::new(),
        }
    }

    pub fn draw_handles(&mut self, ui: &Ui, story_id: u64, path_id: u64, start_pos: Vec2, end_pos: Vec2, baseline_pos: Vec2) {
        let start_color = color("error.red");
        let end_color = color("success.green");
        
        let start_response = ui.allocate_rect(egui::Rect::from_center_size(start_pos, egui::vec2(10.0, 10.0)), Sense::click_and_drag());
        if start_response.drag_started() {
            self.tool.begin_drag(HandleKind::Start, start_pos, ui.input(|i| i.modifiers.shift), ui.input(|i| i.modifiers.ctrl), ui.input(|i| i.modifiers.alt));
        }
        if start_response.dragged() {
            self.tool.update_drag(ui.pointer_hover_pos().unwrap_or(start_pos));
        }
        if start_response.drag_stopped() {
            if let Some(cmd) = self.tool.commit_drag(story_id, path_id) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CustomEvent(serde_json::to_string(&cmd).unwrap()));
            }
        }
        
        ui.painter().circle_filled(start_pos, 5.0, start_color);

        let end_response = ui.allocate_rect(egui::Rect::from_center_size(end_pos, egui::vec2(10.0, 10.0)), Sense::click_and_drag());
        if end_response.drag_started() {
            self.tool.begin_drag(HandleKind::End, end_pos, ui.input(|i| i.modifiers.shift), ui.input(|i| i.modifiers.ctrl), ui.input(|i| i.modifiers.alt));
        }
        if end_response.dragged() {
            self.tool.update_drag(ui.pointer_hover_pos().unwrap_or(end_pos));
        }
        if end_response.drag_stopped() {
            if let Some(cmd) = self.tool.commit_drag(story_id, path_id) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CustomEvent(serde_json::to_string(&cmd).unwrap()));
            }
        }
        
        ui.painter().circle_filled(end_pos, 5.0, end_color);

        let baseline_response = ui.allocate_rect(egui::Rect::from_center_size(baseline_pos, egui::vec2(10.0, 10.0)), Sense::click_and_drag());
        if baseline_response.drag_started() {
            self.tool.begin_drag(HandleKind::Baseline, baseline_pos, ui.input(|i| i.modifiers.shift), ui.input(|i| i.modifiers.ctrl), ui.input(|i| i.modifiers.alt));
        }
        if baseline_response.dragged() {
            self.tool.update_drag(ui.pointer_hover_pos().unwrap_or(baseline_pos));
        }
        if baseline_response.drag_stopped() {
            if let Some(cmd) = self.tool.commit_drag(story_id, path_id) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CustomEvent(serde_json::to_string(&cmd).unwrap()));
            }
        }
        
        ui.painter().circle_filled(baseline_pos, 5.0, color("panel.fg"));

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tool.cancel_drag();
        }
    }

    pub fn handle_click(&mut self, story_id: u64) {
        self.tool.enter_editing(story_id);
    }

    pub fn draw_status_bar(&self, ui: &Ui) {
        if self.tool.active_drag.is_some() {
            let status = self.tool.get_status_text();
            ui.label(status);
        }
    }

    pub fn process_click_on_path_text(
        &mut self, 
        ui: &Ui, 
        layout: &dtool_text::PathTextLayout, 
        click_pos: Vec2, 
        story_id: u64
    ) -> Option<usize> {
        if let Some(glyph_idx) = self.tool.hit_test_glyph(layout, click_pos) {
            let caret_pos = self.tool.caret_position_from_hit(layout, glyph_idx, click_pos);
            self.tool.enter_editing(story_id);
            return Some(caret_pos);
        }
        None
    }

    pub fn draw_overset_indicator(&mut self, ui: &Ui, end_pos: Vec2) {
        let icon_size = 16.0;
        let rect = egui::Rect::from_center_size(end_pos, egui::vec2(icon_size, icon_size));
        
        if let Some(icon_bytes) = dtool_design::icon("text.overset") {
            let texture = ui.ctx().load_inline_image(egui::ImageSource::Bytes { source: std::sync::Arc::new(egui::load::Bytes::Static(icon_bytes)) });
            ui.painter().image(texture, rect.center(), rect.size(), color("error.red"));
        } else {
            ui.painter().circle_filled(end_pos, 8.0, color("error.red"));
        }
    }
}
