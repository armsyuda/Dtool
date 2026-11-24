#[path = "support/test_home.rs"]
mod test_home;
#[test]
fn icon_cache_clears_on_theme_or_scale_change() {
    let _home = test_home::enter("icon_cache_clears_on_theme_or_scale_change");
    let mut shell = dtool_ui::shell::AppShell::new();
    let ctx = egui::Context::default();
    shell.apply_theme(&ctx);
    assert!(shell.icon_cache.tool_raster("tool.move", egui::Color32::WHITE, 16).is_some());
    assert_eq!(shell.icon_cache.len(), 1);
    assert!(!shell.apply_theme(&ctx));
    assert_eq!(shell.icon_cache.len(), 1);
    shell.settings.high_contrast = !shell.settings.high_contrast;
    assert!(shell.apply_theme(&ctx));
    assert!(shell.icon_cache.is_empty());
    assert!(shell.icon_cache.tool_raster("tool.move", egui::Color32::WHITE, 16).is_some());
    let mut raw = egui::RawInput::default();
    raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(2.0);
    ctx.begin_pass(raw);
    assert_eq!(ctx.pixels_per_point(), 2.0);
    shell.apply_theme(&ctx);
    ctx.end_pass().textures_delta.clear();
    assert!(shell.icon_cache.is_empty());
}