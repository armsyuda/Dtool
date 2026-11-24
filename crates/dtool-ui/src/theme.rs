use crate::settings::{UiSettings, UiStyle};

pub use dtool_design::{Palette, dark, light};

pub fn adjust(p: &Palette, settings: &UiSettings) -> Palette {
    dtool_design::adjust(
        p,
        &dtool_design::ThemeAdjust {
            style: match settings.ui_style {
                UiStyle::Dark => dtool_design::ThemeStyle::Dark,
                UiStyle::Light => dtool_design::ThemeStyle::Light,
            },
            background_grey: settings.background_grey,
            ui_brightness: settings.ui_brightness,
            text_contrast: settings.text_contrast,
            high_contrast: settings.high_contrast,
        },
    )
}

pub fn palette_for(settings: &UiSettings) -> Palette {
    dtool_design::palette_for(&dtool_design::ThemeAdjust {
        style: match settings.ui_style {
            UiStyle::Dark => dtool_design::ThemeStyle::Dark,
            UiStyle::Light => dtool_design::ThemeStyle::Light,
        },
        background_grey: settings.background_grey,
        ui_brightness: settings.ui_brightness,
        text_contrast: settings.text_contrast,
        high_contrast: settings.high_contrast,
    })
}

pub fn apply(ctx: &egui::Context, palette: &Palette) {
    dtool_design::apply(ctx, palette);
}
