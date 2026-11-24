use dtool_core::colour::{ColourModel, ProfileStore, RenderingIntent, OcioConfig};
use dtool_design;
use egui;
use std::path::PathBuf;

pub struct ColourSettingsUi {
    pub rgb: String,
    pub rgb32: String,
    pub cmyk: String,
    pub grey: String,
    pub lab: String,
    pub intent: RenderingIntent,
    pub bpc: bool,
    pub ocio_config_path: Option<PathBuf>,
    pub ocio_spaces: Vec<String>,
}

impl Default for ColourSettingsUi {
    fn default() -> Self {
        Self {
            rgb: "sRGB IEC61966-2.1".into(),
            rgb32: "sRGB IEC61966-2.1".into(),
            cmyk: "CMYK Default".into(),
            grey: "Grey Gamma 2.2".into(),
            lab: "LAB D50".into(),
            intent: RenderingIntent::RelativeColorimetric,
            bpc: true,
            ocio_config_path: None,
            ocio_spaces: Vec::new(),
        }
    }
}

pub fn render(ctx: &mut egui::Context, store: &ProfileStore, ui_state: &mut ColourSettingsUi) {
    let rgb_profiles = store.list(ColourModel::Rgb);
    let cmyk_profiles = store.list(ColourModel::Cmyk);
    let lab_profiles = store.list(ColourModel::Lab);
    let grey_profiles = store.list(ColourModel::Grey);

    dtool_design::panel_frame(ctx, "settings.colour.rgb", |ui| {
        let selected_idx = rgb_profiles.iter().position(|p| *p == &ui_state.rgb).unwrap_or(0);
        let combo = dtool_design::dropdown(ui, &rgb_profiles, selected_idx);
        if combo.changed() {
            if let Some(idx) = combo.value() {
                ui_state.rgb = rgb_profiles[idx].to_string();
            }
        }
    });

    dtool_design::panel_frame(ctx, "settings.colour.rgb32", |ui| {
        let selected_idx = rgb_profiles.iter().position(|p| *p == &ui_state.rgb32).unwrap_or(0);
        let combo = dtool_design::dropdown(ui, &rgb_profiles, selected_idx);
        if combo.changed() {
            if let Some(idx) = combo.value() {
                ui_state.rgb32 = rgb_profiles[idx].to_string();
            }
        }
    });

    dtool_design::panel_frame(ctx, "settings.colour.cmyk", |ui| {
        let selected_idx = cmyk_profiles.iter().position(|p| *p == &ui_state.cmyk).unwrap_or(0);
        let combo = dtool_design::dropdown(ui, &cmyk_profiles, selected_idx);
        if combo.changed() {
            if let Some(idx) = combo.value() {
                ui_state.cmyk = cmyk_profiles[idx].to_string();
            }
        }
    });

    dtool_design::panel_frame(ctx, "settings.colour.grey", |ui| {
        let selected_idx = grey_profiles.iter().position(|p| *p == &ui_state.grey).unwrap_or(0);
        let combo = dtool_design::dropdown(ui, &grey_profiles, selected_idx);
        if combo.changed() {
            if let Some(idx) = combo.value() {
                ui_state.grey = grey_profiles[idx].to_string();
            }
        }
    });

    dtool_design::panel_frame(ctx, "settings.colour.lab", |ui| {
        let selected_idx = lab_profiles.iter().position(|p| *p == &ui_state.lab).unwrap_or(0);
        let combo = dtool_design::dropdown(ui, &lab_profiles, selected_idx);
        if combo.changed() {
            if let Some(idx) = combo.value() {
                ui_state.lab = lab_profiles[idx].to_string();
            }
        }
    });

    dtool_design::panel_frame(ctx, "settings.colour.intent", |ui| {
        let intents = [
            RenderingIntent::Perceptual,
            RenderingIntent::RelativeColorimetric,
            RenderingIntent::Saturation,
            RenderingIntent::AbsoluteColorimetric,
        ];
        let labels = [
            "colour.intent.perceptual",
            "colour.intent.relative",
            "colour.intent.saturation",
            "colour.intent.absolute",
        ];
        let selected_idx = intents.iter().position(|i| *i == ui_state.intent).unwrap_or(1);
        let combo = dtool_design::dropdown(ui, &labels, selected_idx);
        if combo.changed() {
            if let Some(idx) = combo.value() {
                ui_state.intent = intents[idx];
            }
        }
    });

    dtool_design::panel_frame(ctx, "colour.bpc", |ui| {
        let toggle = dtool_design::toggle(ui, ui_state.bpc);
        if toggle.changed() {
            ui_state.bpc = toggle.value();
        }
    });

    dtool_design::panel_frame(ctx, "settings.colour.ocio_file", |ui| {
        if let Some(path) = &ui_state.ocio_config_path {
            ui.label(path.to_string_lossy().to_string());
        } else {
            ui.label(dtool_strings::get("settings.colour.ocio_none"));
        }

        if dtool_design::button(ui, "settings.colour.select").clicked() {
            if let Some(path) = rfd::FileDialog::new().add_filter("OCIO Config", &["ocio"]).pick_file() {
                if let Ok(config) = OcioConfig::load(&path) {
                    ui_state.ocio_config_path = Some(path);
                    ui_state.ocio_spaces = config.spaces;
                }
            }
        }
    });
}
