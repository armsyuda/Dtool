use dtool_design::{apply, dark};
#[test]
fn common_widgets_use_reference_density_instead_of_toolbar_height() {
    let ctx=egui::Context::default(); apply(&ctx,&dark());
    assert_eq!(ctx.style_of(ctx.theme()).text_styles[&egui::TextStyle::Body].size,12.0);
    assert_eq!(ctx.style_of(ctx.theme()).spacing.interact_size.y,20.0);
    assert_eq!(ctx.style_of(ctx.theme()).spacing.slider_rail_height,3.0);
    assert_eq!(ctx.style_of(ctx.theme()).visuals.text_edit_bg_color,Some(egui::Color32::from_rgb(31,31,31)));
    let mut button=None;
    let mut output=ctx.run_ui(egui::RawInput::default(),|ui| {button=Some(dtool_design::button(ui,"OK"));}); output.textures_delta.clear();
    let rect=button.unwrap().rect;
    assert!(rect.height()<=21.0,"ordinary button must not inherit the52.8dip toolbar: {rect:?}"); assert!(rect.width()>=75.0);
}
