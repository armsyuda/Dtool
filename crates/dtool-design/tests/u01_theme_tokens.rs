use dtool_design::theme::{
    current_style, dark, light, palette_for, set_style, ThemeAdjust, ThemeStyle,
};
use dtool_design::tokens::{color, missing_tokens_seen, number, size};
use egui::Color32;

fn default_adjust(style: ThemeStyle) -> ThemeAdjust {
    ThemeAdjust {
        style,
        background_grey: 0.25,
        ui_brightness: 0.5,
        text_contrast: 0.5,
        high_contrast: false,
    }
}

fn channel_delta(a: Color32, b: Color32) -> i32 {
    let d = |x: u8, y: u8| (x as i32 - y as i32).unsigned_abs() as i32;
    d(a.r(), b.r()).max(d(a.g(), b.g())).max(d(a.b(), b.b()))
}

#[test]
fn dark_palette_reads_tokens() {
    let palette = dark();
    assert_eq!(palette.style, ThemeStyle::Dark);
    assert_eq!(palette.menu_bar, Color32::from_rgb(0x28, 0x28, 0x28));
    assert_eq!(palette.panel, Color32::from_rgb(0x38, 0x38, 0x38));
    assert_eq!(palette.text, Color32::from_rgb(0xF0, 0xF0, 0xF0));
    assert_eq!(palette.accent, Color32::from_gray(0xB8));
    assert_eq!(palette.pasteboard, Color32::from_gray(0x28));
}

#[test]
fn palette_for_light_style() {
    let palette = palette_for(&default_adjust(ThemeStyle::Light));
    assert_eq!(palette.style, ThemeStyle::Light);
    assert_eq!(palette.panel, Color32::from_rgb(0xF2, 0xF2, 0xF2));
    assert_eq!(palette.accent, Color32::from_gray(0x58));
    assert_eq!(palette.pasteboard, Color32::from_rgb(0xBD, 0xBD, 0xBD));
}

#[test]
fn light_lookup_does_not_leak_into_dark() {
    set_style(ThemeStyle::Dark);
    let before = color("panel");
    let _ = light();
    assert_eq!(current_style(), ThemeStyle::Dark);
    assert_eq!(color("panel"), before);
    assert_eq!(before, Color32::from_rgb(0x38, 0x38, 0x38));
}

#[test]
fn default_adjust_keeps_pasteboard_within_8() {
    let palette = palette_for(&default_adjust(ThemeStyle::Dark));
    let target = Color32::from_gray(0x28);
    assert!(channel_delta(palette.pasteboard, target) <= 8);
    assert_eq!(palette.pasteboard, target);
}

#[test]
fn high_contrast_dark_text_is_white() {
    let mut settings = default_adjust(ThemeStyle::Dark);
    settings.high_contrast = true;
    let palette = palette_for(&settings);
    assert_eq!(palette.text, Color32::WHITE);
    assert!(palette.high_contrast);
}

#[test]
fn unknown_color_token_is_magenta() {
    let key = "u01.fix.unknown.color";
    assert_eq!(color(key), Color32::from_rgb(255, 0, 255));
    assert!(missing_tokens_seen().iter().any(|seen| seen == key));
}

#[test]
fn unknown_number_token_reported() {
    let key = "u01.fix.unknown.number";
    assert_eq!(number(key), 0.0);
    assert!(missing_tokens_seen().iter().any(|seen| seen == key));
}

#[test]
fn component_sizes_come_from_tokens() {
    assert_eq!(size("icon.size.md"), 16.0);
    assert_eq!(size("icon.size.lg"), 20.0);
}
