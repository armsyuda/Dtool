#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::settings::UiStyle;
use dtool_ui::shell::AppShell;

#[test]
fn theme_applies_once_and_follows_settings() {
    let _home = test_home::enter("theme_applies_once_and_follows_settings");
    let ctx = egui::Context::default();
    let mut shell = AppShell::new();
    assert!(shell.apply_theme(&ctx));
    let expected = dtool_ui::theme::palette_for(&shell.settings);
    assert_eq!(ctx.global_style().visuals.selection.bg_fill, expected.accent);
    assert!(!shell.apply_theme(&ctx));
    let before = ctx.global_style().visuals.override_text_color;
    let stroke_before = ctx.global_style().visuals.widgets.noninteractive.bg_stroke;
    shell.settings.ui_style = UiStyle::Light;
    shell.settings.high_contrast = true;
    assert!(shell.apply_theme(&ctx));
    assert_ne!(ctx.global_style().visuals.override_text_color, before);
    assert_ne!(ctx.global_style().visuals.widgets.noninteractive.bg_stroke, stroke_before);
}
