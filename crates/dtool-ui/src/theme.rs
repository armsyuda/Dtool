use crate::settings::UiSettings;

pub use dtool_design::{Palette, dark};

pub fn palette_for(settings: &UiSettings) -> Palette {
    dtool_design::palette_for(&dtool_design::ThemeAdjust {
        style: dtool_design::ThemeStyle::Dark,
        background_grey: settings.background_grey,
        ui_brightness: settings.ui_brightness,
        text_contrast: settings.text_contrast,
        high_contrast: settings.high_contrast,
    })
}

pub fn apply(ctx: &egui::Context, palette: &Palette) {
    ctx.set_theme(egui::Theme::Dark);
    dtool_design::apply(ctx, palette);
}
