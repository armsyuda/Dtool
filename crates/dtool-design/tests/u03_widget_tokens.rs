use dtool_design::theme::{current_style, set_style, widget_token, widget_token_for, ThemeStyle};
use egui::Color32;

const DARK_FOCUS: Color32 = Color32::from_gray(0xB8);

#[test]
fn dark_widget_tokens_follow_design_spec() {
    let dark = |name| widget_token_for(ThemeStyle::Dark, name);
    assert_eq!(dark("bg"), Some(Color32::from_rgb(0x4F, 0x4F, 0x4F)));
    assert_eq!(dark("bg_hover"), Some(Color32::from_rgb(0x40, 0x40, 0x40)));
    assert_eq!(dark("bg_disabled"), Some(Color32::from_rgb(0x42, 0x42, 0x42)));
    assert_eq!(dark("fg_disabled"), Some(Color32::from_rgb(0x93, 0x93, 0x93)));
    assert_eq!(dark("focus"), Some(DARK_FOCUS));
    assert_eq!(dark("mixed"), None);
}

#[test]
fn light_widget_tokens_absent_keep_defaults() {
    assert_eq!(widget_token_for(ThemeStyle::Light, "bg"), None);
    assert_eq!(widget_token_for(ThemeStyle::Light, "focus"), None);
}

#[test]
fn widget_token_reads_the_current_style() {
    set_style(ThemeStyle::Light);
    assert_eq!(current_style(), ThemeStyle::Light);
    assert_eq!(widget_token("bg"), None);
    set_style(ThemeStyle::Dark);
    assert_eq!(widget_token("bg"), widget_token_for(ThemeStyle::Dark, "bg"));
    assert_eq!(widget_token("focus"), Some(DARK_FOCUS));
}
