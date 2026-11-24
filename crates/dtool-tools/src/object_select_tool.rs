use dtool_design::{button, icon_button, panel_frame, panel_header, toggle, dropdown};
use dtool_strings::text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Box,
    Click,
}

#[derive(Debug, Clone)]
pub struct ObjectSelectionTool {
    pub mode: SelectionMode,
    pub multi_part_objects: bool,
    pub soft_edges: bool,
    pub refine: bool,
}

impl Default for ObjectSelectionTool {
    fn default() -> Self {
        Self {
            mode: SelectionMode::Box,
            multi_part_objects: false,
            soft_edges: false,
            refine: false,
        }
    }
}

pub fn render_context_bar(ui: &mut egui::Ui, tool: &mut ObjectSelectionTool) {
    panel_frame(ui, |ui| {
        panel_header(ui, text("tool.object_selection"));

        ui.horizontal(|ui| {
            let mode_label = match tool.mode {
                SelectionMode::Box => "box",
                SelectionMode::Click => "click",
            };
            dropdown(ui, mode_label);

            toggle(ui, &mut tool.multi_part_objects, text("ai.select_subject"));
            toggle(ui, &mut tool.soft_edges, "Soft Edges");
            toggle(ui, &mut tool.refine, "Refine");
        });

        ui.horizontal(|ui| {
            button(ui, text("ai.select_subject"), || {});
        });
    });
}
