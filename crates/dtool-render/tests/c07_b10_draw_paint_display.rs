use dtool_core::colour::{ColourModel, ConvertCtx, ProfileStore};
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint};

fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Colour {
    Colour::from_managed(dtool_core::colour::Colour::cmyk(c, m, y, k, 1.0))
}

fn expected(colour: &Colour) -> egui::Color32 {
    let services = dtool_core::colour::services().read().unwrap();
    let ctx = ConvertCtx { from: services.preferences.cmyk.clone(), to: ProfileStore::SRGB.to_string(), intent: services.preferences.intent, bpc: services.preferences.bpc };
    let out = dtool_core::colour::convert::convert(&colour.managed(), ColourModel::Rgb, &ctx, &services.profiles).unwrap();
    let to_u8 = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(to_u8(out.values[0]), to_u8(out.values[1]), to_u8(out.values[2]), colour.a)
}

fn collect(shape: &egui::Shape, out: &mut Vec<egui::Color32>) {
    match shape {
        egui::Shape::Rect(rect) => out.push(rect.fill),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, out)),
        _ => {}
    }
}

fn fills_of(paint: &Paint) -> Vec<egui::Color32> {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.allocate_space(egui::vec2(16.0, 8.0));
        dtool_render::display::draw_paint(ui, paint);
    });
    output.textures_delta.clear();
    let mut fills = Vec::new();
    output.shapes.iter().for_each(|clipped| collect(&clipped.shape, &mut fills));
    fills
}

#[test]
fn c07_b10_draw_paint_preview_uses_display_path() {
    let ink = cmyk(30.0, 10.0, 70.0, 0.0);
    let fills = fills_of(&Paint::Solid { colour: ink });
    assert!(fills.contains(&expected(&ink)), "solid preview goes through the CMYK display profile");
    assert!(!fills.contains(&egui::Color32::from_rgba_unmultiplied(ink.r, ink.g, ink.b, ink.a)), "naive CMYK preview");
    let rgb = Colour::new(10, 20, 250, 255);
    assert!(fills_of(&Paint::Solid { colour: rgb }).contains(&egui::Color32::from_rgba_unmultiplied(10, 20, 250, 255)));
    let stop_ink = cmyk(0.0, 0.0, 0.0, 100.0);
    let gradient = Paint::Gradient { kind: GradientKind::Linear, stops: vec![GradientStop { offset: 0.0, colour: stop_ink }, GradientStop { offset: 1.0, colour: stop_ink }], geom: GradientGeom::from_legacy_angle(0.0, [0.0, 0.0, 16.0, 8.0]) };
    let fills = fills_of(&gradient);
    assert!(fills.contains(&expected(&stop_ink)), "gradient stops go through the CMYK display profile");
    let start = cmyk(0.0, 90.0, 80.0, 0.0);
    let end = cmyk(80.0, 0.0, 20.0, 10.0);
    let stops = vec![GradientStop { offset: 0.0, colour: start }, GradientStop { offset: 1.0, colour: end }];
    let Paint::Gradient { stops: shown, .. } = dtool_core::paint::display_paint(&Paint::Gradient { kind: GradientKind::Linear, stops, geom: GradientGeom::from_legacy_angle(0.0, [0.0, 0.0, 16.0, 8.0]) }, None, &dtool_core::paint::DisplayColourCtx::app_default()) else { panic!("gradient") };
    let mid = dtool_render::display::interpolate_stops(&shown, 0.5);
    let (a, b) = (expected(&start), expected(&end));
    let half = |x: u8, y: u8| ((x as f32 + y as f32) / 2.0).round() as i32;
    for (got, want) in [(mid.r, half(a.r(), b.r())), (mid.g, half(a.g(), b.g())), (mid.b, half(a.b(), b.b()))] {
        assert!((got as i32 - want).abs() <= 1, "gradient midpoint mixes displayed stops: {got} vs {want}");
    }
}
