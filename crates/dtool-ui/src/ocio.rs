use dtool_core::colour::{OcioConfig, OcioEngine};
use dtool_design;
use egui;
use std::path::PathBuf;

pub struct OcioAdjustUi {
    pub source_space: String,
    pub dest_space: String,
}

impl Default for OcioAdjustUi {
    fn default() -> Self {
        Self {
            source_space: String::new(),
            dest_space: String::new(),
        }
    }
}

pub fn render_ocio_adjust(
    ctx: &mut egui::Context,
    config: &OcioConfig,
    ui_state: &mut OcioAdjustUi,
) {
    let spaces = &config.spaces;

    dtool_design::panel_frame(ctx, "adjust.ocio.source", |ui| {
        let selected_idx = spaces.iter().position(|s| *s == &ui_state.source_space).unwrap_or(0);
        let combo = dtool_design::dropdown(ui, spaces, selected_idx);
        if combo.changed() {
            if let Some(idx) = combo.value() {
                ui_state.source_space = spaces[idx].clone();
            }
        }
    });

    dtool_design::panel_frame(ctx, "adjust.ocio.dest", |ui| {
        let selected_idx = spaces.iter().position(|s| *s == &ui_state.dest_space).unwrap_or(0);
        let combo = dtool_design::dropdown(ui, spaces, selected_idx);
        if combo.changed() {
            if let Some(idx) = combo.value() {
                ui_state.dest_space = spaces[idx].clone();
            }
        }
    });
}

pub fn apply_ocio_transform(
    config: &OcioConfig,
    src: &str,
    dst: &str,
    pixels: &mut [f32],
) -> Result<(), dtool_core::CommandError> {
    let engine = OcioEngine::open(config)?;
    engine.apply(src, dst, pixels)
}
