use dtool_design::{color, space, icon};
use dtool_strings::text;
use dtool_tools::paint_tools::PaintToolOptions;

pub struct FloodFillContextBar {
    pub tolerance: f32,
    pub contiguous: bool,
    pub source_current_layer: bool,
}

impl Default for FloodFillContextBar {
    fn default() -> Self {
        Self {
            tolerance: 0.0,
            contiguous: true,
            source_current_layer: true,
        }
    }
}

pub fn render_flood_fill_context_bar(
    ui: &mut egui::Ui,
    options: &mut FloodFillContextBar,
) {
    let frame = dtool_design::panel_frame(ui);
    
    dtool_design::context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(text("brush.tolerance"));
            dtool_design::slider(ui, &mut options.tolerance, 0.0..=100.0);
            
            ui.add_space(space("sm"));
            
            dtool_design::toggle(ui, &mut options.contiguous, text("brush.contiguous"));
            
            ui.add_space(space("sm"));
            
            dtool_design::dropdown(ui, &mut options.source_current_layer, vec![
                (true, text("layer.current")),
                (false, text("layer.all")),
            ]);
        });
    });
}

pub struct FillToolContextBar {
    pub context_fill: bool,
    pub fill_type: String,
    pub colour: [u8; 4],
    pub pigment_blend: bool,
}

impl Default for FillToolContextBar {
    fn default() -> Self {
        Self {
            context_fill: false,
            fill_type: "solid".to_string(),
            colour: [0, 0, 0, 255],
            pigment_blend: true,
        }
    }
}

pub fn render_fill_tool_context_bar(
    ui: &mut egui::Ui,
    options: &mut FillToolContextBar,
) {
    dtool_design::context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            dtool_design::toggle(ui, &mut options.context_fill, text("fill.context"));
            
            ui.add_space(space("sm"));
            
            dtool_design::dropdown(ui, &mut options.fill_type, vec![
                ("solid".to_string(), text("fill.solid")),
                ("gradient".to_string(), text("fill.gradient")),
            ]);
            
            ui.add_space(space("sm"));
            
            dtool_design::colour_well(ui, &mut options.colour);
            
            ui.add_space(space("sm"));
            
            dtool_design::toggle(ui, &mut options.pigment_blend, text("blend.pigment"));
        });
    });
}

pub fn render_paint_context_bar(
    ui: &mut egui::Ui,
    tool_id: &str,
    options: &mut PaintToolOptions,
) {
    match dtool_core::context_toolbar::canonical_tool(tool_id).unwrap_or(tool_id) {
        "tool.flood_fill" => {
            let mut bar = FloodFillContextBar {
                tolerance: options.params.tolerance * 100.0,
                contiguous: options.params.contiguous,
                source_current_layer: true,
            };
            render_flood_fill_context_bar(ui, &mut bar);
            options.params.tolerance = bar.tolerance / 100.0;
            options.params.contiguous = bar.contiguous;
        }
        _ => {}
    }
}
